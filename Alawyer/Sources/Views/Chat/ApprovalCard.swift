import SwiftUI

struct EventCard: View {
    let event: CoreEventViewData

    var body: some View {
        HStack(alignment: .top, spacing: 6) {
            Image(systemName: iconName)
                .foregroundStyle(iconColor)
                .font(.caption)

            VStack(alignment: .leading, spacing: 2) {
                Text(event.kind)
                    .font(.caption.bold())
                Text(event.payload)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
            }
        }
        .padding(8)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 6))
    }

    private var iconName: String {
        switch event.kind {
        case "agent_phase": return "brain"
        case "intake_progress": return "list.clipboard"
        case "tool_call_request": return "hand.raised"
        case "completed": return "checkmark.circle"
        case "cancelled": return "xmark.circle"
        case "error": return "exclamationmark.triangle"
        default: return "bolt"
        }
    }

    private var iconColor: Color {
        switch event.kind {
        case "completed": return .green
        case "error": return .red
        case "cancelled": return .orange
        case "tool_call_request": return .yellow
        default: return .blue
        }
    }
}

/// Shown when a tool needs user approval (tool permission = "ask")
struct ApprovalCard: View {
    let approval: PendingToolApproval
    let onRespond: (String, String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Image(systemName: "hand.raised.fill")
                    .foregroundStyle(.yellow)
                Text("工具审批请求")
                    .font(.headline)
            }

            Text("工具「\(approval.toolName)」请求执行，需要您的批准。")
                .font(.callout)
                .foregroundStyle(.secondary)

            if let args = approval.argumentsPreview, !args.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("参数")
                        .font(.caption.bold())
                    Text(args)
                        .font(.caption.monospaced())
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(8)
                        .background(.background, in: RoundedRectangle(cornerRadius: 8))
                }
            }

            HStack(spacing: 8) {
                Button("拒绝") {
                    onRespond(approval.requestId, "deny")
                }
                .buttonStyle(.bordered)

                Button("仅此次允许") {
                    onRespond(approval.requestId, "allow")
                }
                .buttonStyle(.borderedProminent)
                .tint(.green)

                Button("总是允许") {
                    onRespond(approval.requestId, "always")
                }
                .buttonStyle(.bordered)

                Button("本次全部允许") {
                    onRespond(approval.requestId, "allowAll")
                }
                .buttonStyle(.bordered)
            }
        }
        .padding(12)
        .background(.orange.opacity(0.1), in: RoundedRectangle(cornerRadius: 10))
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(.orange.opacity(0.3), lineWidth: 1)
        )
    }
}
