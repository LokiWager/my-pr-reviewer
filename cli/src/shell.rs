use anyhow::anyhow;
use chrono::{Local, Utc};
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::models::{EngineState, OpenPr};

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub enum ExecError {
    Io(String),
    NonZero {
        command: String,
        result: CommandResult,
    },
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(f, "{message}"),
            Self::NonZero { command, result } => {
                write!(f, "command failed: {command} (exit {})", result.exit_code)
            }
        }
    }
}

impl std::error::Error for ExecError {}

#[derive(Debug, Clone)]
struct MonthlyFixCounter {
    month_key: String,
    pr_numbers: HashSet<u64>,
}

impl MonthlyFixCounter {
    fn empty_for_current_month() -> Self {
        Self {
            month_key: current_month_key(),
            pr_numbers: HashSet::new(),
        }
    }

    fn rotate_if_needed(&mut self) {
        let now_key = current_month_key();
        if self.month_key != now_key {
            self.month_key = now_key;
            self.pr_numbers.clear();
        }
    }
}

fn current_month_key() -> String {
    Local::now().format("%Y-%m").to_string()
}

fn ansi_color_enabled() -> bool {
    std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map(|v| v != "dumb").unwrap_or(false)
}

fn paint(text: &str, code: &str) -> String {
    if ansi_color_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn colorize_stream_prefix(prefix: &str) -> String {
    let lower = prefix.to_ascii_lowercase();
    if lower.contains("review") {
        return paint(prefix, "1;35");
    }
    if lower.contains("fix") {
        return paint(prefix, "1;36");
    }
    if lower.contains("push") {
        return paint(prefix, "1;33");
    }
    paint(prefix, "1;34")
}

fn monthly_fix_counter() -> &'static Mutex<MonthlyFixCounter> {
    static COUNTER: OnceLock<Mutex<MonthlyFixCounter>> = OnceLock::new();
    COUNTER.get_or_init(|| Mutex::new(MonthlyFixCounter::empty_for_current_month()))
}

pub fn initialize_monthly_fix_counter(state: &EngineState) {
    let month_key = current_month_key();
    let pr_numbers = state
        .monthly_fixed_pr_numbers_by_month
        .get(&month_key)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    if let Ok(mut counter) = monthly_fix_counter().lock() {
        counter.month_key = month_key;
        counter.pr_numbers = pr_numbers;
    }
}

pub fn monthly_fixed_pr_count() -> usize {
    if let Ok(mut counter) = monthly_fix_counter().lock() {
        counter.rotate_if_needed();
        return counter.pr_numbers.len();
    }
    0
}

pub fn record_monthly_fixed_pr(pr_number: u64) -> bool {
    if let Ok(mut counter) = monthly_fix_counter().lock() {
        counter.rotate_if_needed();
        return counter.pr_numbers.insert(pr_number);
    }
    false
}

pub fn sync_monthly_fix_counter_into_state(state: &mut EngineState) {
    if let Ok(mut counter) = monthly_fix_counter().lock() {
        counter.rotate_if_needed();
        let mut prs: Vec<u64> = counter.pr_numbers.iter().copied().collect();
        prs.sort_unstable();
        state
            .monthly_fixed_pr_numbers_by_month
            .insert(counter.month_key.clone(), prs);
    }
}

pub fn sh_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

pub fn run_shell(
    command: &str,
    cwd: Option<&str>,
    fail_on_non_zero: bool,
) -> std::result::Result<CommandResult, ExecError> {
    run_shell_internal(command, cwd, fail_on_non_zero, false, None, false)
}

