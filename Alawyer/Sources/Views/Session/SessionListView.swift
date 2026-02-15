import SwiftUI

struct SessionListView: View {
    @ObservedObject var viewModel: SessionViewModel
    @ObservedObject var chatViewModel: ChatViewModel

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("会话")
                    .font(.headline)
                Spacer()
                Button {
                    chatViewModel.createNewSession()
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.borderless)
                .help("新建会话")
            }
            .padding(.horizontal, 12)
            .padding(.top, 12)
            .padding(.bottom, 8)

            if viewModel.sessions.isEmpty {
                EmptyStateView {
                    chatViewModel.createNewSession()
                }
            } else {
                List(selection: $viewModel.selectedSessionId) {
                    ForEach(viewModel.sessions) { session in
                        SessionRow(session: session)
                            .tag(session.id)
                            .contextMenu {
                                Button(role: .destructive) {
                                    chatViewModel.deleteSession(id: session.id)
                                } label: {
                                    Label("删除会话", systemImage: "trash")
                                }
                            }
                    }
                }
                .listStyle(.sidebar)
                .onChange(of: viewModel.selectedSessionId) { _, newValue in
                    if let newValue {
                        chatViewModel.selectSession(id: newValue)
                    }
                }
            }
        }
    }
}
