import Foundation
import Combine
import AppKit
import PRReviewerCore
import WidgetKit

@MainActor
final class AppViewModel: ObservableObject {
    @Published var settings: AppSettings = .default
    @Published var snapshot: RunSnapshot = RunSnapshot()
    @Published var installMessage: String = ""
    @Published var openPRs: [OpenPR] = []
    @Published var selectedPRNumber: Int?
    @Published var isLoadingPRs: Bool = false
    @Published var isRunningWorkflow: Bool = false

    private var timer: AnyCancellable?

    init() {
        refresh()
        timer = Timer.publish(every: 2, on: .main, in: .common)
            .autoconnect()
            .sink { [weak self] _ in
                self?.reloadSnapshotOnly()
            }
    }

    func refresh() {
        settings = SharedStore.loadSettings()
        snapshot = SharedStore.loadSnapshot()
    }

    func reloadSnapshotOnly() {
        snapshot = SharedStore.loadSnapshot()
    }

    func saveSettings() {
        do {
            try SharedStore.saveSettings(settings)
            installMessage = "Settings saved"
        } catch {
            installMessage = "Save failed: \(error.localizedDescription)"
        }
    }

    func runNow() {
        guard !isRunningWorkflow else { return }
        isRunningWorkflow = true
        DispatchQueue.global(qos: .userInitiated).async {
            _ = WorkflowRunner().runOnce()
            DispatchQueue.main.async {
                self.snapshot = SharedStore.loadSnapshot()
                WidgetCenter.shared.reloadAllTimelines()
                self.isRunningWorkflow = false
            }
        }
    }

    func loadOpenPRs() {
        guard !isLoadingPRs else { return }
        isLoadingPRs = true

        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let prs = try WorkflowRunner().fetchOpenPRs()
                DispatchQueue.main.async {
                    self.openPRs = prs
                    if let selected = self.selectedPRNumber, !prs.contains(where: { $0.number == selected }) {
                        self.selectedPRNumber = nil
                    }
                    self.installMessage = "Loaded \(prs.count) open PR(s)"
                    self.isLoadingPRs = false
                }
            } catch {
                DispatchQueue.main.async {
                    self.installMessage = "Load PRs failed: \(error.localizedDescription)"
                    self.isLoadingPRs = false
                }
            }
        }
    }

    func runSelectedPR() {
        guard !isRunningWorkflow else { return }
        guard let prNumber = selectedPRNumber else {
            installMessage = "Please select a PR first"
            return
        }

        isRunningWorkflow = true
        DispatchQueue.global(qos: .userInitiated).async {
            _ = WorkflowRunner().runSelectedPR(number: prNumber)
            DispatchQueue.main.async {
                self.snapshot = SharedStore.loadSnapshot()
                WidgetCenter.shared.reloadAllTimelines()
                self.isRunningWorkflow = false
            }
        }
    }

    func installLaunchAgent() {
        do {
            let url = try LaunchAgentInstaller.install(with: settings)
            installMessage = "LaunchAgent installed at: \(url.path)"
        } catch {
            installMessage = "Install failed: \(error.localizedDescription)"
        }
    }

    func openReport(path: String) {
        let url = URL(fileURLWithPath: path)
        NSWorkspace.shared.open(url)
    }
}
