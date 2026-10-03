import Dispatch
import Synchronization

/// Single producer, single consumer queue of Float samples.
/// The realtime audio thread writes; the frame processor thread reads.
/// The lock is held only for a short copy into preallocated storage.
final class SampleQueue: Sendable {
    private struct Ring {
        var storage: [Float]
        var readIndex = 0
        var count = 0
        var dropped = 0
    }

    private let ring: Mutex<Ring>
    /// Signalled after each write so the consumer wakes up.
    let available = DispatchSemaphore(value: 0)

    init(capacity: Int) {
        ring = Mutex(Ring(storage: [Float](repeating: 0, count: capacity)))
    }

    /// Called on the realtime thread. Drops the oldest samples if the consumer falls behind.
    func write(_ samples: UnsafePointer<Float>, count: Int) {
        ring.withLock { ring in
            let capacity = ring.storage.count
            // A write bigger than the whole ring keeps only its newest samples.
            let skipped = max(0, count - capacity)
            let samples = samples + skipped
            let count = count - skipped
            ring.dropped += skipped
            let overflow = ring.count + count - capacity
            if overflow > 0 {
                ring.readIndex = (ring.readIndex + overflow) % capacity
                ring.count -= overflow
                ring.dropped += overflow
            }
            var writeIndex = (ring.readIndex + ring.count) % capacity
            ring.storage.withUnsafeMutableBufferPointer { storage in
                for i in 0..<count {
                    storage[writeIndex] = samples[i]
                    writeIndex += 1
                    if writeIndex == capacity { writeIndex = 0 }
                }
            }
            ring.count += count
        }
        available.signal()
    }

    /// Copies up to `max` samples into `destination` and returns how many were copied.
    func read(into destination: UnsafeMutablePointer<Float>, max: Int) -> Int {
        ring.withLock { ring in
            let capacity = ring.storage.count
            let count = min(max, ring.count)
            ring.storage.withUnsafeBufferPointer { storage in
                var readIndex = ring.readIndex
                for i in 0..<count {
                    destination[i] = storage[readIndex]
                    readIndex += 1
                    if readIndex == capacity { readIndex = 0 }
                }
                ring.readIndex = readIndex
            }
            ring.count -= count
            return count
        }
    }

    var droppedSamples: Int {
        ring.withLock { $0.dropped }
    }
}
