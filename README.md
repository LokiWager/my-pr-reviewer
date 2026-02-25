# PRReviewer (macOS App + Widget)

每天定时执行：拉取指定仓库最新代码，检测是否有新增 PR（相对上次执行），对新增 PR 调用 Codex review，然后自动修复并推送；同时在 macOS App 与 Widget 展示进度和报告。

## 功能

- 每日固定时间后台执行（`launchd`）
- 检测新增 PR（使用本地状态文件 `processedPRNumbers`）
- 调用可配置命令模板执行 `review` / `fix`
- 可配置并发（多 PR 并行处理）与命令失败重试
- 自动 `commit` + `push`（可关闭）
- App 显示进度、日志、报告
- Widget 展示当前进度和结果

## 技术结构

- `PRReviewerCore`：核心执行引擎、状态存储、命令执行器、launchd 安装器
- `PRReviewerAgent`：CLI 入口（后台任务执行）
- `PRReviewerApp`：SwiftUI 桌面界面（Dashboard + Settings）
- `PRReviewerWidget`：WidgetKit 进度展示

## 前置依赖

- macOS 14+
- Xcode 15+
- [XcodeGen](https://github.com/yonaskolb/XcodeGen)
- `git`, `gh`, `codex`（都需可在 shell 中执行）
- `gh auth login` 完成授权

## 快速开始

1. 生成 Xcode 项目

```bash
xcodegen generate
```

2. 打开项目

```bash
open PRReviewer.xcodeproj
```

3. 首次运行 App，填写 Settings：
- `Repo Path`：要自动审查的仓库路径
- `Default Branch`：例如 `main`
- `Max concurrent PR workers`：并行 worker 数（建议先从 `1` 或 `2` 开始）
- `Command retries` / `Retry delay seconds`：review/fix/push 失败后的重试策略
- `Review Command Template` / `Fix Command Template`
- `Agent Executable Path`：`PRReviewerAgent` 可执行文件绝对路径

4. 在 App 里点击 `Save`，然后 `Install Daily Task`。

## 命令模板占位符

- `{{PR_NUMBER}}`
- `{{PR_TITLE}}`
- `{{PR_URL}}`
- `{{PR_BRANCH}}`
- `{{DEFAULT_BRANCH}}`
- `{{REPO_PATH}}`
- `{{WORK_DIR}}`
- `{{REPORT_PATH}}`

示例：

```bash
codex review --pr {{PR_NUMBER}} --repo {{REPO_PATH}} > {{REPORT_PATH}}
codex fix --pr {{PR_NUMBER}} --repo {{REPO_PATH}}
```

说明：

- 并发模式下，每个 PR 在独立临时目录执行，不会互相污染工作区。
- `{{REPO_PATH}}` 和 `{{WORK_DIR}}` 都指向当前 PR 的临时工作目录。

## 手动调度脚本

也可以不用 App 安装任务，直接执行：

```bash
./Scripts/install_launch_agent.sh \
  /ABS/PATH/TO/PRReviewerAgent \
  /ABS/PATH/TO/TARGET_REPO \
  9 0
```

## 数据路径

优先 App Group 容器，fallback 为：

- `~/.pr-reviewer/settings.json`
- `~/.pr-reviewer/engine-state.json`
- `~/.pr-reviewer/run-snapshot.json`
- `~/.pr-reviewer/reports/*.md`
- `~/.pr-reviewer/logs/*.log`

## 风险提示

自动修复并推送是高风险操作，建议先在测试仓库验证命令模板和权限，再用于生产仓库。
