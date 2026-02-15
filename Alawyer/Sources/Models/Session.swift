import Foundation

struct SessionSummary: Identifiable, Hashable {
    var id: String
    var title: String
    var scenario: String
    var updatedAt: Date

    var scenarioDisplayName: String {
        switch scenario {
        case "labor": return "劳动仲裁"
        default: return scenario
        }
    }
}
