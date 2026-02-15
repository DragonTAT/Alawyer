import Foundation
import Combine
import CoreBindings
import AppKit
import UniformTypeIdentifiers

@MainActor
final class ChatViewModel: ObservableObject {
    @Published var messages: [ChatLine] = []
    @Published var events: [CoreEventViewData] = []
    @Published var statusText: String = "准备就绪"
    @Published var inputText: String = ""
    @Published var currentSessionId: String?
    @Published var hasCompletedOnboarding: Bool = false
    @Published var isAgentRunning: Bool = false
    @Published var intakeProgress: (current: Int, total: Int)?
    @Published var pendingToolRequest: PendingToolApproval?
    @Published var streamingAssistantText: String = ""

    /// The current Agent task_id (if running)
    private var currentTaskId: String?

    let sessionViewModel = SessionViewModel()
    let settingsViewModel = SettingsViewModel()

    private(set) var bridge: CoreBridge?

    init() {
        bootstrap()
    }

    deinit {
        bridge?.unsubscribe()
    }

    var currentSessionSummary: SessionSummary? {
        guard let currentSessionId else { return nil }
        return sessionViewModel.sessions.first { $0.id == currentSessionId }
    }

    var latestReportContent: String? {
        messages.last(where: {
            $0.role == "assistant"
                && $0.content.contains("【事实摘要】")
                && $0.content.contains("【免责声明】")
        })?.content
    }

    var hasReport: Bool {
        latestReportContent != nil
    }

    var canSkipIntakeQuestion: Bool {
        intakeProgress != nil && !isAgentRunning
    }

    // MARK: - Send Message (Sprint 2: full Agent pipeline)

