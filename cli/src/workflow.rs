use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::models::{
    AppSettings, EngineState, ExecutionStage, OpenPr, PrExecutionResult, RunSnapshot, RunStatus,
};
use crate::shell::{
    commit_and_push_if_needed, initialize_monthly_fix_counter, is_codex_review_prompt_conflict,
    record_monthly_fixed_pr, render_exec_error, run_shell, run_with_retry,
    run_with_retry_streaming, sh_quote, sync_monthly_fix_counter_into_state,
};
use crate::store::{
    StorePaths, load_engine_state, load_settings, load_snapshot, save_engine_state, save_snapshot,
};

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn append_log(snapshot: &mut RunSnapshot, message: impl AsRef<str>) {
    snapshot
        .log_lines
        .push(format!("[{}] {}", now().to_rfc3339(), message.as_ref()));
    if snapshot.log_lines.len() > 500 {
        let keep_from = snapshot.log_lines.len() - 500;
        snapshot.log_lines.drain(0..keep_from);
    }
}

fn log_step(snapshot: &mut RunSnapshot, message: impl AsRef<str>, verbose: bool) {
    append_log(snapshot, message.as_ref());
    if verbose {
        println!("{}", message.as_ref());
    }
}

fn validate_required_commands() -> Result<()> {
    let checks = [
        ("command -v git", "git CLI not found"),
        ("command -v gh", "gh CLI not found"),
        ("command -v codex", "codex CLI not found"),
    ];

    for (command, message) in checks {
        let result = run_shell(command, None, false).map_err(|e| anyhow!(render_exec_error(&e)))?;
        if result.exit_code != 0 {
            bail!(message);
        }
    }

    Ok(())
}

fn is_directory_empty(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("failed reading directory: {}", path.display()))?;
    Ok(entries.next().is_none())
}

fn ensure_repo_ready(settings: &AppSettings) -> Result<()> {
    if settings.repo_path.trim().is_empty() {
        bail!("settings.repo_path is empty");
    }
    if settings.repo_path.starts_with("http://")
        || settings.repo_path.starts_with("https://")
        || settings.repo_path.starts_with("git@")
    {
        bail!(
            "settings.repo_path must be a local directory path, not a remote URL; put remote URL in settings.repo_clone_url"
        );
    }

    let repo_path = Path::new(&settings.repo_path);
    if !repo_path.exists() {
        fs::create_dir_all(repo_path).with_context(|| {
            format!(
                "failed to create repo_path directory: {}",
                repo_path.display()
            )
        })?;
    }

    if is_directory_empty(repo_path)? {
        if settings.repo_clone_url.trim().is_empty() {
            bail!("repo_path is empty and settings.repo_clone_url is empty, cannot auto clone");
        }
        run_with_retry(
            &format!(
                "git clone {} {}",
                sh_quote(&settings.repo_clone_url),
                sh_quote(&settings.repo_path)
            ),
            None,
            settings.max_command_retries,
            settings.retry_delay_seconds,
        )
        .map_err(|e| anyhow!(render_exec_error(&e)))?;
    }

    let repo_check = run_shell(
        "git rev-parse --is-inside-work-tree",
        Some(&settings.repo_path),
        false,
    )
    .map_err(|e| anyhow!(render_exec_error(&e)))?;
    if repo_check.exit_code != 0 {
        bail!(
            "repo_path is not a git repository: {}",
            Path::new(&settings.repo_path).display()
        );
    }

    Ok(())
}

fn validate_command_templates(settings: &AppSettings) -> Result<()> {
    if settings
        .review_command_template
        .contains("codex review --pr")
    {
        bail!(
            "review_command_template contains unsupported flags (--pr). Please use `codex review --base {{DEFAULT_BRANCH}}` style."
        );
    }
    if settings
        .fix_command_template
        .trim_start()
        .starts_with("codex fix")
    {
        bail!(
            "fix_command_template uses unsupported `codex fix`. Please use `codex exec \"...\"`."
        );
    }
    Ok(())
}

