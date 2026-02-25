import Foundation

private struct PRProcessOutcome {
    let result: PRExecutionResult
    let markAsProcessed: Bool
    let logLines: [String]
}

public final class WorkflowRunner {
    private let isoFormatter = ISO8601DateFormatter()

    public init() {}

    @discardableResult
    public func runOnce() -> RunSnapshot {
        let lock = NSLock()
        var snapshot = RunSnapshot(
            startedAt: Date(),
            status: .running,
            stage: .syncingRepo,
            logLines: []
        )
        persist(snapshot)

        do {
            let settings = SharedStore.loadSettings()
            guard !settings.repoPath.isEmpty else {
                throw NSError(domain: "PRReviewer", code: 2, userInfo: [NSLocalizedDescriptionKey: "repoPath is empty in settings"])
            }

            appendLog("Start run", to: &snapshot)
            persist(snapshot)

            try validateEnvironment(repoPath: settings.repoPath)
            try syncRepository(settings: settings, snapshot: &snapshot)

            snapshot.stage = .loadingPRs
            persist(snapshot)

            let openPRs = try listOpenPRs(repoPath: settings.repoPath)
            var state = SharedStore.loadState()
            var processedSet = Set(state.processedPRNumbers)

            var newPRs = openPRs.filter { !processedSet.contains($0.number) }
            if settings.maxPRsPerRun > 0 {
                newPRs = Array(newPRs.prefix(settings.maxPRsPerRun))
            }

            snapshot.totalPRs = newPRs.count
            snapshot.currentIndex = 0
            snapshot.report = []
            snapshot.errorMessage = nil
            persist(snapshot)

            if newPRs.isEmpty {
                appendLog("No new PRs found", to: &snapshot)
                snapshot.stage = .completed
                snapshot.status = .succeeded
                snapshot.finishedAt = Date()
                state.lastRunAt = Date()
                try SharedStore.saveState(state)
                persist(snapshot)
                return snapshot
            }

            let originURL = try remoteOriginURL(repoPath: settings.repoPath)
            let runID = isoFormatter.string(from: Date()).replacingOccurrences(of: ":", with: "-")
            let runRoot = FileManager.default.temporaryDirectory
                .appendingPathComponent("prreviewer-runs", isDirectory: true)
                .appendingPathComponent(runID, isDirectory: true)
            try FileManager.default.createDirectory(at: runRoot, withIntermediateDirectories: true)
            defer {
                try? FileManager.default.removeItem(at: runRoot)
            }

            let queue = OperationQueue()
            queue.name = "PRReviewer.WorkQueue"
            queue.maxConcurrentOperationCount = max(1, settings.maxConcurrentPRs)
            queue.qualityOfService = .userInitiated

            var results: [PRExecutionResult] = []
            var completedCount = 0
            var failedCount = 0
            var newlyProcessed = Set<Int>()

            snapshot.stage = .reviewingPR
            appendLog("Found \(newPRs.count) new PR(s), max concurrency: \(queue.maxConcurrentOperationCount)", to: &snapshot)
            persist(snapshot)

            for pr in newPRs {
                queue.addOperation {
                    lock.lock()
                    snapshot.currentPRNumber = pr.number
                    snapshot.currentPRTitle = pr.title
                    self.appendLog("Start PR #\(pr.number)", to: &snapshot)
                    self.persist(snapshot)
                    lock.unlock()

                    let outcome = self.processPR(
                        pr,
                        settings: settings,
                        originURL: originURL,
                        runRoot: runRoot,
                        runID: runID
                    )

                    lock.lock()
                    completedCount += 1
                    if outcome.markAsProcessed {
                        newlyProcessed.insert(pr.number)
                    }
                    if outcome.result.errorMessage != nil {
                        failedCount += 1
                    }
                    results.append(outcome.result)
                    results.sort { $0.number < $1.number }

                    snapshot.currentIndex = completedCount
                    snapshot.currentPRNumber = pr.number
                    snapshot.currentPRTitle = pr.title
                    snapshot.report = results
                    snapshot.logLines.append(contentsOf: outcome.logLines)
                    self.trimLogLines(&snapshot)
                    self.persist(snapshot)
                    lock.unlock()
                }
            }

            queue.waitUntilAllOperationsAreFinished()

            processedSet.formUnion(newlyProcessed)
            state.processedPRNumbers = Array(processedSet).sorted()
            state.lastRunAt = Date()
            try SharedStore.saveState(state)

            snapshot.report = results.sorted { $0.number < $1.number }
            snapshot.currentIndex = snapshot.totalPRs
            snapshot.finishedAt = Date()
            if failedCount > 0 {
                snapshot.stage = .failed
                snapshot.status = .failed
                snapshot.errorMessage = "\(failedCount) PR(s) failed. Check reports and logs."
                appendLog("Run completed with \(failedCount) failure(s)", to: &snapshot)
            } else {
                snapshot.stage = .completed
                snapshot.status = .succeeded
                appendLog("Run completed successfully", to: &snapshot)
            }
            persist(snapshot)
            return snapshot
        } catch {
            snapshot.stage = .failed
            snapshot.status = .failed
            snapshot.errorMessage = error.localizedDescription
            snapshot.finishedAt = Date()
            appendLog("Run failed: \(error.localizedDescription)", to: &snapshot)
            persist(snapshot)
            return snapshot
        }
    }

