import SwiftUI

struct AboutSettingsView: View {
    let bridge: CoreBridge?

    @State private var kbPath: String = "-"
    @State private var fileCount: UInt32 = 0
    @State private var updatedAtText: String = "-"
    @State private var statusMessage: String = ""

    var body: some View {
        Form {
            Section("应用信息") {
                LabeledContent("应用名称", value: "Alawyer")
                LabeledContent("版本", value: appVersion)
            }

            Section("知识库信息") {
                LabeledContent("文档数量", value: "\(fileCount)")
                LabeledContent("最后更新", value: updatedAtText)
                LabeledContent("知识库路径", value: kbPath)
                    .lineLimit(2)
                    .textSelection(.enabled)

                Button("刷新知识库信息", action: refreshKnowledgeInfo)
            }

            if !statusMessage.isEmpty {
                Text(statusMessage)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .onAppear(perform: refreshKnowledgeInfo)
    }

    private var appVersion: String {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String
        if let version, let build {
            return "\(version) (\(build))"
        }
        return version ?? "开发版本"
    }

    private func refreshKnowledgeInfo() {
        guard let bridge else {
            statusMessage = "Core 未初始化"
            return
        }

        do {
            let info = try bridge.getKnowledgeInfo()
            kbPath = info.kbPath
            fileCount = info.fileCount

            if info.updatedAt > 0 {
                let date = Date(timeIntervalSince1970: TimeInterval(info.updatedAt))
                let formatter = DateFormatter()
                formatter.dateStyle = .medium
                formatter.timeStyle = .short
                updatedAtText = formatter.string(from: date)
            } else {
                updatedAtText = "暂无"
            }
            statusMessage = ""
        } catch {
            statusMessage = "读取知识库信息失败: \(error.localizedDescription)"
        }
    }
}
