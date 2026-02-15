import Foundation
import CoreBindings

final class CoreBridge {
    private let core: Core
    private var eventSink: EventSink?
    private(set) var subscription: Subscription?

    init(dbPath: String, kbPath: String, maxIterations: UInt32 = 15) throws {
        let config = CoreConfig(kbPath: kbPath, dbPath: dbPath, maxIterations: maxIterations)
        self.core = try Core(config: config)
    }

    func hello() -> String {
        core.hello()
    }

    func coreInfo() -> String {
        core.coreInfo()
    }

    func subscribe(onEvent: @escaping (CoreEvent) -> Void) throws {
        let sink = EventSink(onEvent: onEvent)
        let token = try core.subscribeEvents(listener: sink)
        self.eventSink = sink
        self.subscription = token
    }

    func unsubscribe() {
        guard let subscription else {
            return
        }
        try? core.unsubscribeEvents(subscriptionId: subscription.id)
        self.subscription = nil
        self.eventSink = nil
    }

    func emitTestEvent(_ text: String) {
        core.emitTestEvent(message: text)
    }

    // MARK: - Session Management

    func createSession(scenario: String, title: String?) throws -> String {
        try core.createSession(scenario: scenario, title: title)
    }

    func listSessions() throws -> [Session] {
        try core.listSessions()
    }

    func deleteSession(sessionId: String) throws {
        try core.deleteSession(sessionId: sessionId)
    }

    func updateSessionTitle(sessionId: String, title: String) throws {
        try core.updateSessionTitle(sessionId: sessionId, title: title)
    }

    // MARK: - Message

    func createMessage(
        sessionId: String,
        role: String,
        content: String,
        phase: String? = nil,
        toolCallsJson: String? = nil
    ) throws -> Message {
        try core.createMessage(
            sessionId: sessionId,
            role: role,
            content: content,
            phase: phase,
            toolCallsJson: toolCallsJson
        )
    }

    func getMessages(sessionId: String) throws -> [Message] {
        try core.getMessages(sessionId: sessionId)
    }

    // MARK: - Model

    func updateModelConfig(apiKey: String, modelName: String, baseUrl: String? = nil) throws {
        let config = ModelConfig(
            apiKey: apiKey,
            modelName: modelName,
            baseUrl: baseUrl,
            retryMaxRetries: 3,
            retryInitialDelayMs: 200,
            retryMaxDelayMs: 10_000,
            retryBackoffFactor: 2.0
        )
        try core.updateModelConfig(config: config)
    }

    func testModelConnection() throws {
        try core.testModelConnection()
    }

    func pingModel(prompt: String) throws -> String {
        try core.pingModel(prompt: prompt)
    }

    // MARK: - Agent Runtime (Sprint 2)

    /// Send a message and start the Agent pipeline (Plan → Draft → Review).
    /// Returns the task_id for tracking / cancellation.
    func sendMessage(sessionId: String, content: String) throws -> String {
        try core.sendMessage(sessionId: sessionId, content: content)
    }

    /// Cancel a running agent task
    func cancelAgentTask(taskId: String) throws {
        try core.cancelAgentTask(taskId: taskId)
    }

    /// Respond to a tool approval request
    func respondToolCall(requestId: String, response: ToolResponse) throws {
        try core.respondToolCall(requestId: requestId, response: response)
    }

    /// List all registered tools
    func listTools() -> [String] {
        core.listTools()
    }

    // MARK: - Knowledge Base

    func searchKnowledge(query: String, scenario: String, topK: UInt32) throws -> [SearchResult] {
        try core.searchKnowledge(query: query, scenario: scenario, topK: topK)
    }

    func readKnowledgeFile(filePath: String) throws -> String {
        try core.readKnowledgeFile(filePath: filePath)
    }

    func getKnowledgeInfo() throws -> KnowledgeInfo {
        try core.getKnowledgeInfo()
    }

    // MARK: - Report

    func generateReport(sessionId: String) throws -> String {
        try core.generateReport(sessionId: sessionId)
    }

    func exportReportMarkdown(sessionId: String, path: String) throws {
        try core.exportReportMarkdown(sessionId: sessionId, path: path)
    }

    func regenerateReport(sessionId: String) throws -> String {
        try core.regenerateReport(sessionId: sessionId)
    }

    // MARK: - Settings & Permissions

    func setSetting(key: String, value: String) throws {
        try core.setSetting(key: key, value: value)
    }

    func getSetting(key: String) throws -> String? {
        try core.getSetting(key: key)
    }

    func setToolPermission(toolName: String, permission: String) throws {
        try core.setToolPermission(toolName: toolName, permission: permission)
    }

    func getToolPermission(toolName: String) throws -> String {
        try core.getToolPermission(toolName: toolName)
    }

    // MARK: - Logs

    func appendLog(level: String, message: String, sessionId: String? = nil) throws -> Int64 {
        try core.appendLog(level: level, message: message, sessionId: sessionId)
    }

    func listLogs(limit: UInt32 = 100) throws -> [LogEntry] {
        try core.listLogs(limit: limit)
    }
}

private final class EventSink: EventListener {
    private let handler: (CoreEvent) -> Void

    init(onEvent: @escaping (CoreEvent) -> Void) {
        self.handler = onEvent
    }

    func onEvent(event: CoreEvent) {
        handler(event)
    }
}
