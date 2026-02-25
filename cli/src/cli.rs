use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use rustyline::Editor;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use std::fs;

use crate::models::OpenPr;
use crate::store::{StorePaths, load_settings, save_json};
use crate::workflow::{
    print_pr_list, print_report, print_status, run_single_pr_by_number, run_workflow,
};

#[derive(Parser, Debug)]
#[command(name = "pr-reviewer-cli")]
#[command(about = "Interactive CLI for GitHub PR auto review/fix workflow")]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start interactive shell (default)
    Shell,
    /// Execute a single workflow run and print progress
    Run,
    /// List open PRs that can be reviewed
    Prs,
    /// Run review/fix for a specific PR number
    RunPr {
        #[arg(long)]
        pr: u64,
    },
    /// Show latest report summary and file
    Report,
    /// Show latest run status
    Status,
    /// Initialize default settings file if missing
    Init,
}

fn print_help() {
    println!("available commands:");
    println!("  run       - execute workflow once and stream logs");
    println!("  prs       - list all open PRs (with new/processed marker)");
    println!("  pick N    - run review/fix for PR index from last `prs` list");
    println!("  run-pr X  - run review/fix for PR number X");
    println!("  status    - show latest run status");
    println!("  report    - show latest run report and markdown");
    println!("  settings  - print settings file path and content");
    println!("  help      - show this help");
    println!("  quit/exit - leave shell");
}

fn run_shell_mode(paths: &StorePaths) -> Result<()> {
    println!("PR Reviewer CLI Shell");
    println!("workspace: {}", paths.root.display());
    print_help();

    let prompt = format!("{}> ", paths.root.display());
    let history_path = paths.root.join("history.txt");
    let mut rl = Editor::<(), DefaultHistory>::new()
        .map_err(|e| anyhow!("failed to initialize line editor: {e}"))?;
    let _ = rl.load_history(&history_path);
    let mut last_pr_list: Vec<OpenPr> = Vec::new();

    loop {
        let input = match rl.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(err) => return Err(anyhow!("shell input failed: {err}")),
        };

        let command = input.trim();
        if command.is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(command);

        let parts: Vec<&str> = command.split_whitespace().collect();
        match parts[0] {
            "run" if parts.len() == 1 => match run_workflow(paths, true) {
                Ok(snapshot) => {
                    println!(
                        "final status={:?}, progress={}/{}, error={}",
                        snapshot.status,
                        snapshot.current_index,
                        snapshot.total_prs,
                        snapshot.error_message.unwrap_or_else(|| "-".to_string())
                    );
                }
                Err(err) => {
                    println!("run failed: {err}");
                }
            },
            "prs" if parts.len() == 1 => match print_pr_list(paths, true) {
                Ok(prs) => last_pr_list = prs,
                Err(err) => println!("prs failed: {err}"),
            },
            "pick" if parts.len() == 2 => {
                let index = match parts[1].parse::<usize>() {
                    Ok(v) if v > 0 => v,
                    _ => {
                        println!("invalid index: {}", parts[1]);
                        continue;
                    }
                };
                if index > last_pr_list.len() {
                    println!(
                        "index out of range, run `prs` first and choose 1..{}",
                        last_pr_list.len()
                    );
                    continue;
                }
                let pr_number = last_pr_list[index - 1].number;
                match run_single_pr_by_number(paths, pr_number, true) {
                    Ok(snapshot) => {
                        println!(
                            "selected PR done: status={:?}, pr=#{} error={}",
                            snapshot.status,
                            pr_number,
                            snapshot.error_message.unwrap_or_else(|| "-".to_string())
                        );
                    }
                    Err(err) => {
                        println!("run-pr failed for #{}: {}", pr_number, err);
                    }
                }
            }
            "run-pr" if parts.len() == 2 => {
                let pr_number = match parts[1].parse::<u64>() {
                    Ok(v) => v,
                    Err(_) => {
                        println!("invalid pr number: {}", parts[1]);
                        continue;
                    }
                };
                match run_single_pr_by_number(paths, pr_number, true) {
                    Ok(snapshot) => {
                        println!(
                            "selected PR done: status={:?}, pr=#{} error={}",
                            snapshot.status,
                            pr_number,
                            snapshot.error_message.unwrap_or_else(|| "-".to_string())
                        );
                    }
                    Err(err) => {
                        println!("run-pr failed for #{}: {}", pr_number, err);
                    }
                }
            }
            "status" => {
                if let Err(err) = print_status(paths) {
                    println!("status failed: {err}");
                }
            }
            "report" => {
                if let Err(err) = print_report(paths) {
                    println!("report failed: {err}");
                }
            }
            "settings" => {
                println!("settings file: {}", paths.settings.display());
                match fs::read_to_string(&paths.settings) {
                    Ok(content) => println!("{content}"),
                    Err(err) => println!("read settings failed: {err}"),
                }
            }
            "help" if parts.len() == 1 => print_help(),
            "quit" | "exit" if parts.len() == 1 => break,
            _ => {
                println!("unknown command: {command}");
                println!("type 'help' for available commands");
            }
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

pub fn run_app() -> Result<()> {
    let cli = Cli::parse();
    let paths = StorePaths::new()?;

    match cli.command.unwrap_or(Commands::Shell) {
        Commands::Shell => run_shell_mode(&paths),
        Commands::Run => {
            let snapshot = run_workflow(&paths, true)?;
            println!(
                "final status={:?}, total_prs={}, done={}, error={}",
                snapshot.status,
                snapshot.total_prs,
                snapshot.current_index,
                snapshot.error_message.unwrap_or_else(|| "-".to_string())
            );
            Ok(())
        }
        Commands::Prs => {
            let _ = print_pr_list(&paths, true)?;
            Ok(())
        }
        Commands::RunPr { pr } => {
            let snapshot = run_single_pr_by_number(&paths, pr, true)?;
            println!(
                "selected PR done: status={:?}, pr=#{} error={}",
                snapshot.status,
                pr,
                snapshot.error_message.unwrap_or_else(|| "-".to_string())
            );
            Ok(())
        }
        Commands::Report => print_report(&paths),
        Commands::Status => print_status(&paths),
        Commands::Init => {
            let settings = load_settings(&paths)?;
            save_json(&paths.settings, &settings)?;
            println!("settings initialized: {}", paths.settings.display());
            Ok(())
        }
    }
}
