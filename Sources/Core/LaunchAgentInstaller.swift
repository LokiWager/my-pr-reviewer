import Foundation

public enum LaunchAgentInstaller {
    public static let label = "com.codex.prreviewer.agent"

    @discardableResult
    public static func install(with settings: AppSettings) throws -> URL {
        guard !settings.agentExecutablePath.isEmpty else {
            throw NSError(domain: "PRReviewer", code: 1, userInfo: [NSLocalizedDescriptionKey: "Agent executable path is empty"])
        }
        guard !settings.repoPath.isEmpty else {
            throw NSError(domain: "PRReviewer", code: 6, userInfo: [NSLocalizedDescriptionKey: "repoPath is empty"])
        }
        guard (0...23).contains(settings.scheduleHour), (0...59).contains(settings.scheduleMinute) else {
            throw NSError(domain: "PRReviewer", code: 7, userInfo: [NSLocalizedDescriptionKey: "Invalid schedule hour/minute"])
        }

        let launchAgentsDirectory = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents", isDirectory: true)
        try FileManager.default.createDirectory(at: launchAgentsDirectory, withIntermediateDirectories: true)

        let plistURL = launchAgentsDirectory.appendingPathComponent("\(label).plist", isDirectory: false)
        let stdoutLog = SharedPaths.logsDirectory().appendingPathComponent("agent.stdout.log", isDirectory: false).path
        let stderrLog = SharedPaths.logsDirectory().appendingPathComponent("agent.stderr.log", isDirectory: false).path

        let plist: [String: Any] = [
            "Label": label,
            "ProgramArguments": [settings.agentExecutablePath, "--run-once"],
            "StartCalendarInterval": [
                "Hour": settings.scheduleHour,
                "Minute": settings.scheduleMinute
            ],
            "WorkingDirectory": settings.repoPath,
            "StandardOutPath": stdoutLog,
            "StandardErrorPath": stderrLog,
            "RunAtLoad": false,
            "KeepAlive": false
        ]

        let plistData = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
        try plistData.write(to: plistURL, options: .atomic)

        _ = try? Shell.run("launchctl unload \(Shell.quote(plistURL.path))")
        _ = try Shell.run("launchctl load \(Shell.quote(plistURL.path))", failOnNonZeroExit: true)
        return plistURL
    }
}
