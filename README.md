# PRReviewer (macOS App + Widget)

Runs on a daily schedule: pulls the latest code from a target repository, detects newly added PRs (compared with the previous run), calls Codex review for new PRs, then auto-fixes and pushes changes. Progress and reports are shown in both the macOS app and widget.

## Features

- Scheduled daily background execution (`launchd`)
- New PR detection (using local state file `processedPRNumbers`)
- Configurable command templates for `review` / `fix`
- Configurable concurrency (multiple PRs in parallel) and command retry handling
- Automatic `commit` + `push` (can be disabled)
- App dashboard for progress, logs, and reports
- Widget status and result display

## Architecture

- `PRReviewerCore`: execution engine, state storage, command runner, `launchd` installer
- `PRReviewerAgent`: CLI entry point for background tasks
- `PRReviewerApp`: SwiftUI desktop UI (Dashboard + Settings)
- `PRReviewerWidget`: WidgetKit progress display

## Prerequisites

- macOS 14+
- Xcode 15+
- [XcodeGen](https://github.com/yonaskolb/XcodeGen)
- `git`, `gh`, `codex` (all must be available in your shell)
- Authenticated GitHub CLI (`gh auth login`)

## Quick Start

1. Generate the Xcode project.

```bash
xcodegen generate
```

2. Open the project.

```bash
open PRReviewer.xcodeproj
```

3. On first app launch, fill in Settings:
- `Repo Path`: repository path to review automatically
- `Default Branch`: e.g. `main`
- `Max concurrent PR workers`: number of parallel workers (start with `1` or `2`)
- `Command retries` / `Retry delay seconds`: retry policy for review/fix/push failures
- `Review Command Template` / `Fix Command Template`
- `Agent Executable Path`: absolute path to the `PRReviewerAgent` executable

4. In the app, click `Save`, then `Install Daily Task`.

## Command Template Placeholders

- `{{PR_NUMBER}}`
- `{{PR_TITLE}}`
- `{{PR_URL}}`
- `{{PR_BRANCH}}`
- `{{DEFAULT_BRANCH}}`
- `{{REPO_PATH}}`
- `{{WORK_DIR}}`
- `{{REPORT_PATH}}`

Example:

```bash
codex review --pr {{PR_NUMBER}} --repo {{REPO_PATH}} > {{REPORT_PATH}}
codex fix --pr {{PR_NUMBER}} --repo {{REPO_PATH}}
```

Notes:

- In concurrent mode, each PR runs in its own temporary directory to avoid workspace conflicts.
- `{{REPO_PATH}}` and `{{WORK_DIR}}` both point to the current PR's temporary working directory.

## Manual Scheduling Script

You can install the task without using the app by running:

```bash
./Scripts/install_launch_agent.sh \
  /ABS/PATH/TO/PRReviewerAgent \
  /ABS/PATH/TO/TARGET_REPO \
  9 0
```

## Data Paths

The app group container is preferred. Fallback paths:

- `~/.pr-reviewer/settings.json`
- `~/.pr-reviewer/engine-state.json`
- `~/.pr-reviewer/run-snapshot.json`
- `~/.pr-reviewer/reports/*.md`
- `~/.pr-reviewer/logs/*.log`

## Risk Notice

Automatic fixing and pushing is a high-risk operation. Validate command templates and permissions in a test repository before using it in production repositories.
