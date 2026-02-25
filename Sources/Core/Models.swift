import Foundation

public enum RunStatus: String, Codable {
    case idle
    case running
    case succeeded
    case failed
}

public enum ExecutionStage: String, Codable {
    case idle
    case syncingRepo
    case loadingPRs
    case reviewingPR
    case fixingPR
    case pushingChanges
    case completed
    case failed

    public var displayName: String {
        switch self {
        case .idle: return "Idle"
        case .syncingRepo: return "Syncing repository"
        case .loadingPRs: return "Loading PR list"
        case .reviewingPR: return "Reviewing PR"
        case .fixingPR: return "Auto fixing"
        case .pushingChanges: return "Pushing changes"
        case .completed: return "Completed"
        case .failed: return "Failed"
        }
    }
}

public struct AppSettings: Codable {
    public var repoPath: String
    public var defaultBranch: String
    public var scheduleHour: Int
    public var scheduleMinute: Int
    public var maxPRsPerRun: Int
    public var maxConcurrentPRs: Int
    public var maxCommandRetries: Int
    public var retryDelaySeconds: Int
    public var reviewCommandTemplate: String
    public var fixCommandTemplate: String
    public var agentExecutablePath: String
    public var autoPushEnabled: Bool

    public init(
        repoPath: String,
        defaultBranch: String,
        scheduleHour: Int,
        scheduleMinute: Int,
        maxPRsPerRun: Int,
        maxConcurrentPRs: Int,
        maxCommandRetries: Int,
        retryDelaySeconds: Int,
        reviewCommandTemplate: String,
        fixCommandTemplate: String,
        agentExecutablePath: String,
        autoPushEnabled: Bool
    ) {
        self.repoPath = repoPath
        self.defaultBranch = defaultBranch
        self.scheduleHour = scheduleHour
        self.scheduleMinute = scheduleMinute
        self.maxPRsPerRun = maxPRsPerRun
        self.maxConcurrentPRs = maxConcurrentPRs
        self.maxCommandRetries = maxCommandRetries
        self.retryDelaySeconds = retryDelaySeconds
        self.reviewCommandTemplate = reviewCommandTemplate
        self.fixCommandTemplate = fixCommandTemplate
        self.agentExecutablePath = agentExecutablePath
        self.autoPushEnabled = autoPushEnabled
    }

    public static var `default`: AppSettings {
        AppSettings(
            repoPath: "",
            defaultBranch: "main",
            scheduleHour: 9,
            scheduleMinute: 0,
            maxPRsPerRun: 20,
            maxConcurrentPRs: 2,
            maxCommandRetries: 2,
            retryDelaySeconds: 15,
            reviewCommandTemplate: "codex review --pr {{PR_NUMBER}} --repo {{REPO_PATH}} > {{REPORT_PATH}}",
            fixCommandTemplate: "codex fix --pr {{PR_NUMBER}} --repo {{REPO_PATH}}",
            agentExecutablePath: "",
            autoPushEnabled: true
        )
    }

    private enum CodingKeys: String, CodingKey {
        case repoPath
        case defaultBranch
        case scheduleHour
        case scheduleMinute
        case maxPRsPerRun
        case maxConcurrentPRs
        case maxCommandRetries
        case retryDelaySeconds
        case reviewCommandTemplate
        case fixCommandTemplate
        case agentExecutablePath
        case autoPushEnabled
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.repoPath = try container.decodeIfPresent(String.self, forKey: .repoPath) ?? AppSettings.default.repoPath
        self.defaultBranch = try container.decodeIfPresent(String.self, forKey: .defaultBranch) ?? AppSettings.default.defaultBranch
        self.scheduleHour = try container.decodeIfPresent(Int.self, forKey: .scheduleHour) ?? AppSettings.default.scheduleHour
        self.scheduleMinute = try container.decodeIfPresent(Int.self, forKey: .scheduleMinute) ?? AppSettings.default.scheduleMinute
        self.maxPRsPerRun = try container.decodeIfPresent(Int.self, forKey: .maxPRsPerRun) ?? AppSettings.default.maxPRsPerRun
        self.maxConcurrentPRs = try container.decodeIfPresent(Int.self, forKey: .maxConcurrentPRs) ?? AppSettings.default.maxConcurrentPRs
        self.maxCommandRetries = try container.decodeIfPresent(Int.self, forKey: .maxCommandRetries) ?? AppSettings.default.maxCommandRetries
        self.retryDelaySeconds = try container.decodeIfPresent(Int.self, forKey: .retryDelaySeconds) ?? AppSettings.default.retryDelaySeconds
        self.reviewCommandTemplate = try container.decodeIfPresent(String.self, forKey: .reviewCommandTemplate) ?? AppSettings.default.reviewCommandTemplate
        self.fixCommandTemplate = try container.decodeIfPresent(String.self, forKey: .fixCommandTemplate) ?? AppSettings.default.fixCommandTemplate
        self.agentExecutablePath = try container.decodeIfPresent(String.self, forKey: .agentExecutablePath) ?? AppSettings.default.agentExecutablePath
        self.autoPushEnabled = try container.decodeIfPresent(Bool.self, forKey: .autoPushEnabled) ?? AppSettings.default.autoPushEnabled
    }
}

public struct EngineState: Codable {
    public var processedPRNumbers: [Int]
    public var lastRunAt: Date?

    public init(processedPRNumbers: [Int] = [], lastRunAt: Date? = nil) {
        self.processedPRNumbers = processedPRNumbers
        self.lastRunAt = lastRunAt
    }
}

public struct OpenPR: Codable, Identifiable {
    public var id: Int { number }
    public let number: Int
    public let title: String
    public let headRefName: String
    public let url: String
    public let updatedAt: String
}

public struct PRExecutionResult: Codable, Identifiable {
    public var id: Int { number }
    public let number: Int
    public let title: String
    public let url: String
    public let reviewExitCode: Int
    public let fixExitCode: Int
    public let pushed: Bool
    public let reportPath: String
    public let errorMessage: String?
}

public struct RunSnapshot: Codable {
    public var startedAt: Date?
    public var finishedAt: Date?
    public var status: RunStatus
    public var stage: ExecutionStage
    public var totalPRs: Int
    public var currentIndex: Int
    public var currentPRNumber: Int?
    public var currentPRTitle: String?
    public var errorMessage: String?
    public var report: [PRExecutionResult]
    public var logLines: [String]

    public init(
        startedAt: Date? = nil,
        finishedAt: Date? = nil,
        status: RunStatus = .idle,
        stage: ExecutionStage = .idle,
        totalPRs: Int = 0,
        currentIndex: Int = 0,
        currentPRNumber: Int? = nil,
        currentPRTitle: String? = nil,
        errorMessage: String? = nil,
        report: [PRExecutionResult] = [],
        logLines: [String] = []
    ) {
        self.startedAt = startedAt
        self.finishedAt = finishedAt
        self.status = status
        self.stage = stage
        self.totalPRs = totalPRs
        self.currentIndex = currentIndex
        self.currentPRNumber = currentPRNumber
        self.currentPRTitle = currentPRTitle
        self.errorMessage = errorMessage
        self.report = report
        self.logLines = logLines
    }
}
