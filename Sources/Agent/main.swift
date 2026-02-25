import Foundation
import PRReviewerCore

let arguments = CommandLine.arguments
let settings = SharedStore.loadSettings()

if arguments.contains("--install-launch-agent") {
    do {
        let plistURL = try LaunchAgentInstaller.install(with: settings)
        print("LaunchAgent installed: \(plistURL.path)")
        exit(EXIT_SUCCESS)
    } catch {
        fputs("Install LaunchAgent failed: \(error.localizedDescription)\n", stderr)
        exit(EXIT_FAILURE)
    }
}

let snapshot = WorkflowRunner().runOnce()
let exitCode: Int32 = snapshot.status == .succeeded ? EXIT_SUCCESS : EXIT_FAILURE

if snapshot.status == .failed {
    fputs("Run failed: \(snapshot.errorMessage ?? "Unknown error")\n", stderr)
} else {
    print("Run completed. Processed \(snapshot.totalPRs) PR(s).")
}

exit(exitCode)
