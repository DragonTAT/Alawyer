import Foundation

struct CoreEventViewData: Identifiable, Hashable {
    let id = UUID()
    let kind: String
    let payload: String
    let timestamp: Date
}
