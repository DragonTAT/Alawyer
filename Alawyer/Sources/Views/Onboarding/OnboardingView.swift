import SwiftUI

struct OnboardingView: View {
    @ObservedObject var settingsViewModel: SettingsViewModel
    let bridge: CoreBridge?
    let onContinue: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("👋 欢迎使用 Alawyer")
                .font(.title2.bold())

            Text("Alawyer 是您的 AI 法律咨询助手，帮您整理案情、检索法规、生成报告。")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            Divider()

            Text("开始前需要配置 AI 模型：")
                .font(.subheadline)

            SecureField("OpenRouter API Key", text: $settingsViewModel.apiKey)
                .textFieldStyle(.roundedBorder)
            Picker("选择模型", selection: $settingsViewModel.modelName) {
                ForEach(settingsViewModel.freeModelOptions, id: \.self) { model in
                    Text(model).tag(model)
                }
            }
            .pickerStyle(.menu)

            TextField("或手动输入模型名", text: $settingsViewModel.modelName)
                .textFieldStyle(.roundedBorder)

            HStack {
                Button("测试连接") {
                    settingsViewModel.testConnection(bridge: bridge)
                }
                .disabled(settingsViewModel.apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || settingsViewModel.isTesting)

                Button("开始使用") {
                    onContinue()
                }
                .disabled(settingsViewModel.apiKey.trimmingCharacters(in: .whitespaces).isEmpty)
                .buttonStyle(.borderedProminent)
            }

            if !settingsViewModel.connectionMessage.isEmpty {
                Text(settingsViewModel.connectionMessage)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            HStack {
                Image(systemName: "lock.shield")
                    .foregroundStyle(.green)
                Text("所有数据仅存储在本地，不会上传任何服务器")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(24)
        .frame(width: 500, height: 420)
    }
}
