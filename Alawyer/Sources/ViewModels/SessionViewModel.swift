import Foundation

@MainActor
final class SessionViewModel: ObservableObject {
    @Published var sessions: [SessionSummary] = []
    @Published var selectedSessionId: String?
}
