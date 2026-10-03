import AVFAudio
import Foundation
import Synchronization

/// Runs on its own thread: drains the sample queue, converts to 48 kHz mono Int16,
/// and emits 10 ms frames.
final class FrameProcessor: @unchecked Sendable {
    static let outputFormat = AVAudioFormat(
        commonFormat: .pcmFormatInt16, sampleRate: 48_000, channels: 1, interleaved: true)!

    private let queue: SampleQueue
    private let onFrame: @Sendable (AudioFrame) -> Void
    private let stopped = Atomic<Bool>(false)

    // Touched only on the processor thread after start().
    private let converter: AVAudioConverter
    private let inputBuffer: AVAudioPCMBuffer
    private let outputBuffer: AVAudioPCMBuffer
    private var accumulator = FrameAccumulator()
    private var frameIndex: UInt32 = 0

    init(inputSampleRate: Double, queue: SampleQueue, onFrame: @escaping @Sendable (AudioFrame) -> Void) throws {
        guard
            let inputFormat = AVAudioFormat(standardFormatWithSampleRate: inputSampleRate, channels: 1),
            let converter = AVAudioConverter(from: inputFormat, to: Self.outputFormat)
        else { throw AudioCapture.CaptureError.unsupportedFormat }
        let inputCapacity: AVAudioFrameCount = 4096
        let outputCapacity = AVAudioFrameCount((Double(inputCapacity) * 48_000 / inputSampleRate).rounded(.up)) + 256
        guard
            let inputBuffer = AVAudioPCMBuffer(pcmFormat: inputFormat, frameCapacity: inputCapacity),
            let outputBuffer = AVAudioPCMBuffer(pcmFormat: Self.outputFormat, frameCapacity: outputCapacity)
        else { throw AudioCapture.CaptureError.unsupportedFormat }
        self.queue = queue
        self.onFrame = onFrame
        self.converter = converter
        self.inputBuffer = inputBuffer
        self.outputBuffer = outputBuffer
    }

    func start() {
        let thread = Thread { [self] in run() }
        thread.name = "AirMic frame processor"
        thread.qualityOfService = .userInteractive
        thread.start()
    }

    func stop() {
        stopped.store(true, ordering: .relaxed)
        queue.available.signal()
    }

    private func run() {
        while !stopped.load(ordering: .relaxed) {
            _ = queue.available.wait(timeout: .now() + .milliseconds(50))
            drain()
        }
    }

    private func drain() {
        guard let destination = inputBuffer.floatChannelData?[0] else { return }
        while !stopped.load(ordering: .relaxed) {
            let count = queue.read(into: destination, max: Int(inputBuffer.frameCapacity))
            if count == 0 { return }
            inputBuffer.frameLength = AVAudioFrameCount(count)
            convertAndEmit()
        }
    }

    private func convertAndEmit() {
        let input = inputBuffer
        let supplied = InputFlag()
        // Loop until the converter has consumed this chunk and handed back all output.
        while true {
            outputBuffer.frameLength = 0
            var error: NSError?
            let status = converter.convert(to: outputBuffer, error: &error) { _, inputStatus in
                if supplied.isSet {
                    inputStatus.pointee = .noDataNow
                    return nil
                }
                supplied.isSet = true
                inputStatus.pointee = .haveData
                return input
            }
            if status == .error { return }
            if let samples = outputBuffer.int16ChannelData?[0], outputBuffer.frameLength > 0 {
                let buffer = UnsafeBufferPointer(start: samples, count: Int(outputBuffer.frameLength))
                accumulator.append(buffer) { frame in
                    onFrame(AudioFrame(index: frameIndex, samples: frame))
                    frameIndex &+= 1
                }
            }
            if status != .haveData { return }
        }
    }
}

/// Marks that the converter's input block has already handed over the current chunk.
/// Used only on the processor thread.
private final class InputFlag: @unchecked Sendable {
    var isSet = false
}
