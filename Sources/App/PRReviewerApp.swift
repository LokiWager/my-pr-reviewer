import SwiftUI

@main
struct PRReviewerApp: App {
    @StateObject private var viewModel = AppViewModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(viewModel)
        }
    }
}

struct ContentView: View {
    @EnvironmentObject private var viewModel: AppViewModel

    var body: some View {
        TabView {
            DashboardView()
                .tabItem {
                    Label("Dashboard", systemImage: "gauge")
                }

            SettingsView()
                .tabItem {
                    Label("Settings", systemImage: "gearshape")
                }
        }
        .frame(minWidth: 960, minHeight: 620)
        .onAppear {
            viewModel.refresh()
        }
    }
}
