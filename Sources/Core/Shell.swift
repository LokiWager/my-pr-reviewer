import Foundation

public struct CommandResult {
    public let exitCode: Int32
    public let stdout: String
    public let stderr: String
}

public enum CommandError: Error {
    case nonZeroExit(command: String, result: CommandResult)
}

public enum Shell {
    @discardableResult
    public static func run(
        _ command: String,
        currentDirectory: String? = nil,
        failOnNonZeroExit: Bool = false
    ) throws -> CommandResult {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-lc", command]
        if let currentDirectory, !currentDirectory.isEmpty {
            process.currentDirectoryURL = URL(fileURLWithPath: currentDirectory)
        }

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        try process.run()
        process.waitUntilExit()

        let stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
        let stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()

        let result = CommandResult(
            exitCode: process.terminationStatus,
            stdout: String(data: stdoutData, encoding: .utf8) ?? "",
            stderr: String(data: stderrData, encoding: .utf8) ?? ""
        )

        if failOnNonZeroExit, result.exitCode != 0 {
            throw CommandError.nonZeroExit(command: command, result: result)
        }

        return result
    }

    public static func quote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }
}
