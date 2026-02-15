import SwiftUI

struct SettingsView: View {
    @ObservedObject var chatViewModel: ChatViewModel

    var body: some View {
        TabView {
            ModelSettingsView(
                viewModel: chatViewModel.settingsViewModel,
                bridge: chatViewModel.bridge
            )
            .tabItem {
                Label("模型", systemImage: "cpu")
            }

            PermissionSettingsView(bridge: chatViewModel.bridge)
                .tabItem {
                    Label("权限", systemImage: "hand.raised")
                }

            AboutSettingsView(bridge: chatViewModel.bridge)
                .tabItem {
                    Label("关于", systemImage: "info.circle")
                }
        }
        .padding(16)
    }
}
