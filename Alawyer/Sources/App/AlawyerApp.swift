import SwiftUI

@main
struct AlawyerApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    @StateObject private var chatVM = ChatViewModel()

    var body: some Scene {
        WindowGroup {
            if chatVM.hasCompletedOnboarding {
                NavigationSplitView {
                    SessionListView(viewModel: chatVM.sessionViewModel, chatViewModel: chatVM)
                        .frame(minWidth: 260)
                } detail: {
                    TabView {
                        ChatView(viewModel: chatVM)
                            .tabItem {
                                Label("咨询", systemImage: "message")
                            }
                        SettingsView(chatViewModel: chatVM)
                            .tabItem {
                                Label("设置", systemImage: "gearshape")
                            }
                    }
                }
            } else {
                OnboardingView(
                    settingsViewModel: chatVM.settingsViewModel,
                    bridge: chatVM.bridge,
                    onContinue: { chatVM.completeOnboarding() }
                )
            }
        }
        .windowStyle(.automatic)
    }
}
