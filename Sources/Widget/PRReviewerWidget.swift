import WidgetKit
import SwiftUI
import PRReviewerCore

struct PRWidgetEntry: TimelineEntry {
    let date: Date
    let snapshot: RunSnapshot
}

struct PRWidgetProvider: TimelineProvider {
    func placeholder(in context: Context) -> PRWidgetEntry {
        PRWidgetEntry(date: Date(), snapshot: RunSnapshot(status: .running, stage: .reviewingPR, totalPRs: 5, currentIndex: 2, currentPRNumber: 123))
    }

    func getSnapshot(in context: Context, completion: @escaping (PRWidgetEntry) -> Void) {
        completion(PRWidgetEntry(date: Date(), snapshot: SharedStore.loadSnapshot()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<PRWidgetEntry>) -> Void) {
        let entry = PRWidgetEntry(date: Date(), snapshot: SharedStore.loadSnapshot())
        let next = Calendar.current.date(byAdding: .minute, value: 1, to: Date()) ?? Date().addingTimeInterval(60)
        completion(Timeline(entries: [entry], policy: .after(next)))
    }
}

struct PRReviewerWidgetEntryView: View {
    var entry: PRWidgetProvider.Entry

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("PR Review Bot")
                .font(.headline)

            Text(entry.snapshot.stage.displayName)
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Text("\(entry.snapshot.currentIndex)/\(entry.snapshot.totalPRs)")
                .font(.system(size: 24, weight: .bold, design: .rounded))

            if let pr = entry.snapshot.currentPRNumber {
                Text("Current: #\(pr)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Text("Status: \(entry.snapshot.status.rawValue)")
                .font(.caption)
                .foregroundStyle(entry.snapshot.status == .failed ? .red : .secondary)
        }
        .padding(12)
    }
}

struct PRReviewerWidget: Widget {
    let kind: String = "PRReviewerWidget"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: kind, provider: PRWidgetProvider()) { entry in
            PRReviewerWidgetEntryView(entry: entry)
        }
        .configurationDisplayName("PR Review Progress")
        .description("Show current PR review/fix progress and result.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}

@main
struct PRReviewerWidgetBundle: WidgetBundle {
    var body: some Widget {
        PRReviewerWidget()
    }
}