fn rollback_uncommitted_changes(settings: &AppSettings) -> Result<()> {
    let status = run_shell("git status --porcelain", Some(&settings.repo_path), true)
        .map_err(|e| anyhow!(render_exec_error(&e)))?;
    if status.stdout.trim().is_empty() {
        return Ok(());
    }

    run_shell("git reset --hard HEAD", Some(&settings.repo_path), true)
        .map_err(|e| anyhow!(render_exec_error(&e)))?;
    run_shell("git clean -fd", Some(&settings.repo_path), true)
        .map_err(|e| anyhow!(render_exec_error(&e)))?;
    Ok(())
}

fn sync_repository(settings: &AppSettings) -> Result<()> {
    rollback_uncommitted_changes(settings)?;

    run_with_retry(
        "git fetch --all --prune",
        Some(&settings.repo_path),
        settings.max_command_retries,
        settings.retry_delay_seconds,
    )
    .map_err(|e| anyhow!(render_exec_error(&e)))?;

    run_with_retry(
        &format!("git checkout {}", sh_quote(&settings.default_branch)),
        Some(&settings.repo_path),
        settings.max_command_retries,
        settings.retry_delay_seconds,
    )
    .map_err(|e| anyhow!(render_exec_error(&e)))?;

    run_with_retry(
        &format!(
            "git pull --ff-only origin {}",
            sh_quote(&settings.default_branch)
        ),
        Some(&settings.repo_path),
        settings.max_command_retries,
        settings.retry_delay_seconds,
    )
    .map_err(|e| anyhow!(render_exec_error(&e)))?;

    Ok(())
}

fn list_open_prs(settings: &AppSettings) -> Result<Vec<OpenPr>> {
    let command = "gh pr list --state open --limit 200 --json number,title,headRefName,url,updatedAt,author,assignees,reviews,reviewRequests,comments,latestReviews";
    let result = run_with_retry(
        command,
        Some(&settings.repo_path),
        settings.max_command_retries,
        settings.retry_delay_seconds,
    )
    .map_err(|e| anyhow!(render_exec_error(&e)))?;

    let prs: Vec<OpenPr> = serde_json::from_str(&result.stdout).with_context(|| {
        format!(
            "failed parsing gh pr json output, stdout snippet: {}",
            result.stdout.chars().take(120).collect::<String>()
        )
    })?;

    Ok(prs)
}

fn checkout_pr(pr_number: u64, settings: &AppSettings) -> Result<()> {
    run_with_retry(
        &format!("gh pr checkout {pr_number}"),
        Some(&settings.repo_path),
        settings.max_command_retries,
        settings.retry_delay_seconds,
    )
    .map_err(|e| anyhow!(render_exec_error(&e)))?;
    Ok(())
}

fn expand_template(
    template: &str,
    pr: &OpenPr,
    settings: &AppSettings,
    report_path: &Path,
) -> String {
    template
        .replace("{{PR_NUMBER}}", &pr.number.to_string())
        .replace("{{PR_TITLE}}", &sh_quote(&pr.title))
        .replace("{{PR_URL}}", &sh_quote(&pr.url))
        .replace("{{PR_BRANCH}}", &sh_quote(&pr.head_ref_name))
        .replace("{{DEFAULT_BRANCH}}", &sh_quote(&settings.default_branch))
        .replace("{{REPO_PATH}}", &sh_quote(&settings.repo_path))
        .replace("{{WORK_DIR}}", &sh_quote(&settings.repo_path))
        .replace(
            "{{REPORT_PATH}}",
            &sh_quote(&report_path.display().to_string()),
        )
}

fn write_report(
    report_path: &Path,
    pr: &OpenPr,
    command: &str,
    result: &crate::shell::CommandResult,
    step: &str,
) -> Result<()> {
    let content = format!(
        "# PR #{} Report\n\n- Title: {}\n- URL: {}\n- Step: {}\n- Time: {}\n- Command: `{}`\n- Exit Code: {}\n\n## stdout\n\n```\n{}\n```\n\n## stderr\n\n```\n{}\n```\n",
        pr.number,
        pr.title,
        pr.url,
        step,
        now().to_rfc3339(),
        command,
        result.exit_code,
        result.stdout,
        result.stderr
    );
    fs::write(report_path, content)
        .with_context(|| format!("failed writing report: {}", report_path.display()))?;
    Ok(())
}

