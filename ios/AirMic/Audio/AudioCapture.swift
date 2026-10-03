import AVFAudio

/// Captures the microphone and emits 10 ms frames of 48 kHz mono Int16 PCM.
///
/// Uses an `AVAudioSinkNode` rather than an input tap: tap buffers are 100–400 ms,
/// the sink node gets every IO cycle (about 5 ms here).
@MainActor
final class AudioCapture {
    enum CaptureError: Error {
        case permissionDenied
        case noInput
        case unsupportedFormat
    }

    /// Voice mode turns on Apple's voice processing (echo cancellation, noise suppression).
    /// Raw mode uses `.measurement` for an unprocessed signal.
    var voiceProcessing = true

    private let engine = AVAudioEngine()
    private var sink: AVAudioSinkNode?
    private var processor: FrameProcessor?

    var isRunning: Bool { processor != nil }

    static func requestPermission() async -> Bool {
        await AVAudioApplication.requestRecordPermission()
    }

    /// `onFrame` is called on the processor thread, about 100 times a second.
    func start(onFrame: @escaping @Sendable (AudioFrame) -> Void) throws {
        guard processor == nil else { return }

        let session = AVAudioSession.sharedInstance()
        try session.setCategory(
            .playAndRecord, mode: voiceProcessing ? .voiceChat : .measurement, options: [.defaultToSpeaker])
        try session.setPreferredSampleRate(48_000)
        try session.setPreferredIOBufferDuration(0.005)
        try session.setActive(true)

        let input = engine.inputNode
        try input.setVoiceProcessingEnabled(voiceProcessing)
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else { throw CaptureError.noInput }

        let queue = SampleQueue(capacity: Int(format.sampleRate))
        let processor = try FrameProcessor(inputSampleRate: format.sampleRate, queue: queue, onFrame: onFrame)

        // Realtime thread: copy channel 0 into the queue, nothing else.
        let sink = AVAudioSinkNode { _, frameCount, bufferList in
            let buffers = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: bufferList))
            if let data = buffers.first?.mData {
                queue.write(data.assumingMemoryBound(to: Float.self), count: Int(frameCount))
            }
            return noErr
        }
        engine.attach(sink)
        engine.connect(input, to: sink, format: format)
        engine.prepare()
        do {
            try engine.start()
        } catch {
            engine.detach(sink)
            throw error
        }
        processor.start()
        self.sink = sink
        self.processor = processor
    }

    func stop() {
        guard let processor else { return }
        engine.stop()
        if let sink { engine.detach(sink) }
        processor.stop()
        self.processor = nil
        sink = nil
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }
}
