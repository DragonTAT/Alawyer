import SwiftUI

struct PermissionSettingsView: View {
    let bridge: CoreBridge?

    private let toolNames = [
        "ask_user",
        "kb_search",
        "kb_read",
        "cite",
        "summarize_facts",
        "check_safety",
        "suggest_escalation",
    ]

    @State private var permissions: [String: String] = [:]
    @State private var statusMessage: String = ""

    var body: some View {
        Form {
            Section(header: Text("工具权限").font(.headline)) {
                Text("控制 Agent 运行时对每个工具的使用权限")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                ForEach(toolNames, id: \.self) { toolName in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(displayName(for: toolName))
                                .font(.body)
                            Text(toolName)
                                .font(.caption2)
                                .foregroundStyle(.tertiary)
                        }

                        Spacer()

                        Picker("", selection: Binding(
                            get: { permissions[toolName] ?? "ask" },
                            set: { newValue in
                                permissions[toolName] = newValue
                                setPermission(toolName: toolName, permission: newValue)
                            }
                        )) {
                            Text("总是允许").tag("allow")
                            Text("每次询问").tag("ask")
                            Text("禁止").tag("deny")
                        }
                        .pickerStyle(.segmented)
                        .frame(width: 220)
                    }
                }
            }

            if !statusMessage.isEmpty {
                Text(statusMessage)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .onAppear {
            loadPermissions()
        }
    }

    private func displayName(for toolName: String) -> String {
        switch toolName {
        case "ask_user": return "追问用户"
        case "kb_search": return "知识库检索"
        case "kb_read": return "知识库阅读"
        case "cite": return "引用法条"
        case "summarize_facts": return "事实摘要"
        case "check_safety": return "安全审查"
        case "suggest_escalation": return "建议咨询律师"
        default: return toolName
        }
    }

    private func loadPermissions() {
        guard let bridge else { return }
        for toolName in toolNames {
            if let perm = try? bridge.getToolPermission(toolName: toolName) {
                permissions[toolName] = perm
            }
        }
    }

    private func setPermission(toolName: String, permission: String) {
        guard let bridge else {
            statusMessage = "Core 未初始化"
            return
        }
        do {
            try bridge.setToolPermission(toolName: toolName, permission: permission)
            statusMessage = "已更新「\(displayName(for: toolName))」→ \(permission)"
        } catch {
            statusMessage = "设置失败: \(error.localizedDescription)"
        }
    }
}