fn fetch_open_prs_with_state(
    paths: &StorePaths,
    sync: bool,
) -> Result<(AppSettings, Vec<OpenPr>, HashSet<u64>)> {
    let state = load_engine_state(paths)?;
    initialize_monthly_fix_counter(&state);

    let settings = load_settings(paths)?;
    validate_command_templates(&settings)?;
    validate_required_commands()?;
    ensure_repo_ready(&settings)?;
    if sync {
        sync_repository(&settings)?;
    }

    let mut prs = list_open_prs(&settings)?;
    prs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    let processed_set: HashSet<u64> = state.processed_pr_numbers.into_iter().collect();
    Ok((settings, prs, processed_set))
}

fn get_current_gh_login(settings: &AppSettings) -> Option<String> {
    let result = run_shell("gh api user --jq .login", Some(&settings.repo_path), false).ok()?;
    if result.exit_code != 0 {
        return None;
    }
    let login = result.stdout.trim();
    if login.is_empty() {
        None
    } else {
        Some(login.to_ascii_lowercase())
    }
}

fn value_contains_login(value: &serde_json::Value, login_lower: &str) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(v)) = map.get("login")
                && v.eq_ignore_ascii_case(login_lower)
            {
                return true;
            }
            map.values().any(|v| value_contains_login(v, login_lower))
        }
        serde_json::Value::Array(arr) => arr.iter().any(|v| value_contains_login(v, login_lower)),
        _ => false,
    }
}

fn pr_involves_login(pr: &OpenPr, login_lower: &str) -> bool {
    if pr.author.login.eq_ignore_ascii_case(login_lower) {
        return true;
    }
    value_contains_login(&pr.assignees, login_lower)
        || value_contains_login(&pr.reviews, login_lower)
        || value_contains_login(&pr.review_requests, login_lower)
        || value_contains_login(&pr.comments, login_lower)
        || value_contains_login(&pr.latest_reviews, login_lower)
}

fn pr_has_commit_by_login(
    settings: &AppSettings,
    pr_number: u64,
    login_lower: &str,
) -> Result<bool> {
    let command = format!("gh pr view {} --json commits", pr_number);
    let result = run_with_retry(
        &command,
        Some(&settings.repo_path),
        settings.max_command_retries,
        settings.retry_delay_seconds,
    )
    .map_err(|e| anyhow!(render_exec_error(&e)))?;

    let value: serde_json::Value = serde_json::from_str(&result.stdout).with_context(|| {
        format!(
            "failed parsing gh pr view commits json for PR #{}",
            pr_number
        )
    })?;
    Ok(value_contains_login(
        value.get("commits").unwrap_or(&serde_json::Value::Null),
        login_lower,
    ))
}

pub fn print_pr_list(paths: &StorePaths, sync: bool) -> Result<Vec<OpenPr>> {
    let (settings, prs, processed_set) = fetch_open_prs_with_state(paths, sync)?;
    let my_login = get_current_gh_login(&settings);

    let mut filtered_prs: Vec<OpenPr> = Vec::new();
    for pr in prs {
        if pr.title.to_ascii_lowercase().contains("wip") {
            continue;
        }

        let hide = if let Some(login) = &my_login {
            if pr_involves_login(&pr, login) {
                true
            } else {
                pr_has_commit_by_login(&settings, pr.number, login).unwrap_or(false)
            }
        } else {
            false
        };

        if !hide {
            filtered_prs.push(pr);
        }
    }

    if filtered_prs.is_empty() {
        println!("no open PRs to show (after participant filter)");
        return Ok(Vec::new());
    }

    println!("open PRs:");
    for (idx, pr) in filtered_prs.iter().enumerate() {
        let state = if processed_set.contains(&pr.number) {
            "processed"
        } else {
            "new"
        };
        let author = if let Some(name) = &pr.author.name {
            if name.trim().is_empty() {
                pr.author.login.clone()
            } else {
                format!("{} ({})", name.trim(), pr.author.login)
            }
        } else {
            pr.author.login.clone()
        };
        println!(
            "{:>3}. #{} [{}] {} | author: {}",
            idx + 1,
            pr.number,
            state,
            pr.title,
            author
        );
    }

    Ok(filtered_prs)
}

