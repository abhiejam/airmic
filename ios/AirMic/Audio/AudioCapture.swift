import AVFAudio
import UIKit

/// Captures the microphone and emits 10 ms frames of 48 kHz mono Int16 PCM.
///
/// Uses an `AVAudioSinkNode` rather than an input tap: tap buffers are 100–400 ms,
/// the sink node gets every IO cycle (about 5 ms here).
///
/// Survives interruptions (calls, Siri) and route changes (AirPods): the engine is rebuilt
/// and capture resumes on its own.
@MainActor
final class AudioCapture {
    enum CaptureError: Error {
        case permissionDenied
        case noInput
        case unsupportedFormat
    }

    enum Interruption {
        case began
        case ended
    }

    /// Voice mode turns on Apple's voice processing (echo cancellation, noise suppression).
    /// Raw mode uses `.measurement` for an unprocessed signal.
    var voiceProcessing = true
    var onInterruption: ((Interruption) -> Void)?

    private var engine = AVAudioEngine()
    private var processor: FrameProcessor?
    private var onFrame: (@Sendable (AudioFrame) -> Void)?
    private var observers: [NSObjectProtocol] = []
    private var isInterrupted = false

    var isRunning: Bool { onFrame != nil }

    static func requestPermission() async -> Bool {
        await AVAudioApplication.requestRecordPermission()
    }

    /// `onFrame` is called on the processor thread, about 100 times a second.
    func start(onFrame: @escaping @Sendable (AudioFrame) -> Void) throws {
        guard self.onFrame == nil else { return }
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(
            .playAndRecord, mode: voiceProcessing ? .voiceChat : .measurement, options: [.defaultToSpeaker])
        try session.setPreferredSampleRate(48_000)
        try session.setPreferredIOBufferDuration(0.005)
        try session.setActive(true)
        try startEngine(onFrame: onFrame)
        self.onFrame = onFrame
        observe()
    }

    func stop() {
        guard onFrame != nil else { return }
        onFrame = nil
        isInterrupted = false
        observers.forEach(NotificationCenter.default.removeObserver)
        observers = []
        stopEngine()
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    // MARK: - Engine

    private func startEngine(onFrame: @escaping @Sendable (AudioFrame) -> Void) throws {
        let engine = AVAudioEngine()
        let input = engine.inputNode
        try input.setVoiceProcessingEnabled(voiceProcessing)
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else { throw CaptureError.noInput }

        let queue = SampleQueue(capacity: Int(format.sampleRate))
        let processor = try FrameProcessor(inputSampleRate: format.sampleRate, queue: queue, onFrame: onFrame)
        let sink = Self.makeSink(writingTo: queue)
        engine.attach(sink)
        engine.connect(input, to: sink, format: format)
        engine.prepare()
        try engine.start()
        processor.start()
        self.engine = engine
        self.processor = processor
    }

    private func stopEngine() {
        engine.stop()
        processor?.stop()
        processor = nil
    }

    /// Rebuilds the engine, e.g. after AirPods connect and the input format changes.
    private func restartEngine() {
        guard let onFrame, !isInterrupted else { return }
        stopEngine()
        do {
            try AVAudioSession.sharedInstance().setActive(true)
            try startEngine(onFrame: onFrame)
        } catch {
            // Try again when the app is next active.
            isInterrupted = true
        }
    }

    /// Built outside the main actor: a closure written in a `@MainActor` method is main actor
    /// isolated, and Swift 6 traps when the realtime thread calls it.
    private nonisolated static func makeSink(writingTo queue: SampleQueue) -> AVAudioSinkNode {
        // Realtime thread: copy channel 0 into the queue, nothing else.
        AVAudioSinkNode { _, frameCount, bufferList in
            let buffers = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: bufferList))
            if let data = buffers.first?.mData {
                queue.write(data.assumingMemoryBound(to: Float.self), count: Int(frameCount))
            }
            return noErr
        }
    }

    // MARK: - Interruptions and route changes

    private func observe() {
        let center = NotificationCenter.default
        observers = [
            center.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] note in
                let type = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt)
                    .flatMap(AVAudioSession.InterruptionType.init)
                MainActor.assumeIsolated { self?.handleInterruption(type) }
            },
            center.addObserver(forName: .AVAudioEngineConfigurationChange, object: nil, queue: .main) { [weak self] note in
                let source = note.object.map { ObjectIdentifier($0 as AnyObject) }
                MainActor.assumeIsolated { self?.engineConfigurationChanged(source: source) }
            },
            center.addObserver(forName: AVAudioSession.mediaServicesWereResetNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.restartEngine() }
            },
            // An interruption can end while the app is suspended without an `ended` note.
            center.addObserver(forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.resumeIfInterrupted() }
            },
        ]
    }

    private func engineConfigurationChanged(source: ObjectIdentifier?) {
        // Only for our current engine, and only once it has actually stopped (it does on a real change).
        guard source == ObjectIdentifier(engine), !engine.isRunning else { return }
        restartEngine()
    }

    private func handleInterruption(_ type: AVAudioSession.InterruptionType?) {
        switch type {
        case .began:
            isInterrupted = true
            stopEngine()
            onInterruption?(.began)
        case .ended:
            resumeIfInterrupted()
        default:
            break
        }
    }

    private func resumeIfInterrupted() {
        guard isInterrupted, onFrame != nil else { return }
        isInterrupted = false
        restartEngine()
        if !isInterrupted { onInterruption?(.ended) }
    }
}