    public func fetchOpenPRs() throws -> [OpenPR] {
        let settings = SharedStore.loadSettings()
        guard !settings.repoPath.isEmpty else {
            throw NSError(domain: "PRReviewer", code: 2, userInfo: [NSLocalizedDescriptionKey: "repoPath is empty in settings"])
        }

        try validateEnvironment(repoPath: settings.repoPath)
        try syncRepository(repoPath: settings.repoPath, defaultBranch: settings.defaultBranch)
        return try listOpenPRs(repoPath: settings.repoPath)
    }

    @discardableResult
    public func runSelectedPR(number: Int) -> RunSnapshot {
        var snapshot = RunSnapshot(
            startedAt: Date(),
            status: .running,
            stage: .syncingRepo,
            logLines: []
        )
        persist(snapshot)

        do {
            let settings = SharedStore.loadSettings()
            guard !settings.repoPath.isEmpty else {
                throw NSError(domain: "PRReviewer", code: 2, userInfo: [NSLocalizedDescriptionKey: "repoPath is empty in settings"])
            }

            appendLog("Start selected PR run for #\(number)", to: &snapshot)
            persist(snapshot)

            try validateEnvironment(repoPath: settings.repoPath)
            try syncRepository(settings: settings, snapshot: &snapshot)

            snapshot.stage = .loadingPRs
            persist(snapshot)

            let openPRs = try listOpenPRs(repoPath: settings.repoPath)
            guard let selectedPR = openPRs.first(where: { $0.number == number }) else {
                throw NSError(domain: "PRReviewer", code: 10, userInfo: [NSLocalizedDescriptionKey: "PR #\(number) not found in open PR list"])
            }

            snapshot.totalPRs = 1
            snapshot.currentIndex = 0
            snapshot.currentPRNumber = selectedPR.number
            snapshot.currentPRTitle = selectedPR.title
            snapshot.report = []
            snapshot.errorMessage = nil
            appendLog("Selected PR #\(selectedPR.number): \(selectedPR.title)", to: &snapshot)
            persist(snapshot)

            let originURL = try remoteOriginURL(repoPath: settings.repoPath)
            let runID = isoFormatter.string(from: Date()).replacingOccurrences(of: ":", with: "-")
            let runRoot = FileManager.default.temporaryDirectory
                .appendingPathComponent("prreviewer-runs", isDirectory: true)
                .appendingPathComponent(runID, isDirectory: true)
            try FileManager.default.createDirectory(at: runRoot, withIntermediateDirectories: true)
            defer {
                try? FileManager.default.removeItem(at: runRoot)
            }

            snapshot.stage = .reviewingPR
            persist(snapshot)

            let outcome = processPR(
                selectedPR,
                settings: settings,
                originURL: originURL,
                runRoot: runRoot,
                runID: runID
            )

            snapshot.currentIndex = 1
            snapshot.currentPRNumber = selectedPR.number
            snapshot.currentPRTitle = selectedPR.title
            snapshot.report = [outcome.result]
            snapshot.logLines.append(contentsOf: outcome.logLines)
            trimLogLines(&snapshot)

            var state = SharedStore.loadState()
            state.lastRunAt = Date()
            if outcome.markAsProcessed {
                var processedSet = Set(state.processedPRNumbers)
                processedSet.insert(selectedPR.number)
                state.processedPRNumbers = Array(processedSet).sorted()
            }
            try SharedStore.saveState(state)

            snapshot.finishedAt = Date()
            if let error = outcome.result.errorMessage, !error.isEmpty {
                snapshot.stage = .failed
                snapshot.status = .failed
                snapshot.errorMessage = error
                appendLog("Selected PR #\(selectedPR.number) failed", to: &snapshot)
            } else {
                snapshot.stage = .completed
                snapshot.status = .succeeded
                appendLog("Selected PR #\(selectedPR.number) completed", to: &snapshot)
            }

            persist(snapshot)
            return snapshot
        } catch {
            snapshot.stage = .failed
            snapshot.status = .failed
            snapshot.errorMessage = error.localizedDescription
            snapshot.finishedAt = Date()
            appendLog("Selected PR run failed: \(error.localizedDescription)", to: &snapshot)
            persist(snapshot)
            return snapshot
        }
    }

