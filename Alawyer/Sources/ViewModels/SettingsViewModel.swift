import Foundation

@MainActor
final class SettingsViewModel: ObservableObject {
    @Published var apiKey: String = ""
    @Published var modelName: String = "openrouter/free"
    @Published var baseURL: String = ""
    @Published var connectionMessage: String = ""
    @Published var isTesting: Bool = false

    /// OpenRouter currently available free model defaults (as of 2026-02-15).
    let freeModelOptions: [String] = [
        "openrouter/free",
        "google/gemma-3-27b-it:free",
        "google/gemma-3-12b-it:free",
        "stepfun/step-3.5-flash:free",
        "arcee-ai/trinity-large-preview:free",
        "upstage/solar-pro-3:free",
    ]

    /// Save to Keychain + apply to Rust Core in one step
    @discardableResult
    func saveAndApply(to bridge: CoreBridge?) -> Bool {
        guard let bridge else {
            connectionMessage = "Core 未初始化"
            return false
        }

        let trimmedKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedModel = modelName.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedBaseUrl = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)

        if trimmedKey.isEmpty {
            connectionMessage = "请先填写 OpenRouter API Key"
            return false
        }

        apiKey = trimmedKey
        modelName = trimmedModel.isEmpty ? "openrouter/free" : trimmedModel
        baseURL = trimmedBaseUrl

        // 1. Save API Key to Keychain
        do {
            try KeychainService.saveApiKey(trimmedKey)
        } catch {
            connectionMessage = "Keychain 保存失败: \(error.localizedDescription)"
            return false
        }

        // 2. Apply model config to Rust Core
        do {
            try bridge.updateModelConfig(
                apiKey: trimmedKey,
                modelName: modelName,
                baseUrl: trimmedBaseUrl.isEmpty ? nil : trimmedBaseUrl
            )
            connectionMessage = "配置已保存并应用"
            return true
        } catch {
            connectionMessage = "模型配置更新失败: \(error.localizedDescription)"
            return false
        }
    }

    /// Internal: apply model config without saving Keychain (used on app launch)
    func applyModel(to bridge: CoreBridge?) {
        guard let bridge else { return }

        let trimmedKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedModel = modelName.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedBaseUrl = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !trimmedKey.isEmpty else { return }

        do {
            try bridge.updateModelConfig(
                apiKey: trimmedKey,
                modelName: trimmedModel.isEmpty ? "openrouter/free" : trimmedModel,
                baseUrl: trimmedBaseUrl.isEmpty ? nil : trimmedBaseUrl
            )
        } catch {
            connectionMessage = "模型配置更新失败: \(error.localizedDescription)"
        }
    }

    /// Test OpenRouter connection using current model settings
    func testConnection(bridge: CoreBridge?) {
        guard let bridge else {
            connectionMessage = "Core 未初始化"
            return
        }

        let trimmedKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedKey.isEmpty {
            connectionMessage = "请先填写 OpenRouter API Key"
            return
        }

        isTesting = true
        connectionMessage = "正在测试连接..."

        do {
            // Always apply current form values before testing.
            let trimmedModel = modelName.trimmingCharacters(in: .whitespacesAndNewlines)
            let trimmedBaseUrl = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)

            try bridge.updateModelConfig(
                apiKey: trimmedKey,
                modelName: trimmedModel.isEmpty ? "openrouter/free" : trimmedModel,
                baseUrl: trimmedBaseUrl.isEmpty ? nil : trimmedBaseUrl
            )
            try bridge.testModelConnection()
            connectionMessage = "✅ 连接成功"
        } catch {
            connectionMessage = "❌ 连接失败: \(error.localizedDescription)"
        }
        isTesting = false
    }
}
