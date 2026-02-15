import XCTest
import CoreBindings

final class AlawyerTests: XCTestCase {
    func testRustHelloAndStorageRoundTrip() throws {
        let tempRoot = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("alawyer-test-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempRoot, withIntermediateDirectories: true)

        let dbPath = tempRoot.appendingPathComponent("core.sqlite").path
        let kbPath = tempRoot.appendingPathComponent("kb", isDirectory: true).path

        let core = try Core(
            config: CoreConfig(
                kbPath: kbPath,
                dbPath: dbPath,
                maxIterations: 5
            )
        )

        XCTAssertEqual(core.hello(), "hello from alawyer-core (rust)")
        let sessionId = try core.createSession(scenario: "labor", title: "测试")
        _ = try core.createMessage(
            sessionId: sessionId,
            role: "user",
            content: "hello",
            phase: "plan",
            toolCallsJson: nil
        )
        let messages = try core.getMessages(sessionId: sessionId)
        XCTAssertEqual(messages.count, 1)
        XCTAssertEqual(messages[0].content, "hello")
    }
}