    private func processPR(
        _ pr: OpenPR,
        settings: AppSettings,
        originURL: String,
        runRoot: URL,
        runID: String
    ) -> PRProcessOutcome {
        var logs: [String] = []

        func log(_ message: String) {
            let timestamp = isoFormatter.string(from: Date())
            logs.append("[\(timestamp)] [PR #\(pr.number)] \(message)")
        }

        var reviewExitCode = -1
        var fixExitCode = -1
        var pushed = false

        let reportFileName = "pr-\(pr.number)-\(runID).md"
        let reportURL = SharedPaths.reportDirectory().appendingPathComponent(reportFileName, isDirectory: false)

        let workDir = runRoot.appendingPathComponent("pr-\(pr.number)", isDirectory: true)
        defer {
            try? FileManager.default.removeItem(at: workDir)
        }

        do {
            log("Clone repository")
            _ = try runCommandWithRetry(
                "git clone --quiet --no-tags \(Shell.quote(originURL)) \(Shell.quote(workDir.path))",
                currentDirectory: nil,
                retries: settings.maxCommandRetries,
                retryDelaySeconds: settings.retryDelaySeconds,
                stepName: "clone"
            )

            log("Checkout PR")
            _ = try runCommandWithRetry(
                "gh pr checkout \(pr.number)",
                currentDirectory: workDir.path,
                retries: settings.maxCommandRetries,
                retryDelaySeconds: settings.retryDelaySeconds,
                stepName: "checkout"
            )

            let reviewCommand = expand(
                template: settings.reviewCommandTemplate,
                pr: pr,
                repoPath: workDir.path,
                defaultBranch: settings.defaultBranch,
                reportPath: reportURL.path
            )

            log("Run review command")
            let reviewResult = try runCommandWithRetry(
                reviewCommand,
                currentDirectory: workDir.path,
                retries: settings.maxCommandRetries,
                retryDelaySeconds: settings.retryDelaySeconds,
                stepName: "review"
            )
            reviewExitCode = Int(reviewResult.exitCode)
            try writeReviewArtifact(pr: pr, command: reviewCommand, result: reviewResult, reportURL: reportURL)

            let fixCommand = expand(
                template: settings.fixCommandTemplate,
                pr: pr,
                repoPath: workDir.path,
                defaultBranch: settings.defaultBranch,
                reportPath: reportURL.path
            )

            log("Run fix command")
            let fixResult = try runCommandWithRetry(
                fixCommand,
                currentDirectory: workDir.path,
                retries: settings.maxCommandRetries,
                retryDelaySeconds: settings.retryDelaySeconds,
                stepName: "fix"
            )
            fixExitCode = Int(fixResult.exitCode)

            if settings.autoPushEnabled {
                log("Commit and push")
                pushed = try commitAndPushIfNeeded(
                    pr: pr,
                    repoPath: workDir.path,
                    retries: settings.maxCommandRetries,
                    retryDelaySeconds: settings.retryDelaySeconds
                )
            }

            log("PR completed")
            return PRProcessOutcome(
                result: PRExecutionResult(
                    number: pr.number,
                    title: pr.title,
                    url: pr.url,
                    reviewExitCode: reviewExitCode,
                    fixExitCode: fixExitCode,
                    pushed: pushed,
                    reportPath: reportURL.path,
                    errorMessage: nil
                ),
                markAsProcessed: true,
                logLines: logs
            )
        } catch {
            let (failedCommand, commandResult) = commandFailureDetail(error)
            let message = errorMessage(error)
            log("Failed: \(message)")

            if let commandResult {
                try? writeReviewArtifact(
                    pr: pr,
                    command: failedCommand ?? "",
                    result: commandResult,
                    reportURL: reportURL
                )
            } else if !FileManager.default.fileExists(atPath: reportURL.path) {
                let markdown = """
                # PR #\(pr.number) Review Report

                - Title: \(pr.title)
                - URL: \(pr.url)
                - Generated At: \(isoFormatter.string(from: Date()))
                - Exit Code: -1

                ## error

                \(message)
                """
                try? markdown.write(to: reportURL, atomically: true, encoding: .utf8)
            }

            return PRProcessOutcome(
                result: PRExecutionResult(
                    number: pr.number,
                    title: pr.title,
                    url: pr.url,
                    reviewExitCode: reviewExitCode,
                    fixExitCode: fixExitCode,
                    pushed: pushed,
                    reportPath: reportURL.path,
                    errorMessage: message
                ),
                markAsProcessed: false,
                logLines: logs
            )
        }
    }