    func send() {
        let trimmed = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return
        }
        inputText = ""
        submitUserInput(trimmed, displayContent: trimmed)
    }

    func skipCurrentIntakeQuestion() {
        guard canSkipIntakeQuestion else { return }
        submitUserInput("（用户跳过此题）", displayContent: "（已跳过此问题）")
    }

    func copyLatestReport() {
        guard let sessionId = currentSessionId else {
            statusText = "请先创建会话"
            return
        }

        let report: String
        if let bridge, let generated = try? bridge.generateReport(sessionId: sessionId) {
            report = generated
        } else if let local = latestReportContent {
            report = local
        } else {
            statusText = "暂无可复制的报告"
            return
        }

        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(report, forType: .string)
        statusText = "📋 已复制报告全文"
    }

    func exportLatestReportMarkdown() {
        let panel = NSSavePanel()
        panel.canCreateDirectories = true
        if let markdownType = UTType(filenameExtension: "md") {
            panel.allowedContentTypes = [markdownType]
        } else {
            panel.allowedContentTypes = [.plainText]
        }
        panel.nameFieldStringValue = suggestedMarkdownFileName()

        let result = panel.runModal()
        guard result == .OK, let url = panel.url else {
            return
        }

        guard let sessionId = currentSessionId else {
            statusText = "请先创建会话"
            return
        }

        do {
            if let bridge {
                try bridge.exportReportMarkdown(sessionId: sessionId, path: url.path)
            } else {
                guard let report = latestReportContent else {
                    statusText = "暂无可导出的报告"
                    return
                }
                try report.write(to: url, atomically: true, encoding: .utf8)
            }
            statusText = "✅ 已导出 Markdown：\(url.lastPathComponent)"
        } catch {
            statusText = "导出失败: \(error.localizedDescription)"
        }
    }

    func regenerateReport() {
        guard let bridge, let sessionId = currentSessionId else {
            statusText = "请先创建会话"
            return
        }

        do {
            let taskId = try bridge.regenerateReport(sessionId: sessionId)
            currentTaskId = taskId
            isAgentRunning = true
            pendingToolRequest = nil
            streamingAssistantText = ""
            statusText = "♻️ 正在重新生成报告..."
        } catch {
            statusText = "重新生成失败: \(error.localizedDescription)"
            isAgentRunning = false
        }
    }

    private func submitUserInput(_ content: String, displayContent: String) {
        guard let bridge, let sessionId = currentSessionId else {
            statusText = "请先创建会话"
            return
        }

        maybeAutoTitle(sessionId: sessionId, firstUserInput: content)
        messages.append(ChatLine(role: "user", content: displayContent, timestamp: Date()))
        streamingAssistantText = ""

        do {
            let taskId = try bridge.sendMessage(sessionId: sessionId, content: content)
            currentTaskId = taskId
            isAgentRunning = true
            statusText = "正在整理你的问题..."
        } catch {
            statusText = "发送失败: \(error.localizedDescription)"
            isAgentRunning = false
        }
    }

    /// Cancel the current running agent task
    func cancelCurrentTask() {
        guard let bridge, let taskId = currentTaskId else { return }
        do {
            try bridge.cancelAgentTask(taskId: taskId)
        } catch {
            statusText = "取消失败: \(error.localizedDescription)"
        }
    }

    /// Respond to a tool approval request
    func respondToToolCall(requestId: String, action: String) {
        guard let bridge else { return }
        do {
            let response: ToolResponse
            switch action {
            case "allow":
                response = .allow(always: false)
            case "always":
                response = .allow(always: true)
            case "allowAll":
                response = .allowAllThisSession
            case "deny":
                response = .deny
            default:
                response = .deny
            }
            try bridge.respondToolCall(requestId: requestId, response: response)
            pendingToolRequest = nil
        } catch {
            statusText = "审批失败: \(error.localizedDescription)"
        }
    }

    // MARK: - Session Management

    func createNewSession() {
        guard let bridge else { return }
        do {
            let sessionId = try bridge.createSession(scenario: "labor", title: nil)
            currentSessionId = sessionId
            messages = []
            events = []
            intakeProgress = nil
            pendingToolRequest = nil
            isAgentRunning = false
            streamingAssistantText = ""
            reloadSessions()
            selectSession(id: sessionId)
        } catch {
            statusText = "创建会话失败: \(error.localizedDescription)"
        }
    }

    func selectSession(id: String) {
        currentSessionId = id
        sessionViewModel.selectedSessionId = id
        intakeProgress = nil
        pendingToolRequest = nil
        isAgentRunning = false
        currentTaskId = nil
        streamingAssistantText = ""
        loadMessages(for: id)
    }

    func deleteSession(id: String) {
        guard let bridge else { return }
        do {
            try bridge.deleteSession(sessionId: id)
            if currentSessionId == id {
                currentSessionId = nil
                messages = []
                events = []
                intakeProgress = nil
                pendingToolRequest = nil
                currentTaskId = nil
                streamingAssistantText = ""
            }
            reloadSessions()
            // Select the most recent remaining session if any
            if let first = sessionViewModel.sessions.first {
                selectSession(id: first.id)
            }
        } catch {
            statusText = "删除失败: \(error.localizedDescription)"
        }
    }

    func reloadSessions() {
        guard let bridge else { return }
        do {
            let sessions = try bridge.listSessions()
            sessionViewModel.sessions = sessions.map {
                SessionSummary(
                    id: $0.id,
                    title: $0.title ?? "未命名会话",
                    scenario: $0.scenario,
                    updatedAt: Date(timeIntervalSince1970: TimeInterval($0.updatedAt))
                )
            }
        } catch {
            statusText = "加载会话列表失败: \(error.localizedDescription)"
        }
    }

    // MARK: - Onboarding

    func completeOnboarding() {
        guard let bridge else { return }
        let applied = settingsViewModel.saveAndApply(to: bridge)
        guard applied else {
            statusText = settingsViewModel.connectionMessage
            return
        }
        hasCompletedOnboarding = true

        // Load existing sessions or show empty state
        reloadSessions()
        if let first = sessionViewModel.sessions.first {
            selectSession(id: first.id)
        }
    }

    // MARK: - Private

    private func bootstrap() {
        do {
            let paths = try Self.makeAppPaths()
            try? Self.seedKnowledgeBaseIfNeeded(at: paths.kbPath)
            let bridge = try CoreBridge(dbPath: paths.dbPath, kbPath: paths.kbPath)
            try bridge.subscribe { [weak self] event in
                Task { @MainActor in
                    self?.handleCoreEvent(event)
                }
            }
            self.bridge = bridge
            self.statusText = bridge.coreInfo()

            // Load API key from Keychain
            let loadedApiKey = (try? KeychainService.loadApiKey()) ?? ""
            settingsViewModel.apiKey = loadedApiKey

            // Check if onboarding is needed
            if loadedApiKey.isEmpty {
                hasCompletedOnboarding = false
            } else {
                hasCompletedOnboarding = true
                // Auto-apply saved model config on launch
                settingsViewModel.applyModel(to: bridge)
            }

            // Load existing sessions (don't create a new one on every launch)
            reloadSessions()
            if let first = sessionViewModel.sessions.first {
                selectSession(id: first.id)
            }
        } catch {
            statusText = "启动失败: \(error.localizedDescription)"
        }
    }

    private func loadMessages(for sessionId: String) {
        guard let bridge else { return }
        do {
            let msgs = try bridge.getMessages(sessionId: sessionId)
            messages = msgs.map {
                ChatLine(
                    role: $0.role,
                    content: $0.content,
                    timestamp: Date(timeIntervalSince1970: TimeInterval($0.createdAt))
                )
            }
        } catch {
            statusText = "加载消息失败: \(error.localizedDescription)"
        }
    }

    // MARK: - Event Handling (Sprint 2)

    private func handleCoreEvent(_ event: CoreEvent) {
        // Always record event in the event log
        events.insert(
            CoreEventViewData(
                kind: event.kind,
                payload: event.payload,
                timestamp: Date(timeIntervalSince1970: TimeInterval(event.timestamp))
            ),
            at: 0
        )

        // Process Sprint 2 events by kind
        switch event.kind {
        case "agent_phase":
            handleAgentPhase(event.payload)

        case "stream_chunk":
            handleStreamChunk(event.payload)

        case "intake_progress":
            handleIntakeProgress(event.payload)

        case "intake_done":
            statusText = "信息已收集完成，正在生成报告..."

        case "tool_call_request":
            handleToolCallRequest(event.payload)

        case "tool_call_response":
            // Tool was approved/denied — clear pending request
            pendingToolRequest = nil

        case "completed":
            handleCompleted(event.payload)

        case "cancelled":
            handleCancelled()

        case "error":
            handleError(event.payload)

        default:
            break
        }
    }

    private func handleAgentPhase(_ payload: String) {
        guard let data = parseJSON(payload),
              let phase = data["phase"] as? String else { return }

        switch phase {
        case "planning":
            statusText = "📋 正在分析案情..."
        case "drafting":
            statusText = "✏️ 正在生成报告..."
        case "reviewing":
            statusText = "🔍 正在审查内容..."
        default:
            statusText = "处理中: \(phase)"
        }
    }

    private func handleIntakeProgress(_ payload: String) {
        guard let data = parseJSON(payload),
              let current = data["current"] as? Int,
              let total = data["total"] as? Int else { return }

        intakeProgress = (current: current, total: total)
        statusText = "正在梳理案情（\(current)/\(total)）"

        // Reload messages to show the new intake question
        if let sessionId = currentSessionId {
            loadMessages(for: sessionId)
        }
    }

    private func handleToolCallRequest(_ payload: String) {
        guard let data = parseJSON(payload),
              let requestId = data["request_id"] as? String,
              let toolName = data["tool_name"] as? String else { return }

        pendingToolRequest = PendingToolApproval(
            requestId: requestId,
            toolName: toolName,
            argumentsPreview: data["arguments"].map(previewJSON),
            timestamp: Date()
        )
        statusText = "⏳ 工具「\(toolName)」需要您的审批"
    }

    private func handleCompleted(_ payload: String) {
        isAgentRunning = false
        currentTaskId = nil
        pendingToolRequest = nil
        streamingAssistantText = ""

        // Reload messages to include the Agent's output
        if let sessionId = currentSessionId {
            loadMessages(for: sessionId)
        }
        reloadSessions()

        let data = parseJSON(payload)
        if data?["report"] != nil {
            intakeProgress = nil
            statusText = "✅ 报告已生成"
        } else if data?["message"] != nil {
            statusText = "我们继续补全信息，我再问你下一题。"
        } else {
            statusText = "✅ 完成"
        }
    }

    private func handleCancelled() {
        isAgentRunning = false
        currentTaskId = nil
        pendingToolRequest = nil
        streamingAssistantText = ""
        statusText = "⏹ 已取消"
    }

    private func handleError(_ payload: String) {
        isAgentRunning = false
        currentTaskId = nil
        pendingToolRequest = nil
        streamingAssistantText = ""

        let message: String
        if let data = parseJSON(payload), let msg = data["message"] as? String {
            message = msg
        } else {
            message = payload
        }
        statusText = "❌ 错误: \(message)"
    }

    private func handleStreamChunk(_ payload: String) {
        let chunk: String
        if let data = parseJSON(payload), let content = data["content"] as? String {
            chunk = content
        } else {
            chunk = payload
        }
        guard !chunk.isEmpty else { return }

        streamingAssistantText += chunk
        statusText = "✍️ 正在输出..."
    }

    private func parseJSON(_ str: String) -> [String: Any]? {
        guard let data = str.data(using: .utf8) else { return nil }
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    }

    private func previewJSON(_ value: Any) -> String {
        guard JSONSerialization.isValidJSONObject(value),
              let data = try? JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted]),
              let str = String(data: data, encoding: .utf8) else {
            return "\(value)"
        }
        return str
    }

    private func maybeAutoTitle(sessionId: String, firstUserInput: String) {
        guard let bridge else { return }
        guard let current = sessionViewModel.sessions.first(where: { $0.id == sessionId }) else {
            return
        }

        let currentTitle = current.title.trimmingCharacters(in: .whitespacesAndNewlines)
        let isUntitled = currentTitle.isEmpty || currentTitle == "未命名会话"
        guard isUntitled else { return }

        let compact = firstUserInput
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !compact.isEmpty else { return }

        let maxLength = 18
        let newTitle: String
        if compact.count > maxLength {
            newTitle = String(compact.prefix(maxLength)) + "..."
        } else {
            newTitle = compact
        }

        do {
            try bridge.updateSessionTitle(sessionId: sessionId, title: newTitle)
            if let idx = sessionViewModel.sessions.firstIndex(where: { $0.id == sessionId }) {
                sessionViewModel.sessions[idx].title = newTitle
                sessionViewModel.sessions[idx].updatedAt = Date()
            }
        } catch {
            statusText = "会话标题更新失败: \(error.localizedDescription)"
        }
    }

    private func suggestedMarkdownFileName() -> String {
        let title = currentSessionSummary?.title.trimmingCharacters(in: .whitespacesAndNewlines)
        let fallback = "法律咨询报告"
        let base = (title?.isEmpty == false ? title! : fallback)
        let sanitized = base.replacingOccurrences(
            of: #"[/\\:*?"<>|]"#,
            with: "-",
            options: .regularExpression
        )
        return "\(sanitized).md"
    }

    static func makeAppPaths() throws -> (dbPath: String, kbPath: String) {
        let fileManager = FileManager.default
        let base = try fileManager.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        ).appendingPathComponent("Alawyer", isDirectory: true)

        try fileManager.createDirectory(at: base, withIntermediateDirectories: true)

        let dbPath = base.appendingPathComponent("alawyer.sqlite").path
        let kbPath = base.appendingPathComponent("kb", isDirectory: true).path
        return (dbPath, kbPath)
    }

    static func seedKnowledgeBaseIfNeeded(at kbPath: String) throws {
        let fileManager = FileManager.default
        let kbURL = URL(fileURLWithPath: kbPath, isDirectory: true)
        try fileManager.createDirectory(at: kbURL, withIntermediateDirectories: true)

        if try hasMarkdownFiles(in: kbURL) {
            return
        }

        guard let seedRoot = Bundle.module.url(forResource: "SeedKB", withExtension: nil) else {
            return
        }
        let sourceLabor = seedRoot.appendingPathComponent("labor", isDirectory: true)
        guard fileManager.fileExists(atPath: sourceLabor.path) else {
            return
        }

        let targetLabor = kbURL.appendingPathComponent("labor", isDirectory: true)
        try fileManager.createDirectory(at: targetLabor, withIntermediateDirectories: true)

        let files = try fileManager.contentsOfDirectory(
            at: sourceLabor,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        )

        for file in files where file.pathExtension.lowercased() == "md" {
            let destination = targetLabor.appendingPathComponent(file.lastPathComponent)
            if fileManager.fileExists(atPath: destination.path) {
                try fileManager.removeItem(at: destination)
            }
            try fileManager.copyItem(at: file, to: destination)
        }
    }

    static func hasMarkdownFiles(in rootURL: URL) throws -> Bool {
        let fileManager = FileManager.default
        let enumerator = fileManager.enumerator(
            at: rootURL,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        )

        while let item = enumerator?.nextObject() as? URL {
            if item.pathExtension.lowercased() == "md" {
                return true
            }
        }
        return false
    }
}

/// Model for pending tool approval requests
struct PendingToolApproval: Identifiable {
    let id = UUID()
    let requestId: String
    let toolName: String
    let argumentsPreview: String?
    let timestamp: Date
}
