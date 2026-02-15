import SwiftUI

struct ModelSettingsView: View {
    @ObservedObject var viewModel: SettingsViewModel
    let bridge: CoreBridge?

    var body: some View {
        Form {
            SecureField("OpenRouter API Key", text: $viewModel.apiKey)

            Picker("免费模型默认选项", selection: $viewModel.modelName) {
                ForEach(viewModel.freeModelOptions, id: \.self) { model in
                    Text(model).tag(model)
                }
            }
            .pickerStyle(.menu)

            TextField("或手动输入模型名", text: $viewModel.modelName)
            TextField("Base URL（可选）", text: $viewModel.baseURL)

            HStack {
                Button("保存并应用") { viewModel.saveAndApply(to: bridge) }
                Button("测试连接") { viewModel.testConnection(bridge: bridge) }
                    .disabled(viewModel.isTesting || viewModel.apiKey.isEmpty)
            }

            if !viewModel.connectionMessage.isEmpty {
                Text(viewModel.connectionMessage)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}
