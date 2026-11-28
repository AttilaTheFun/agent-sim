// Swift Testing tests hosted in XCTestDemo.app (agent-sim's own, beside x86Sim's XCTest sources): what `agent-sim
// xctest` must run of a hosted bundle written with @Test, as Isomer's and Visor's are. One fails on purpose
// (expected.txt).
import Foundation
import Testing

@Suite struct SwiftTestingTests {
    @Test func passes() {
        #expect(1 + 1 == 2)
    }

    @Test func deliberateFailure() {
        #expect(Int("2") == 3)
    }

    @Test func runsInTheHostApp() {
        #expect(Bundle.main.bundleIdentifier == "com.x86sim.test.XCTestDemo")
    }

    @Test(arguments: [1, 2, 3]) func parameterized(n: Int) {
        #expect(n > 0)
    }
}
