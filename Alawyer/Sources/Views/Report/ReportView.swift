import SwiftUI

struct ReportView: View {
    let content: String

    var body: some View {
        ScrollView {
            Text(content)
                .frame(maxWidth: .infinity, alignment: .leading)
                .textSelection(.enabled)
                .padding()
        }
    }
}