fn execute_pr(
    paths: &StorePaths,
    settings: &AppSettings,
    pr: &OpenPr,
    state: &mut EngineState,
    snapshot: &mut RunSnapshot,
    ordinal: usize,
    total: usize,
    verbose: bool,
) -> Result<PrExecutionResult> {
    snapshot.current_index = ordinal;
    snapshot.current_pr_number = Some(pr.number);
    snapshot.current_pr_title = Some(pr.title.clone());
    snapshot.stage = ExecutionStage::ReviewingPr;
    log_step(
        snapshot,
        format!(
            "[{}/{}] Processing PR #{}: {}",
            ordinal, total, pr.number, pr.title
        ),
        verbose,
    );
    save_snapshot(paths, snapshot)?;

    let report_name = format!(
        "pr-{}-{}.md",
        pr.number,
        now().to_rfc3339().replace(':', "-")
    );
    let report_path = paths.reports.join(report_name);

    log_step(snapshot, format!("Checkout PR #{}", pr.number), verbose);
    checkout_pr(pr.number, settings)?;

    let mut review_cmd = expand_template(
        &settings.review_command_template,
        pr,
        settings,
        &report_path,
    );
    log_step(snapshot, format!("Review PR #{}", pr.number), verbose);
    let review_result = match run_with_retry_streaming(
        &review_cmd,
        Some(&settings.repo_path),
        settings.max_command_retries,
        settings.retry_delay_seconds,
        verbose,
        Some("[review] "),
    ) {
        Ok(result) => result,
        Err(err) if is_codex_review_prompt_conflict(&err) => {
            review_cmd = format!("codex review --base {}", sh_quote(&settings.default_branch));
            log_step(
                snapshot,
                "Detected codex review --base prompt conflict, fallback to bare --base",
                verbose,
            );
            run_with_retry_streaming(
                &review_cmd,
                Some(&settings.repo_path),
                settings.max_command_retries,
                settings.retry_delay_seconds,
                verbose,
                Some("[review] "),
            )
            .map_err(|e| anyhow!(render_exec_error(&e)))?
        }
        Err(err) => return Err(anyhow!(render_exec_error(&err))),
    };
    write_report(&report_path, pr, &review_cmd, &review_result, "review")?;

    snapshot.stage = ExecutionStage::FixingPr;
    save_snapshot(paths, snapshot)?;

    let fix_cmd = expand_template(&settings.fix_command_template, pr, settings, &report_path);
    log_step(snapshot, format!("Fix PR #{}", pr.number), verbose);
    let fix_result = run_with_retry_streaming(
        &fix_cmd,
        Some(&settings.repo_path),
        settings.max_command_retries,
        settings.retry_delay_seconds,
        verbose,
        Some("[fix] "),
    )
    .map_err(|e| anyhow!(render_exec_error(&e)))?;

    let mut pushed = false;
    if settings.auto_push_enabled {
        snapshot.stage = ExecutionStage::PushingChanges;
        save_snapshot(paths, snapshot)?;
        log_step(
            snapshot,
            format!("Push changes for PR #{}", pr.number),
            verbose,
        );
        pushed = commit_and_push_if_needed(
            pr,
            &settings.repo_path,
            settings.max_command_retries,
            settings.retry_delay_seconds,
        )
        .map_err(|e| anyhow!(render_exec_error(&e)))?;
    }

    if review_result.exit_code == 0 && fix_result.exit_code == 0 && pushed {
        if record_monthly_fixed_pr(pr.number) {
            log_step(
                snapshot,
                format!(
                    "Counter updated: PR #{} counted for this calendar month",
                    pr.number
                ),
                verbose,
            );
            sync_monthly_fix_counter_into_state(state);
            save_engine_state(paths, state)?;
        } else {
            log_step(
                snapshot,
                format!(
                    "Counter unchanged: PR #{} already counted this calendar month",
                    pr.number
                ),
                verbose,
            );
        }
    }

    Ok(PrExecutionResult {
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        review_exit_code: review_result.exit_code,
        fix_exit_code: fix_result.exit_code,
        pushed,
        report_path: report_path.display().to_string(),
        error_message: None,
    })
}

