# pr-reviewer-cli

Rust interactive CLI for automated PR review/fix workflow.

## Build

```bash
cd /Users/tingyi-suger/Documents/workspaces/my-pr-reviewer/cli
cargo build
```

## First time setup

```bash
cargo run -- init
```

Then edit:

- `/Users/tingyi-suger/.pr-reviewer-cli/settings.json`

Required fields:
- `repo_path`
- `repo_clone_url` (用于首次自动 clone)
- `default_branch`
- `review_command_template`
- `fix_command_template`

If `repo_path` does not exist or is empty, CLI will auto clone from `repo_clone_url`.

## Enter CLI shell

```bash
cargo run
```

You will enter an interactive prompt like:

```text
/Users/tingyi-suger/.pr-reviewer-cli>
```

Available shell commands:
- `run`: start workflow and print execution logs
- `prs`: list open PRs (`new` / `processed`) and author name/login; PRs where current `gh` user already appears in `participants` are hidden
- `pick N`: choose PR by index from latest `prs` output and run review+fix+push
- `run-pr X`: run review+fix+push for PR number `X`
- `status`: show latest run status
- `report`: show latest report summary and latest markdown report content
- `settings`: print settings file content
- `help`
- `quit` / `exit`

Shell supports command history with arrow keys (`↑` / `↓`). History is saved at `~/.pr-reviewer-cli/history.txt`.
Before each run, the tool force-rolls back local uncommitted changes in `repo_path` (`git reset --hard HEAD` and `git clean -fd`) to avoid branch-switch conflicts.
During `run` / `run-pr` / `pick`, `codex review` and `codex exec` logs are streamed live to the console with `[review]` and `[fix]` prefixes.

## Non-interactive commands

```bash
cargo run -- run
cargo run -- prs
cargo run -- run-pr --pr 123
cargo run -- status
cargo run -- report
```

## Commit identity

When pushing fixes, commits use your local Git identity from the target repository/environment (`git config user.name` / `git config user.email`). The CLI does not set a Codex author.
The CLI also strips any `Co-Authored-By:` trailers before push.

## Data path

Default root:
- `~/.pr-reviewer-cli/`

Files:
- `settings.json`
- `engine-state.json`
- `run-snapshot.json`
- `reports/*.md`
- `logs/`

You can override with env var:

```bash
PR_REVIEWER_HOME=/custom/path cargo run
```

## Template placeholders

- `{{PR_NUMBER}}`
- `{{PR_TITLE}}`
- `{{PR_URL}}`
- `{{PR_BRANCH}}`
- `{{DEFAULT_BRANCH}}`
- `{{REPO_PATH}}`
- `{{WORK_DIR}}`
- `{{REPORT_PATH}}`

## settings.json example

```json
{
  "repo_path": "/Users/tingyi-suger/Documents/workspaces/target-repo",
  "repo_clone_url": "git@github.com:your-org/your-repo.git",
  "default_branch": "main",
  "max_prs_per_run": 20,
  "max_command_retries": 2,
  "retry_delay_seconds": 15,
  "review_command_template": "codex review --base {{DEFAULT_BRANCH}}",
  "fix_command_template": "codex exec \"You are in a checked-out PR branch. Read findings and fix issues for PR #{{PR_NUMBER}} ({{PR_TITLE}}). Use report context at {{REPORT_PATH}} when relevant. Make minimal safe changes and update tests if needed.\"",
  "auto_push_enabled": true
}
```
