import SwiftUI

struct IntakeProgressView: View {
    let current: Int
    let total: Int

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("问诊进度：\(current)/\(total)")
                .font(.caption)
                .foregroundStyle(.secondary)
            ProgressView(value: Double(current), total: Double(max(total, 1)))
        }
    }
}
