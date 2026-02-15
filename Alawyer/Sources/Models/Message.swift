import Foundation

struct ChatLine: Identifiable, Hashable {
    let id = UUID()
    let role: String
    let content: String
    let timestamp: Date
}
