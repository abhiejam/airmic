import Testing
@testable import AirMic

struct FrameAccumulatorTests {
    @Test func emitsExactFramesAndKeepsRemainder() {
        var accumulator = FrameAccumulator()
        var frames: [[Int16]] = []

        accumulator.append((0..<1000).map { Int16($0) }) { frames.append($0) }
        #expect(frames.count == 2)
        #expect(frames.allSatisfy { $0.count == 480 })
        #expect(frames[1].first == 480)

        accumulator.append((1000..<1440).map { Int16($0) }) { frames.append($0) }
        #expect(frames.count == 3)
        #expect(frames[2] == (960..<1440).map { Int16($0) })
    }

    @Test func smallChunksAddUpToOneFrame() {
        var accumulator = FrameAccumulator()
        var count = 0
        for _ in 0..<48 {
            accumulator.append([Int16](repeating: 1, count: 10)) { _ in count += 1 }
        }
        #expect(count == 1)
    }

    @Test func frameIsLittleEndianAnd960Bytes() {
        let frame = AudioFrame(index: 0, samples: [0x0102] + [Int16](repeating: 0, count: 479))
        #expect(frame.pcm.count == 960)
        #expect(frame.pcm[0] == 0x02)
        #expect(frame.pcm[1] == 0x01)
    }

    @Test func rmsOfFullScaleSquareIsOne() {
        #expect(AudioFrame.rms([Int16.max, -Int16.max]) == 1)
        #expect(AudioFrame.rms([0, 0]) == 0)
    }
}
