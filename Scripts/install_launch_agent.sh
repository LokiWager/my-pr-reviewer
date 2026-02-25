#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 4 ]]; then
  echo "Usage: $0 <agent_executable_path> <repo_path> <hour> <minute>"
  exit 1
fi

AGENT_PATH="$1"
REPO_PATH="$2"
HOUR="$3"
MINUTE="$4"
LABEL="com.codex.prreviewer.agent"
PLIST="$HOME/Library/LaunchAgents/${LABEL}.plist"
LOG_ROOT="$HOME/.pr-reviewer/logs"

mkdir -p "$HOME/Library/LaunchAgents" "$LOG_ROOT"

cat > "$PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${AGENT_PATH}</string>
    <string>--run-once</string>
  </array>
  <key>StartCalendarInterval</key>
  <dict>
    <key>Hour</key>
    <integer>${HOUR}</integer>
    <key>Minute</key>
    <integer>${MINUTE}</integer>
  </dict>
  <key>WorkingDirectory</key>
  <string>${REPO_PATH}</string>
  <key>StandardOutPath</key>
  <string>${LOG_ROOT}/agent.stdout.log</string>
  <key>StandardErrorPath</key>
  <string>${LOG_ROOT}/agent.stderr.log</string>
  <key>RunAtLoad</key>
  <false/>
</dict>
</plist>
PLIST

launchctl unload "$PLIST" 2>/dev/null || true
launchctl load "$PLIST"
echo "Installed ${PLIST}"
