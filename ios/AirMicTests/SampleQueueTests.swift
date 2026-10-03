import Testing
@testable import AirMic

struct SampleQueueTests {
    @Test func readsBackInOrderAcrossWrap() {
        let queue = SampleQueue(capacity: 8)
        var out = [Float](repeating: 0, count: 8)

        write(queue, [1, 2, 3, 4, 5, 6])
        #expect(read(queue, &out, max: 4) == [1, 2, 3, 4])
        write(queue, [7, 8, 9, 10])
        #expect(read(queue, &out, max: 8) == [5, 6, 7, 8, 9, 10])
    }

    @Test func dropsOldestOnOverflow() {
        let queue = SampleQueue(capacity: 4)
        var out = [Float](repeating: 0, count: 4)
        write(queue, [1, 2, 3, 4, 5, 6])
        #expect(read(queue, &out, max: 4) == [3, 4, 5, 6])
        #expect(queue.droppedSamples == 2)
    }

    private func write(_ queue: SampleQueue, _ samples: [Float]) {
        samples.withUnsafeBufferPointer { queue.write($0.baseAddress!, count: $0.count) }
    }

    private func read(_ queue: SampleQueue, _ out: inout [Float], max: Int) -> [Float] {
        let count = out.withUnsafeMutableBufferPointer { queue.read(into: $0.baseAddress!, max: max) }
        return Array(out[0..<count])
    }
}
