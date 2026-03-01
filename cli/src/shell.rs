use anyhow::anyhow;
use chrono::{Local, Utc};
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::collections::HashSet;
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
    run_shell_internal(command, cwd, fail_on_non_zero, false, None)
}

pub fn run_shell_internal(
    command: &str,
    cwd: Option<&str>,
    fail_on_non_zero: bool,
    stream_output: bool,
    stream_prefix: Option<&str>,
) -> std::result::Result<CommandResult, ExecError> {
    println!(
        "Calendar-month fixed PR count: {}",
        monthly_fixed_pr_count()
    );

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
        for (is_stdout, line) in rx {
            if is_stdout {
                out_buf.push_str(&line);
                out_buf.push('\n');
            } else {
                err_buf.push_str(&line);
                err_buf.push('\n');
            }

            if let Some(prefix) = stream_prefix {
                if is_stdout {
                    println!("{prefix}{line}");
                } else {
                    eprintln!("{prefix}{line}");
                }
            } else if is_stdout {
                println!("{line}");
            } else {
                eprintln!("{line}");
            }
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
    run_with_retry_streaming(command, cwd, retries, retry_delay_seconds, false, None)
}

pub fn run_with_retry_streaming(
    command: &str,
    cwd: Option<&str>,
    retries: u8,
    retry_delay_seconds: u64,
    stream_output: bool,
    stream_prefix: Option<&str>,
) -> std::result::Result<CommandResult, ExecError> {
    let attempts = retries.max(1) as usize + 1;
    let mut last_err: Option<ExecError> = None;

    for attempt in 1..=attempts {
        match run_shell_internal(command, cwd, true, stream_output, stream_prefix) {
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

pub fn sanitize_latest_commit_message(repo_path: &str) -> std::result::Result<(), ExecError> {
    let latest = run_shell("git log -1 --pretty=%B", Some(repo_path), true)?;
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

    let amend = run_shell(
        &format!(
            "git -c core.hooksPath=/dev/null commit --amend --no-verify -F {}",
            sh_quote(&temp_file.display().to_string())
        ),
        Some(repo_path),
        true,
    );
    let _ = fs::remove_file(&temp_file);
    amend.map(|_| ())
}

pub fn commit_and_push_if_needed(
    pr: &OpenPr,
    repo_path: &str,
    retries: u8,
    retry_delay_seconds: u64,
) -> std::result::Result<bool, ExecError> {
    let status = run_shell("git status --porcelain", Some(repo_path), true)?;
    if status.stdout.trim().is_empty() {
        return Ok(false);
    }

    run_shell("git add -A", Some(repo_path), true)?;
    run_shell(
        &format!(
            "git -c core.hooksPath=/dev/null commit --no-verify -m {}",
            sh_quote(&format!("chore: auto-fix for PR #{}", pr.number))
        ),
        Some(repo_path),
        true,
    )?;
    sanitize_latest_commit_message(repo_path)?;

    run_with_retry("git push", Some(repo_path), retries, retry_delay_seconds)?;

    Ok(true)
}

pub fn anyhow_from_exec(err: ExecError) -> anyhow::Error {
    anyhow!(render_exec_error(&err))
}
