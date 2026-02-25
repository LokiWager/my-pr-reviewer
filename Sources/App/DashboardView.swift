import SwiftUI
import PRReviewerCore

struct DashboardView: View {
    @EnvironmentObject private var viewModel: AppViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            header
            statusCard
            prListCard
            reportTable
            logs
        }
        .padding(20)
    }

    private var header: some View {
        HStack {
            Text("PR Auto Review")
                .font(.largeTitle)
                .fontWeight(.semibold)

            Spacer()

            Button("Run Now") {
                viewModel.runNow()
            }
            .buttonStyle(.borderedProminent)
            .disabled(viewModel.isRunningWorkflow)

            Button("Install Daily Task") {
                viewModel.installLaunchAgent()
            }
            .buttonStyle(.bordered)
        }
    }

    private var statusCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Status")
                .font(.headline)

            HStack {
                Text("Run Status: \(viewModel.snapshot.status.rawValue)")
                Spacer()
                Text("Stage: \(viewModel.snapshot.stage.displayName)")
            }

            HStack {
                Text("Progress: \(viewModel.snapshot.currentIndex)/\(viewModel.snapshot.totalPRs)")
                Spacer()
                if let number = viewModel.snapshot.currentPRNumber {
                    Text("Current PR: #\(number)")
                }
            }

            if let title = viewModel.snapshot.currentPRTitle {
                Text("Title: \(title)")
                    .foregroundStyle(.secondary)
            }

            if let error = viewModel.snapshot.errorMessage, !error.isEmpty {
                Text(error)
                    .foregroundStyle(.red)
                    .font(.callout)
            }

            if !viewModel.installMessage.isEmpty {
                Text(viewModel.installMessage)
                    .foregroundStyle(.secondary)
                    .font(.callout)
            }
        }
        .padding(14)
        .background(Color.gray.opacity(0.12))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private var prListCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Open PRs")
                    .font(.headline)

                Spacer()

                Button(viewModel.isLoadingPRs ? "Loading..." : "Load PRs") {
                    viewModel.loadOpenPRs()
                }
                .disabled(viewModel.isLoadingPRs || viewModel.isRunningWorkflow)

                Button("Run Selected PR") {
                    viewModel.runSelectedPR()
                }
                .buttonStyle(.borderedProminent)
                .disabled(viewModel.selectedPRNumber == nil || viewModel.isRunningWorkflow)
            }

            if viewModel.openPRs.isEmpty {
                Text("No PR list loaded")
                    .foregroundStyle(.secondary)
            } else {
                List(selection: $viewModel.selectedPRNumber) {
                    ForEach(viewModel.openPRs) { pr in
                        VStack(alignment: .leading, spacing: 3) {
                            Text("#\(pr.number) \(pr.title)")
                                .fontWeight(.medium)
                            HStack(spacing: 8) {
                                Text(pr.headRefName)
                                Text(pr.updatedAt)
                                Text(pr.url)
                            }
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        }
                        .tag(pr.number)
                    }
                }
                .frame(minHeight: 180)
            }
        }
        .padding(14)
        .background(Color.gray.opacity(0.12))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }

    private var reportTable: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Report")
                .font(.headline)

            if viewModel.snapshot.report.isEmpty {
                Text("No report yet")
                    .foregroundStyle(.secondary)
            } else {
                List(viewModel.snapshot.report) { item in
                    HStack {
                        VStack(alignment: .leading) {
                            Text("#\(item.number) \(item.title)")
                                .fontWeight(.medium)
                            Text(item.url)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text("review \(item.reviewExitCode)")
                            .font(.caption)
                        Text("fix \(item.fixExitCode)")
                            .font(.caption)
                        Text(item.pushed ? "pushed" : "no-push")
                            .font(.caption)
                            .foregroundStyle(item.pushed ? .green : .secondary)
                        Button("Open") {
                            viewModel.openReport(path: item.reportPath)
                        }
                    }
                }
                .frame(minHeight: 220)
            }
        }
    }

    private var logs: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Logs")
                .font(.headline)

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(viewModel.snapshot.logLines.suffix(80).enumerated()), id: \.offset) { _, line in
                        Text(line)
                            .font(.system(size: 11, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            .frame(minHeight: 150)
            .padding(8)
            .background(Color.black.opacity(0.04))
            .clipShape(RoundedRectangle(cornerRadius: 8))
        }
    }
}
