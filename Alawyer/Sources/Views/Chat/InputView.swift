import SwiftUI

struct InputView: View {
    @Binding var text: String
    let onSend: () -> Void
    var onSkip: (() -> Void)? = nil

    var body: some View {
        HStack(spacing: 8) {
            TextField("输入你的法律问题", text: $text, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .lineLimit(1...4)

            if let onSkip {
                Button("跳过此题", action: onSkip)
                    .buttonStyle(.bordered)
            }

            Button("发送", action: onSend)
                .keyboardShortcut(.return, modifiers: [])
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
    }
}