    private func runCommandWithRetry(
        _ command: String,
        currentDirectory: String?,
        retries: Int,
        retryDelaySeconds: Int,
        stepName: String
    ) throws -> CommandResult {
        let attempts = max(1, retries + 1)
        var lastError: Error?

        for attempt in 1...attempts {
            do {
                return try Shell.run(command, currentDirectory: currentDirectory, failOnNonZeroExit: true)
            } catch {
                lastError = error
                if attempt < attempts {
                    let delay = max(1, retryDelaySeconds)
                    Thread.sleep(forTimeInterval: TimeInterval(delay))
                }
            }
        }

        if let lastError {
            throw lastError
        }
        throw NSError(
            domain: "PRReviewer",
            code: 8,
            userInfo: [NSLocalizedDescriptionKey: "Command failed after retries for step: \(stepName)"]
        )
    }

    private func validateEnvironment(repoPath: String) throws {
        let gitCheck = try Shell.run("git rev-parse --is-inside-work-tree", currentDirectory: repoPath)
        guard gitCheck.exitCode == 0 else {
            throw NSError(domain: "PRReviewer", code: 3, userInfo: [NSLocalizedDescriptionKey: "repoPath is not a git repository"])
        }

        let ghCheck = try Shell.run("command -v gh")
        guard ghCheck.exitCode == 0 else {
            throw NSError(domain: "PRReviewer", code: 4, userInfo: [NSLocalizedDescriptionKey: "gh CLI not found"])
        }

        let codexCheck = try Shell.run("command -v codex")
        guard codexCheck.exitCode == 0 else {
            throw NSError(domain: "PRReviewer", code: 5, userInfo: [NSLocalizedDescriptionKey: "codex CLI not found"])
        }
    }

    private func syncRepository(settings: AppSettings, snapshot: inout RunSnapshot) throws {
        appendLog("Sync repository", to: &snapshot)
        persist(snapshot)
        try syncRepository(repoPath: settings.repoPath, defaultBranch: settings.defaultBranch)
    }

    private func syncRepository(repoPath: String, defaultBranch: String) throws {
        _ = try Shell.run("git fetch --all --prune", currentDirectory: repoPath, failOnNonZeroExit: true)
        _ = try Shell.run("git checkout \(Shell.quote(defaultBranch))", currentDirectory: repoPath, failOnNonZeroExit: true)
        _ = try Shell.run("git pull --ff-only origin \(Shell.quote(defaultBranch))", currentDirectory: repoPath, failOnNonZeroExit: true)
    }

    private func remoteOriginURL(repoPath: String) throws -> String {
        let result = try Shell.run(
            "git config --get remote.origin.url",
            currentDirectory: repoPath,
            failOnNonZeroExit: true
        )
        let originURL = result.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        if originURL.isEmpty {
            throw NSError(domain: "PRReviewer", code: 9, userInfo: [NSLocalizedDescriptionKey: "remote.origin.url is empty"])
        }
        return originURL
    }

    private func listOpenPRs(repoPath: String) throws -> [OpenPR] {
        let command = "gh pr list --state open --limit 200 --json number,title,headRefName,url,updatedAt"
        let result = try Shell.run(command, currentDirectory: repoPath, failOnNonZeroExit: true)
        let data = Data(result.stdout.utf8)
        let prs = try JSONDecoder().decode([OpenPR].self, from: data)
        return prs.sorted { $0.updatedAt > $1.updatedAt }
    }