pub fn run_workflow(paths: &StorePaths, verbose: bool) -> Result<RunSnapshot> {
    let settings = load_settings(paths)?;
    let mut state = load_engine_state(paths)?;
    initialize_monthly_fix_counter(&state);

    let mut snapshot = RunSnapshot {
        started_at: Some(now()),
        finished_at: None,
        status: RunStatus::Running,
        stage: ExecutionStage::SyncingRepo,
        total_prs: 0,
        current_index: 0,
        current_pr_number: None,
        current_pr_title: None,
        error_message: None,
        report: Vec::new(),
        log_lines: Vec::new(),
    };
    log_step(&mut snapshot, "Start run", verbose);
    save_snapshot(paths, &snapshot)?;

    log_step(&mut snapshot, "Validate required commands", verbose);
    if let Err(err) = validate_required_commands() {
        snapshot.status = RunStatus::Failed;
        snapshot.stage = ExecutionStage::Failed;
        snapshot.error_message = Some(err.to_string());
        snapshot.finished_at = Some(now());
        log_step(&mut snapshot, format!("Validation failed: {err}"), verbose);
        save_snapshot(paths, &snapshot)?;
        return Ok(snapshot);
    }

    log_step(
        &mut snapshot,
        "Prepare repository (auto clone if empty)",
        verbose,
    );
    if let Err(err) = ensure_repo_ready(&settings) {
        snapshot.status = RunStatus::Failed;
        snapshot.stage = ExecutionStage::Failed;
        snapshot.error_message = Some(err.to_string());
        snapshot.finished_at = Some(now());
        log_step(
            &mut snapshot,
            format!("Repository preparation failed: {err}"),
            verbose,
        );
        save_snapshot(paths, &snapshot)?;
        return Ok(snapshot);
    }

    log_step(&mut snapshot, "Validate command templates", verbose);
    if let Err(err) = validate_command_templates(&settings) {
        snapshot.status = RunStatus::Failed;
        snapshot.stage = ExecutionStage::Failed;
        snapshot.error_message = Some(err.to_string());
        snapshot.finished_at = Some(now());
        log_step(
            &mut snapshot,
            format!("Template validation failed: {err}"),
            verbose,
        );
        save_snapshot(paths, &snapshot)?;
        return Ok(snapshot);
    }

    log_step(&mut snapshot, "Sync repository", verbose);
    if let Err(err) = sync_repository(&settings) {
        snapshot.status = RunStatus::Failed;
        snapshot.stage = ExecutionStage::Failed;
        snapshot.error_message = Some(err.to_string());
        snapshot.finished_at = Some(now());
        log_step(&mut snapshot, format!("Sync failed: {err}"), verbose);
        save_snapshot(paths, &snapshot)?;
        return Ok(snapshot);
    }

    snapshot.stage = ExecutionStage::LoadingPrs;
    log_step(&mut snapshot, "Loading open PR list", verbose);
    save_snapshot(paths, &snapshot)?;

    let open_prs = match list_open_prs(&settings) {
        Ok(prs) => prs,
        Err(err) => {
            snapshot.status = RunStatus::Failed;
            snapshot.stage = ExecutionStage::Failed;
            snapshot.error_message = Some(err.to_string());
            snapshot.finished_at = Some(now());
            log_step(&mut snapshot, format!("Load PRs failed: {err}"), verbose);
            save_snapshot(paths, &snapshot)?;
            return Ok(snapshot);
        }
    };

    let processed: HashSet<u64> = state.processed_pr_numbers.iter().copied().collect();
    let mut new_prs: Vec<OpenPr> = open_prs
        .into_iter()
        .filter(|pr| !processed.contains(&pr.number))
        .collect();
    new_prs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    if new_prs.len() > settings.max_prs_per_run {
        new_prs.truncate(settings.max_prs_per_run);
    }

    snapshot.total_prs = new_prs.len();
    let total_prs = snapshot.total_prs;
    log_step(
        &mut snapshot,
        format!("Found {total_prs} new PR(s)"),
        verbose,
    );
    save_snapshot(paths, &snapshot)?;

    if new_prs.is_empty() {
        snapshot.status = RunStatus::Succeeded;
        snapshot.stage = ExecutionStage::Completed;
        snapshot.finished_at = Some(now());
        state.last_run_at = Some(now());
        sync_monthly_fix_counter_into_state(&mut state);
        save_engine_state(paths, &state)?;
        log_step(&mut snapshot, "No new PRs, run finished", verbose);
        save_snapshot(paths, &snapshot)?;
        return Ok(snapshot);
    }

    let mut processed_set: HashSet<u64> = state.processed_pr_numbers.iter().copied().collect();
    let mut failures = 0usize;

    for (idx, pr) in new_prs.iter().enumerate() {
        match execute_pr(
            paths,
            &settings,
            pr,
            &mut state,
            &mut snapshot,
            idx + 1,
            total_prs,
            verbose,
        ) {
            Ok(pr_result) => {
                processed_set.insert(pr.number);
                snapshot.report.push(pr_result);
                log_step(
                    &mut snapshot,
                    format!("PR #{} finished", pr.number),
                    verbose,
                );
            }
            Err(err) => {
                failures += 1;
                log_step(
                    &mut snapshot,
                    format!("PR #{} failed: {err}", pr.number),
                    verbose,
                );
                snapshot.report.push(PrExecutionResult {
                    number: pr.number,
                    title: pr.title.clone(),
                    url: pr.url.clone(),
                    review_exit_code: -1,
                    fix_exit_code: -1,
                    pushed: false,
                    report_path: String::new(),
                    error_message: Some(err.to_string()),
                });
            }
        }

        snapshot.report.sort_by_key(|item| item.number);
        save_snapshot(paths, &snapshot)?;
    }

    let _ = run_shell(
        &format!("git checkout {}", sh_quote(&settings.default_branch)),
        Some(&settings.repo_path),
        false,
    );

    state.processed_pr_numbers = processed_set.into_iter().collect();
    state.processed_pr_numbers.sort_unstable();
    state.last_run_at = Some(now());
    sync_monthly_fix_counter_into_state(&mut state);
    save_engine_state(paths, &state)?;

    if failures > 0 {
        snapshot.status = RunStatus::Failed;
        snapshot.stage = ExecutionStage::Failed;
        snapshot.error_message = Some(format!("{failures} PR(s) failed"));
        log_step(
            &mut snapshot,
            format!("Run completed with {failures} failure(s)"),
            verbose,
        );
    } else {
        snapshot.status = RunStatus::Succeeded;
        snapshot.stage = ExecutionStage::Completed;
        log_step(&mut snapshot, "Run completed successfully", verbose);
    }

    snapshot.finished_at = Some(now());
    save_snapshot(paths, &snapshot)?;
    Ok(snapshot)
}