pub fn run_shell_internal(
    command: &str,
    cwd: Option<&str>,
    fail_on_non_zero: bool,
    stream_output: bool,
    stream_prefix: Option<&str>,
    compact_stream: bool,
) -> std::result::Result<CommandResult, ExecError> {
    let mut cmd = Command::new("/bin/zsh");
    cmd.arg("-lc").arg(command);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let result = if stream_output {
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| {
            ExecError::Io(format!("failed to execute command: {command}, error: {e}"))
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            ExecError::Io(format!("failed to capture stdout for command: {command}"))
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ExecError::Io(format!("failed to capture stderr for command: {command}"))
        })?;

        let (tx, rx) = mpsc::channel::<(bool, String)>();
        let tx_stdout = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(std::result::Result::ok) {
                let _ = tx_stdout.send((true, line));
            }
        });

        let tx_stderr = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(std::result::Result::ok) {
                let _ = tx_stderr.send((false, line));
            }
        });
        drop(tx);

        let mut out_buf = String::new();
        let mut err_buf = String::new();
        let use_compact_stream = compact_stream
            && stream_prefix.is_some()
            && std::io::stdout().is_terminal()
            && std::env::var("TERM").map(|v| v != "dumb").unwrap_or(false);
        let mut compact_renderer = use_compact_stream.then(|| CompactStepRenderer::new(5));
        for (is_stdout, line) in rx {
            if is_stdout {
                out_buf.push_str(&line);
                out_buf.push('\n');
            } else {
                err_buf.push_str(&line);
                err_buf.push('\n');
            }

            if let Some(renderer) = compact_renderer.as_mut() {
                renderer.push(is_stdout, &line);
            } else if let Some(prefix) = stream_prefix {
                let styled_prefix = colorize_stream_prefix(prefix);
                if is_stdout {
                    println!("{styled_prefix}{line}");
                } else {
                    eprintln!("{}{}", styled_prefix, paint(&line, "31"));
                }
            } else if is_stdout {
                println!("{line}");
            } else {
                eprintln!("{}", paint(&line, "31"));
            }
        }
        if let Some(renderer) = compact_renderer.as_mut() {
            renderer.clear();
        }

        let status = child
            .wait()
            .map_err(|e| ExecError::Io(format!("failed waiting command: {command}, error: {e}")))?;
        CommandResult {
            exit_code: status.code().unwrap_or(-1),
            stdout: out_buf,
            stderr: err_buf,
        }
    } else {
        let output = cmd.output().map_err(|e| {
            ExecError::Io(format!("failed to execute command: {command}, error: {e}"))
        })?;

        CommandResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }
    };

    if fail_on_non_zero && result.exit_code != 0 {
        return Err(ExecError::NonZero {
            command: command.to_string(),
            result,
        });
    }

    Ok(result)
}

pub fn run_with_retry(
    command: &str,
    cwd: Option<&str>,
    retries: u8,
    retry_delay_seconds: u64,
) -> std::result::Result<CommandResult, ExecError> {
    run_with_retry_streaming(
        command,
        cwd,
        retries,
        retry_delay_seconds,
        false,
        None,
        false,
    )
}

