import SwiftUI

struct EmptyStateView: View {
    let onCreate: () -> Void

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: "doc.text.magnifyingglass")
                .font(.largeTitle)
                .foregroundStyle(.secondary)
            Text("还没有任何会话")
                .font(.headline)
            Text("点击下方按钮开始您的第一次法律咨询")
                .font(.caption)
                .foregroundStyle(.secondary)
            Button {
                onCreate()
            } label: {
                Label("新建会话", systemImage: "plus")
            }
            .buttonStyle(.borderedProminent)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }
}