    private func commitAndPushIfNeeded(
        pr: OpenPR,
        repoPath: String,
        retries: Int,
        retryDelaySeconds: Int
    ) throws -> Bool {
        let status = try Shell.run("git status --porcelain", currentDirectory: repoPath, failOnNonZeroExit: true)
        if status.stdout.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return false
        }

        _ = try Shell.run("git add -A", currentDirectory: repoPath, failOnNonZeroExit: true)
        _ = try Shell.run(
            "git commit -m \(Shell.quote("chore: codex auto-fix for PR #\(pr.number)"))",
            currentDirectory: repoPath,
            failOnNonZeroExit: true
        )
        _ = try runCommandWithRetry(
            "git push",
            currentDirectory: repoPath,
            retries: retries,
            retryDelaySeconds: retryDelaySeconds,
            stepName: "push"
        )
        return true
    }

    private func expand(
        template: String,
        pr: OpenPR,
        repoPath: String,
        defaultBranch: String,
        reportPath: String
    ) -> String {
        var command = template
        command = command.replacingOccurrences(of: "{{PR_NUMBER}}", with: String(pr.number))
        command = command.replacingOccurrences(of: "{{PR_TITLE}}", with: Shell.quote(pr.title))
        command = command.replacingOccurrences(of: "{{PR_URL}}", with: Shell.quote(pr.url))
        command = command.replacingOccurrences(of: "{{PR_BRANCH}}", with: Shell.quote(pr.headRefName))
        command = command.replacingOccurrences(of: "{{DEFAULT_BRANCH}}", with: Shell.quote(defaultBranch))
        command = command.replacingOccurrences(of: "{{REPO_PATH}}", with: Shell.quote(repoPath))
        command = command.replacingOccurrences(of: "{{WORK_DIR}}", with: Shell.quote(repoPath))
        command = command.replacingOccurrences(of: "{{REPORT_PATH}}", with: Shell.quote(reportPath))
        return command
    }

    private func commandFailureDetail(_ error: Error) -> (String?, CommandResult?) {
        if let commandError = error as? CommandError {
            switch commandError {
            case .nonZeroExit(let command, let result):
                return (command, result)
            }
        }
        return (nil, nil)
    }

    private func errorMessage(_ error: Error?) -> String {
        guard let error else {
            return "Unknown error"
        }

        if let commandError = error as? CommandError {
            switch commandError {
            case .nonZeroExit(let command, let result):
                let stderr = result.stderr.trimmingCharacters(in: .whitespacesAndNewlines)
                if stderr.isEmpty {
                    return "Command failed: \(command) (exit \(result.exitCode))"
                }
                return "Command failed: \(command) (exit \(result.exitCode)) stderr: \(stderr)"
            }
        }

        return error.localizedDescription
    }

    private func writeReviewArtifact(pr: OpenPR, command: String, result: CommandResult, reportURL: URL) throws {
        if FileManager.default.fileExists(atPath: reportURL.path) {
            let existing = try? String(contentsOf: reportURL, encoding: .utf8)
            if let existing, !existing.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                return
            }
        }

        let markdown = """
        # PR #\(pr.number) Review Report

        - Title: \(pr.title)
        - URL: \(pr.url)
        - Generated At: \(isoFormatter.string(from: Date()))
        - Review Command: `\(command)`
        - Exit Code: \(result.exitCode)

        ## stdout

        ```
        \(result.stdout)
        ```

        ## stderr

        ```
        \(result.stderr)
        ```
        """

        try markdown.write(to: reportURL, atomically: true, encoding: .utf8)
    }

    private func appendLog(_ message: String, to snapshot: inout RunSnapshot) {
        let timestamp = isoFormatter.string(from: Date())
        snapshot.logLines.append("[\(timestamp)] \(message)")
        trimLogLines(&snapshot)
    }

    private func trimLogLines(_ snapshot: inout RunSnapshot) {
        if snapshot.logLines.count > 500 {
            snapshot.logLines = Array(snapshot.logLines.suffix(500))
        }
    }

    private func persist(_ snapshot: RunSnapshot) {
        try? SharedStore.saveSnapshot(snapshot)
    }
}