pub fn run_with_retry_streaming(
    command: &str,
    cwd: Option<&str>,
    retries: u8,
    retry_delay_seconds: u64,
    stream_output: bool,
    stream_prefix: Option<&str>,
    compact_stream: bool,
) -> std::result::Result<CommandResult, ExecError> {
    let attempts = retries.max(1) as usize + 1;
    let mut last_err: Option<ExecError> = None;

    for attempt in 1..=attempts {
        match run_shell_internal(
            command,
            cwd,
            true,
            stream_output,
            stream_prefix,
            compact_stream,
        ) {
            Ok(result) => return Ok(result),
            Err(err) => {
                last_err = Some(err);
                if attempt < attempts {
                    std::thread::sleep(Duration::from_secs(retry_delay_seconds.max(1)));
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| ExecError::Io("unknown command failure".to_string())))
}

struct CompactStepRenderer {
    max_lines: usize,
    rendered_once: bool,
    next_row: usize,
}

impl CompactStepRenderer {
    fn new(max_lines: usize) -> Self {
        Self {
            max_lines,
            rendered_once: false,
            next_row: 0,
        }
    }

    fn push(&mut self, _is_stdout: bool, line: &str) {
        let normalized = format!("[info] {line}");

        if !self.rendered_once {
            for _ in 0..self.max_lines {
                println!();
            }
            self.rendered_once = true;
            self.next_row = 0;
        }

        let row = self.next_row;
        self.next_row = (self.next_row + 1) % self.max_lines;

        let up = self.max_lines.saturating_sub(row);
        let rendered_line = paint(&normalized, "90");
        if up > 0 {
            print!("\x1b[{}A", up);
        }
        print!("\r\x1b[2K{rendered_line}");
        if up > 0 {
            print!("\x1b[{}B", up);
        }
        print!("\r");
        let _ = std::io::stdout().flush();
    }

    fn clear(&mut self) {
        if !self.rendered_once {
            return;
        }
        let block_lines = self.max_lines;
        print!("\x1b[{}A", block_lines);
        for _ in 0..block_lines {
            print!("\r\x1b[2K\x1b[1B");
        }
        print!("\x1b[{}A", block_lines);
        let _ = std::io::stdout().flush();
        self.rendered_once = false;
    }
}

pub fn render_exec_error(err: &ExecError) -> String {
    match err {
        ExecError::Io(message) => message.clone(),
        ExecError::NonZero { command, result } => {
            let stderr = result.stderr.trim();
            if stderr.is_empty() {
                format!("{command} failed with exit {}", result.exit_code)
            } else {
                format!("{command} failed with exit {}: {stderr}", result.exit_code)
            }
        }
    }
}

pub fn is_codex_review_prompt_conflict(err: &ExecError) -> bool {
    match err {
        ExecError::NonZero { command, result } => {
            command.contains("codex review")
                && command.contains("--base")
                && result.stderr.contains("cannot be used with '[PROMPT]'")
        }
        _ => false,
    }
}

pub fn strip_co_authored_by_trailers(message: &str) -> String {
    let filtered = message
        .lines()
        .filter(|line| {
            !line
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("co-authored-by:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    filtered.trim_end().to_string() + "\n"
}

pub fn sanitize_latest_commit_message(
    repo_path: &str,
    stream_output: bool,
    stream_prefix: Option<&str>,
    compact_stream: bool,
) -> std::result::Result<(), ExecError> {
    let latest = run_shell_internal(
        "git log -1 --pretty=%B",
        Some(repo_path),
        true,
        stream_output,
        stream_prefix,
        compact_stream,
    )?;
    let cleaned = strip_co_authored_by_trailers(&latest.stdout);
    if cleaned.trim_end() == latest.stdout.trim_end() {
        return Ok(());
    }

    let temp_file = std::env::temp_dir().join(format!(
        "pr-reviewer-commit-msg-{}-{}.txt",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::write(&temp_file, cleaned).map_err(|e| {
        ExecError::Io(format!(
            "failed to write temp commit message file {}: {}",
            temp_file.display(),
            e
        ))
    })?;

    let amend = run_shell_internal(
        &format!(
            "git -c core.hooksPath=/dev/null commit --amend --no-verify -F {}",
            sh_quote(&temp_file.display().to_string())
        ),
        Some(repo_path),
        true,
        stream_output,
        stream_prefix,
        compact_stream,
    );
    let _ = fs::remove_file(&temp_file);
    amend.map(|_| ())
}

#[derive(Debug, Clone)]
struct ReviewFinding {
    issue_level: u8,
    title: String,
}

fn parse_review_findings(text: &str) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if !line.starts_with("- [") {
            continue;
        }

        let bracketed = &line[2..];
        if !bracketed.starts_with('[') {
            continue;
        }
        let Some(close_idx) = bracketed.find(']') else {
            continue;
        };

        let level_token = bracketed[1..close_idx].trim().to_ascii_uppercase();
        if level_token.len() != 2 || !level_token.starts_with('P') {
            continue;
        }
        let level_digit = level_token.as_bytes()[1];
        if !(b'0'..=b'3').contains(&level_digit) {
            continue;
        }
        let issue_level = level_digit - b'0';

        let remainder = bracketed[close_idx + 1..].trim();
        if remainder.is_empty() {
            continue;
        }
        let title_source = remainder
            .split_once('—')
            .map(|(left, _)| left)
            .unwrap_or(remainder)
            .trim();
        if title_source.is_empty() {
            continue;
        }
        let title = title_source
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim_end_matches('.')
            .to_string();
        if title.is_empty() {
            continue;
        }

        findings.push(ReviewFinding { issue_level, title });
    }

    findings
}

fn highest_issue_level_from_findings(findings: &[ReviewFinding]) -> String {
    let best = findings.iter().map(|item| item.issue_level).min();
    best.map(|value| format!("P{value}"))
        .unwrap_or_else(|| "Unknown".to_string())
}

fn infer_issue_level_from_text(text: &str) -> String {
    let findings = parse_review_findings(text);
    if !findings.is_empty() {
        return highest_issue_level_from_findings(&findings);
    }

    let mut best_p_level: Option<u8> = None;
    let mut has_critical = false;
    let mut has_high = false;
    let mut has_medium = false;
    let mut has_low = false;

    for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if lower.len() == 2
            && lower.starts_with('p')
            && let Some(digit) = lower.chars().nth(1)
            && ('0'..='3').contains(&digit)
        {
            let value = digit as u8 - b'0';
            best_p_level = Some(best_p_level.map_or(value, |current| current.min(value)));
            continue;
        }

        match lower.as_str() {
            "critical" | "blocker" | "sev1" | "severity1" => has_critical = true,
            "high" | "sev2" | "severity2" => has_high = true,
            "medium" | "med" | "sev3" | "severity3" => has_medium = true,
            "low" | "minor" | "sev4" | "severity4" => has_low = true,
            _ => {}
        }
    }

    if let Some(value) = best_p_level {
        return format!("P{value}");
    }
    if has_critical {
        return "P0".to_string();
    }
    if has_high {
        return "P1".to_string();
    }
    if has_medium {
        return "P2".to_string();
    }
    if has_low {
        return "P3".to_string();
    }
    "Unknown".to_string()
}

fn summarize_change_from_findings(findings: &[ReviewFinding]) -> Option<String> {
    if findings.is_empty() {
        return None;
    }

    let mut titles: Vec<String> = findings
        .iter()
        .take(2)
        .map(|item| item.title.clone())
        .collect();
    titles.retain(|item| !item.is_empty());
    if titles.is_empty() {
        return None;
    }

    if findings.len() == 1 {
        return Some(format!("Apply review suggestion: {}.", titles[0]));
    }

    if findings.len() == 2 {
        return Some(format!(
            "Apply review suggestions: {}; {}.",
            titles[0], titles[1]
        ));
    }

    Some(format!(
        "Apply review suggestions: {}; {} (+{} more).",
        titles[0],
        titles[1],
        findings.len() - 2
    ))
}

fn derive_commit_context_from_report(report_path: Option<&Path>) -> (String, String) {
    let default_summary = "Apply automated fixes based on review findings.".to_string();

    let Some(path) = report_path else {
        return (default_summary, "Unknown".to_string());
    };
    let content = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return (default_summary, "Unknown".to_string()),
    };

    let findings = parse_review_findings(&content);
    if !findings.is_empty() {
        let issue_level = highest_issue_level_from_findings(&findings);
        let summary =
            summarize_change_from_findings(&findings).unwrap_or_else(|| default_summary.clone());
        return (summary, issue_level);
    }

    let issue_level = infer_issue_level_from_text(&content);
    (default_summary, issue_level)
}

fn format_summary_with_level(issue_level: &str, summary: &str) -> String {
    let level_tag = if issue_level.starts_with('P') {
        issue_level.to_string()
    } else {
        "Pn".to_string()
    };
    format!("[{level_tag}] {summary}")
}

fn build_commit_message(pr_number: u64, issue_level: &str, summary: &str) -> String {
    let mut message = format!("chore: auto-fix for PR #{pr_number}\n\n");
    message.push_str(&format!(
        "Summary: {}\n",
        format_summary_with_level(issue_level, summary)
    ));
    message
}

fn extract_codex_commit_message(stdout: &str) -> Option<String> {
    let start_tag = "BEGIN_COMMIT_MESSAGE";
    let end_tag = "END_COMMIT_MESSAGE";
    let selected = if let Some(start_idx) = stdout.find(start_tag) {
        let body_start = start_idx + start_tag.len();
        let rest = &stdout[body_start..];
        if let Some(end_idx) = rest.find(end_tag) {
            &rest[..end_idx]
        } else {
            rest
        }
    } else {
        stdout
    };

    let mut message = selected
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .collect::<Vec<_>>()
        .join(" ");

    if let Some(value) = message.strip_prefix("Commit message:") {
        message = value.trim().to_string();
    }

    if message.is_empty() {
        return None;
    }

    let max_chars = 160usize;
    if message.chars().count() > max_chars {
        message = message.chars().take(max_chars).collect::<String>();
        message = message.trim_end().to_string();
    }

    Some(message)
}

fn generate_commit_message_with_codex(
    pr: &OpenPr,
    report_path: Option<&Path>,
    repo_path: &str,
) -> Option<String> {
    let report_hint = report_path
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "N/A".to_string());
    let prompt = format!(
        "You are generating a git commit message for staged changes in the current repository.\n\
PR #{pr_number}: {pr_title}\n\
Review report path: {report_hint}\n\
Read the staged diff (`git diff --cached`) and produce one short summary line in plain text.\n\
Requirements:\n\
- <= 120 characters\n\
- no markdown, no code fences, no quotes\n\
- describe what was fixed\n\
Output must use this exact wrapper:\n\
BEGIN_COMMIT_MESSAGE\n\
<your single-line message>\n\
END_COMMIT_MESSAGE",
        pr_number = pr.number,
        pr_title = pr.title
    );
    let command = format!("codex exec {}", sh_quote(&prompt));
    let result = run_shell_internal(&command, Some(repo_path), false, false, None, false).ok()?;
    if result.exit_code != 0 {
        return None;
    }
    extract_codex_commit_message(&result.stdout)
}

pub fn commit_and_push_if_needed(
    pr: &OpenPr,
    report_path: Option<&Path>,
    repo_path: &str,
    retries: u8,
    retry_delay_seconds: u64,
    stream_output: bool,
    stream_prefix: Option<&str>,
    compact_stream: bool,
) -> std::result::Result<bool, ExecError> {
    let status = run_shell_internal(
        "git status --porcelain",
        Some(repo_path),
        true,
        stream_output,
        stream_prefix,
        compact_stream,
    )?;
    if status.stdout.trim().is_empty() {
        return Ok(false);
    }

    run_shell_internal(
        "git add -A",
        Some(repo_path),
        true,
        stream_output,
        stream_prefix,
        compact_stream,
    )?;
    let fallback_message = || {
        let (summary, issue_level) = derive_commit_context_from_report(report_path);
        build_commit_message(pr.number, &issue_level, &summary)
    };
    let commit_message = generate_commit_message_with_codex(pr, report_path, repo_path)
        .unwrap_or_else(fallback_message);
    let temp_file = std::env::temp_dir().join(format!(
        "pr-reviewer-commit-msg-{}-{}.txt",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::write(&temp_file, commit_message).map_err(|e| {
        ExecError::Io(format!(
            "failed to write temp commit message file {}: {}",
            temp_file.display(),
            e
        ))
    })?;

    let commit_result = run_shell_internal(
        &format!(
            "git -c core.hooksPath=/dev/null commit --no-verify -F {}",
            sh_quote(&temp_file.display().to_string())
        ),
        Some(repo_path),
        true,
        stream_output,
        stream_prefix,
        compact_stream,
    );
    let _ = fs::remove_file(&temp_file);
    commit_result?;
    sanitize_latest_commit_message(repo_path, stream_output, stream_prefix, compact_stream)?;

    run_with_retry_streaming(
        "git push",
        Some(repo_path),
        retries,
        retry_delay_seconds,
        stream_output,
        stream_prefix,
        compact_stream,
    )?;

    Ok(true)
}

pub fn anyhow_from_exec(err: ExecError) -> anyhow::Error {
    anyhow!(render_exec_error(&err))
}

#[cfg(test)]
mod tests {
    use super::{
        build_commit_message, derive_commit_context_from_report, extract_codex_commit_message,
        format_summary_with_level, infer_issue_level_from_text, parse_review_findings,
        summarize_change_from_findings,
    };

    #[test]
    fn infer_issue_level_prefers_highest_priority_p_level() {
        let text = "Findings: [P2] null pointer risk; [P1] auth bypass";
        assert_eq!(infer_issue_level_from_text(text), "P1");
    }

    #[test]
    fn parse_review_findings_extracts_final_suggestions() {
        let text = "\
            OpenAI Codex v0.98.0 (research preview)\n\
            model: gpt-5.3-codex\n\
            - [P1] Detect POSTPAY from nested GCP payment schedule — /tmp/a.ts:1\n\
            - [P2] Clear stale metric columns before early return — /tmp/b.ts:2\n";
        let findings = parse_review_findings(text);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].issue_level, 1);
        assert_eq!(
            findings[0].title,
            "Detect POSTPAY from nested GCP payment schedule"
        );
    }

    #[test]
    fn summarize_change_from_findings_uses_findings_instead_of_headers() {
        let text = "- [P1] Detect POSTPAY from nested GCP payment schedule — /tmp/a.ts:1";
        let findings = parse_review_findings(text);
        let summary = summarize_change_from_findings(&findings);
        assert_eq!(
            summary.as_deref(),
            Some("Apply review suggestion: Detect POSTPAY from nested GCP payment schedule.")
        );
    }

    #[test]
    fn derive_context_uses_fallback_when_report_missing() {
        let (summary, level) = derive_commit_context_from_report(None);
        assert_eq!(summary, "Apply automated fixes based on review findings.");
        assert_eq!(level, "Unknown");
    }

    #[test]
    fn format_summary_with_level_prefixes_priority() {
        let summary = format_summary_with_level(
            "P1",
            "Apply review suggestion: Detect POSTPAY from nested GCP payment schedule.",
        );
        assert_eq!(
            summary,
            "[P1] Apply review suggestion: Detect POSTPAY from nested GCP payment schedule."
        );
    }

    #[test]
    fn build_commit_message_includes_only_summary_with_level_prefix() {
        let message = build_commit_message(
            42,
            "P1",
            "Apply review suggestion: Detect POSTPAY from nested GCP payment schedule.",
        );
        assert!(message.contains("PR #42"));
        assert!(message.contains("Summary: [P1] Apply review suggestion: Detect POSTPAY from nested GCP payment schedule."));
        assert!(!message.contains("Reason:"));
        assert!(!message.contains("Issue level:"));
    }

    #[test]
    fn extract_codex_commit_message_prefers_wrapped_section() {
        let output = "\
OpenAI Codex\n\
BEGIN_COMMIT_MESSAGE\n\
fix: handle nested payment schedule in POSTPAY detection\n\
END_COMMIT_MESSAGE\n\
extra";
        let message = extract_codex_commit_message(output);
        assert_eq!(
            message.as_deref(),
            Some("fix: handle nested payment schedule in POSTPAY detection")
        );
    }

    #[test]
    fn extract_codex_commit_message_handles_unwrapped_output() {
        let output = "Commit message: fix: clear stale metric columns before early return";
        let message = extract_codex_commit_message(output);
        assert_eq!(
            message.as_deref(),
            Some("fix: clear stale metric columns before early return")
        );
    }

    #[test]
    fn extract_codex_commit_message_returns_none_for_empty_output() {
        let output = "```";
        let message = extract_codex_commit_message(output);
        assert!(message.is_none());
    }
}
