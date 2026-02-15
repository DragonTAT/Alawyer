import SwiftUI

struct ChatView: View {
    @ObservedObject var viewModel: ChatViewModel

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text(viewModel.currentSessionSummary?.scenarioDisplayName ?? "劳动仲裁")
                    .font(.caption.bold())
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(.blue.opacity(0.12), in: Capsule())

                Text(viewModel.currentSessionSummary?.title ?? "未命名会话")
                    .font(.headline)
                    .lineLimit(1)

                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.top, 10)
            .padding(.bottom, 6)

            // Status bar
            HStack {
                Text(viewModel.statusText)
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Spacer()

                if viewModel.isAgentRunning {
                    ProgressView()
                        .controlSize(.small)

                    Button("取消") {
                        viewModel.cancelCurrentTask()
                    }
                    .font(.caption)
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)

            Divider()

            if viewModel.hasReport {
                HStack(spacing: 8) {
                    Button("复制全文") {
                        viewModel.copyLatestReport()
                    }
                    .buttonStyle(.bordered)

                    Button("导出MD") {
                        viewModel.exportLatestReportMarkdown()
                    }
                    .buttonStyle(.bordered)

                    Button("重新生成") {
                        viewModel.regenerateReport()
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(viewModel.isAgentRunning)

                    Spacer()
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 6)

                Divider()
            }

            // Intake progress (shown during intake phase)
            if let progress = viewModel.intakeProgress {
                IntakeProgressView(current: progress.current, total: progress.total)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
            }

            // Tool approval card (shown when Agent waits for permission)
            if let approval = viewModel.pendingToolRequest {
                ApprovalCard(approval: approval) { requestId, action in
                    viewModel.respondToToolCall(requestId: requestId, action: action)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
            }

            // Messages list
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 10) {
                        ForEach(viewModel.messages) { message in
                            MessageRow(line: message)
                                .id(message.id)
                        }

                        if !viewModel.streamingAssistantText.isEmpty {
                            MessageRow(
                                line: ChatLine(
                                    role: "assistant",
                                    content: viewModel.streamingAssistantText,
                                    timestamp: Date()
                                )
                            )
                        }
                    }
                    .padding(12)
                }
                .onChange(of: viewModel.messages.count) { _, _ in
                    if let last = viewModel.messages.last {
                        withAnimation(.easeOut(duration: 0.2)) {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        }
                    }
                }
            }

            Divider()

            // Event log (collapsible)
            if !viewModel.events.isEmpty {
                DisclosureGroup("事件日志 (\(viewModel.events.count))") {
                    ScrollView(.vertical) {
                        LazyVStack(alignment: .leading, spacing: 4) {
                            ForEach(viewModel.events) { event in
                                EventCard(event: event)
                            }
                        }
                        .padding(4)
                    }
                    .frame(maxHeight: 120)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 4)
                .font(.caption)

                Divider()
            }

            // Input
            if viewModel.canSkipIntakeQuestion {
                InputView(
                    text: $viewModel.inputText,
                    onSend: viewModel.send,
                    onSkip: { viewModel.skipCurrentIntakeQuestion() }
                )
                .padding(12)
                .disabled(viewModel.isAgentRunning)
            } else {
                InputView(
                    text: $viewModel.inputText,
                    onSend: viewModel.send
                )
                .padding(12)
                .disabled(viewModel.isAgentRunning)
            }
        }
    }
}