pub fn run_single_pr_by_number(
    paths: &StorePaths,
    pr_number: u64,
    verbose: bool,
) -> Result<RunSnapshot> {
    let (settings, prs, mut processed_set) = fetch_open_prs_with_state(paths, true)?;
    let pr = prs
        .into_iter()
        .find(|item| item.number == pr_number)
        .ok_or_else(|| anyhow!("PR #{} is not open or not found", pr_number))?;

    let mut state = load_engine_state(paths)?;
    initialize_monthly_fix_counter(&state);
    let mut snapshot = RunSnapshot {
        started_at: Some(now()),
        finished_at: None,
        status: RunStatus::Running,
        stage: ExecutionStage::ReviewingPr,
        total_prs: 1,
        current_index: 0,
        current_pr_number: None,
        current_pr_title: None,
        error_message: None,
        report: Vec::new(),
        log_lines: Vec::new(),
    };
    log_step(
        &mut snapshot,
        format!("Start selected PR run for #{}", pr.number),
        verbose,
    );
    save_snapshot(paths, &snapshot)?;

    match execute_pr(
        paths,
        &settings,
        &pr,
        &mut state,
        &mut snapshot,
        1,
        1,
        verbose,
    ) {
        Ok(result) => {
            processed_set.insert(pr.number);
            snapshot.report.push(result);
            snapshot.status = RunStatus::Succeeded;
            snapshot.stage = ExecutionStage::Completed;
            log_step(
                &mut snapshot,
                format!("Selected PR #{} completed successfully", pr.number),
                verbose,
            );
        }
        Err(err) => {
            snapshot.status = RunStatus::Failed;
            snapshot.stage = ExecutionStage::Failed;
            snapshot.error_message = Some(err.to_string());
            snapshot.report.push(PrExecutionResult {
                number: pr.number,
                title: pr.title.clone(),
                url: pr.url.clone(),
                review_exit_code: -1,
                fix_exit_code: -1,
                pushed: false,
                report_path: String::new(),
                error_message: Some(err.to_string()),
            });
            log_step(
                &mut snapshot,
                format!("Selected PR #{} failed: {err}", pr.number),
                verbose,
            );
        }
    }

    let _ = run_shell(
        &format!("git checkout {}", sh_quote(&settings.default_branch)),
        Some(&settings.repo_path),
        false,
    );

    state.processed_pr_numbers = processed_set.into_iter().collect();
    state.processed_pr_numbers.sort_unstable();
    state.last_run_at = Some(now());
    sync_monthly_fix_counter_into_state(&mut state);
    save_engine_state(paths, &state)?;

    snapshot.finished_at = Some(now());
    snapshot.current_index = 1;
    save_snapshot(paths, &snapshot)?;
    Ok(snapshot)
}

