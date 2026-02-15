import SwiftUI

struct MessageRow: View {
    let line: ChatLine

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Text(line.role == "user" ? "你" : "AI")
                .font(.caption.bold())
                .foregroundStyle(line.role == "user" ? .blue : .green)
                .frame(width: 24)

            Text(renderedContent)
                .font(.body)
                .textSelection(.enabled)

            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
    }

    private var renderedContent: AttributedString {
        if let markdown = try? AttributedString(markdown: line.content) {
            return markdown
        }
        return AttributedString(line.content)
    }
}
