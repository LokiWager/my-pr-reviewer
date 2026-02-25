import Foundation

public enum SharedPaths {
    public static let appGroupID = "group.com.codex.prreviewer"

    public static func rootDirectory() -> URL {
        if let groupURL = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroupID) {
            let appRoot = groupURL.appendingPathComponent("PRReviewer", isDirectory: true)
            try? FileManager.default.createDirectory(at: appRoot, withIntermediateDirectories: true)
            return appRoot
        }

        let fallback = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".pr-reviewer", isDirectory: true)
        try? FileManager.default.createDirectory(at: fallback, withIntermediateDirectories: true)
        return fallback
    }

    public static func settingsFile() -> URL {
        rootDirectory().appendingPathComponent("settings.json", isDirectory: false)
    }

    public static func stateFile() -> URL {
        rootDirectory().appendingPathComponent("engine-state.json", isDirectory: false)
    }

    public static func snapshotFile() -> URL {
        rootDirectory().appendingPathComponent("run-snapshot.json", isDirectory: false)
    }

    public static func reportDirectory() -> URL {
        let directory = rootDirectory().appendingPathComponent("reports", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }

    public static func logsDirectory() -> URL {
        let directory = rootDirectory().appendingPathComponent("logs", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }
}

public enum SharedStore {
    private static let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        return encoder
    }()

    private static let decoder: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()

    public static func loadSettings() -> AppSettings {
        (try? load(AppSettings.self, from: SharedPaths.settingsFile())) ?? .default
    }

    public static func saveSettings(_ settings: AppSettings) throws {
        try save(settings, to: SharedPaths.settingsFile())
    }

    public static func loadState() -> EngineState {
        (try? load(EngineState.self, from: SharedPaths.stateFile())) ?? EngineState()
    }

    public static func saveState(_ state: EngineState) throws {
        try save(state, to: SharedPaths.stateFile())
    }

    public static func loadSnapshot() -> RunSnapshot {
        (try? load(RunSnapshot.self, from: SharedPaths.snapshotFile())) ?? RunSnapshot()
    }

    public static func saveSnapshot(_ snapshot: RunSnapshot) throws {
        try save(snapshot, to: SharedPaths.snapshotFile())
    }

    public static func saveReportMarkdown(_ markdown: String, fileName: String) throws -> URL {
        let fileURL = SharedPaths.reportDirectory().appendingPathComponent(fileName, isDirectory: false)
        try markdown.write(to: fileURL, atomically: true, encoding: .utf8)
        return fileURL
    }

    private static func load<T: Decodable>(_ type: T.Type, from fileURL: URL) throws -> T {
        let data = try Data(contentsOf: fileURL)
        return try decoder.decode(type, from: data)
    }

    private static func save<T: Encodable>(_ value: T, to fileURL: URL) throws {
        let data = try encoder.encode(value)
        try data.write(to: fileURL, options: .atomic)
    }
}
