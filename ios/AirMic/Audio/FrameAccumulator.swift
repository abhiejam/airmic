import Foundation

/// Cuts a stream of 48 kHz mono samples into exact 10 ms frames (480 samples, 960 bytes).
struct FrameAccumulator {
    static let samplesPerFrame = 480

    private var pending: [Int16] = []

    init() {
        pending.reserveCapacity(Self.samplesPerFrame * 2)
    }

    /// Appends samples and calls `emit` once per completed frame, in order.
    mutating func append(_ samples: UnsafeBufferPointer<Int16>, emit: ([Int16]) -> Void) {
        pending.append(contentsOf: samples)
        var start = 0
        while pending.count - start >= Self.samplesPerFrame {
            emit(Array(pending[start..<start + Self.samplesPerFrame]))
            start += Self.samplesPerFrame
        }
        pending.removeFirst(start)
    }

    mutating func append(_ samples: [Int16], emit: ([Int16]) -> Void) {
        samples.withUnsafeBufferPointer { append($0, emit: emit) }
    }
}

/// One 10 ms frame ready to send.
struct AudioFrame: Sendable {
    /// Frames since capture started; `index * 480` is the sample timestamp.
    let index: UInt32
    /// PCM signed 16 bit little endian.
    let pcm: Data
    /// Root mean square level, 0...1.
    let rms: Float

    init(index: UInt32, samples: [Int16]) {
        self.index = index
        pcm = samples.withUnsafeBufferPointer { buffer in
            var data = Data(capacity: buffer.count * 2)
            for sample in buffer {
                withUnsafeBytes(of: sample.littleEndian) { data.append(contentsOf: $0) }
            }
            return data
        }
        rms = Self.rms(samples)
    }

    static func rms(_ samples: [Int16]) -> Float {
        guard !samples.isEmpty else { return 0 }
        var sum: Float = 0
        for sample in samples {
            let value = Float(sample) / Float(Int16.max)
            sum += value * value
        }
        return (sum / Float(samples.count)).squareRoot()
    }
}
