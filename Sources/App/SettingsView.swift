import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var viewModel: AppViewModel

    var body: some View {
        Form {
            Section("Repository") {
                TextField("Repo Path", text: $viewModel.settings.repoPath)
                TextField("Default Branch", text: $viewModel.settings.defaultBranch)
                Toggle("Auto push after fix", isOn: $viewModel.settings.autoPushEnabled)
                Stepper("Max PRs per run: \(viewModel.settings.maxPRsPerRun)", value: $viewModel.settings.maxPRsPerRun, in: 1...200)
            }

            Section("Execution") {
                Stepper("Max concurrent PR workers: \(viewModel.settings.maxConcurrentPRs)", value: $viewModel.settings.maxConcurrentPRs, in: 1...10)
                Stepper("Command retries: \(viewModel.settings.maxCommandRetries)", value: $viewModel.settings.maxCommandRetries, in: 0...10)
                Stepper("Retry delay seconds: \(viewModel.settings.retryDelaySeconds)", value: $viewModel.settings.retryDelaySeconds, in: 1...120)
            }

            Section("Schedule") {
                Stepper("Hour: \(viewModel.settings.scheduleHour)", value: $viewModel.settings.scheduleHour, in: 0...23)
                Stepper("Minute: \(viewModel.settings.scheduleMinute)", value: $viewModel.settings.scheduleMinute, in: 0...59)
                TextField("Agent Executable Path", text: $viewModel.settings.agentExecutablePath)
            }

            Section("Commands") {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Review Command Template")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    TextEditor(text: $viewModel.settings.reviewCommandTemplate)
                        .font(.system(size: 12, design: .monospaced))
                        .frame(minHeight: 80)
                }

                VStack(alignment: .leading, spacing: 6) {
                    Text("Fix Command Template")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    TextEditor(text: $viewModel.settings.fixCommandTemplate)
                        .font(.system(size: 12, design: .monospaced))
                        .frame(minHeight: 80)
                }

                Text("Available placeholders: {{PR_NUMBER}}, {{PR_TITLE}}, {{PR_URL}}, {{PR_BRANCH}}, {{DEFAULT_BRANCH}}, {{REPO_PATH}}, {{WORK_DIR}}, {{REPORT_PATH}}")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            HStack {
                Button("Save") {
                    viewModel.saveSettings()
                }
                .buttonStyle(.borderedProminent)

                Button("Reload") {
                    viewModel.refresh()
                }
                .buttonStyle(.bordered)

                Spacer()
            }
        }
        .formStyle(.grouped)
        .padding(12)
    }
}