fn latest_file_by_modified_time(dir: &Path) -> Result<Option<PathBuf>> {
    let mut latest: Option<(SystemTime, PathBuf)> = None;

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let metadata = entry.metadata()?;
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        match &latest {
            Some((current, _)) if modified <= *current => {}
            _ => latest = Some((modified, path)),
        }
    }

    Ok(latest.map(|(_, path)| path))
}

pub fn print_status(paths: &StorePaths) -> Result<()> {
    let snapshot = load_snapshot(paths)?;
    println!("status      : {:?}", snapshot.status);
    println!("stage       : {}", snapshot.stage.display_name());
    println!(
        "progress    : {}/{}",
        snapshot.current_index, snapshot.total_prs
    );
    println!(
        "current_pr  : {}",
        snapshot
            .current_pr_number
            .map(|v| format!("#{v}"))
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "last_error  : {}",
        snapshot.error_message.unwrap_or_else(|| "-".to_string())
    );
    Ok(())
}

pub fn print_report(paths: &StorePaths) -> Result<()> {
    let snapshot = load_snapshot(paths)?;

    println!("latest run status: {:?}", snapshot.status);
    println!("stage: {}", snapshot.stage.display_name());
    println!("processed in run: {}", snapshot.report.len());
    if let Some(started) = snapshot.started_at {
        println!("started_at: {}", started.to_rfc3339());
    }
    if let Some(finished) = snapshot.finished_at {
        println!("finished_at: {}", finished.to_rfc3339());
    }

    if snapshot.report.is_empty() {
        println!("no PR report entries yet");
    } else {
        println!("--- PR results ---");
        for item in &snapshot.report {
            let state = if item.error_message.is_some() {
                "failed"
            } else if item.pushed {
                "pushed"
            } else {
                "done"
            };
            println!(
                "#{} {} [{}] report={}",
                item.number, item.title, state, item.report_path
            );
            if let Some(err) = &item.error_message {
                println!("  error: {err}");
            }
        }
    }

    if let Some(path) = latest_file_by_modified_time(&paths.reports)? {
        println!("--- latest markdown report ---");
        println!("file: {}", path.display());
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read report: {}", path.display()))?;
        println!("{content}");
    } else {
        println!(
            "no markdown report file found in {}",
            paths.reports.display()
        );
    }

    Ok(())
}
