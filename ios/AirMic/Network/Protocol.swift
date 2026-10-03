import Foundation

// Wire protocol v1: docs/protocol.md. Test vectors: docs/protocol/vectors.json.

enum AirMicProtocol {
    static let version = 1
    static let controlPort: UInt16 = 47800
    static let sampleRate = 48_000
    static let maxLineLength = 64 * 1024
}

/// 16 byte audio packet header, big endian.
struct AudioHeader: Equatable, Sendable {
    static let size = 16
    static let version: UInt8 = 1

    var muted: Bool
    /// 0 = PCM.
    var codec: UInt8 = 0
    var sessionID: UInt32
    var sequence: UInt32
    var timestamp: UInt32

    enum DecodeError: Error, Equatable {
        case tooShort
        case badMagic
        case badVersion
    }

    init(muted: Bool, codec: UInt8 = 0, sessionID: UInt32, sequence: UInt32, timestamp: UInt32) {
        self.muted = muted
        self.codec = codec
        self.sessionID = sessionID
        self.sequence = sequence
        self.timestamp = timestamp
    }

    init(decoding data: Data) throws(DecodeError) {
        let bytes = [UInt8](data.prefix(Self.size))
        guard bytes.count == Self.size else { throw .tooShort }
        guard bytes[0] == 0x41, bytes[1] == 0x4D else { throw .badMagic }
        guard bytes[2] == Self.version else { throw .badVersion }
        func uint32(at offset: Int) -> UInt32 {
            bytes[offset..<offset + 4].reduce(0) { $0 << 8 | UInt32($1) }
        }
        muted = bytes[3] & 0x01 != 0
        codec = (bytes[3] >> 1) & 0x07
        sessionID = uint32(at: 4)
        sequence = uint32(at: 8)
        timestamp = uint32(at: 12)
    }

    func encoded() -> Data {
        var data = Data(capacity: Self.size + 960)
        data.append(contentsOf: [0x41, 0x4D, Self.version, (muted ? 0x01 : 0) | (codec & 0x07) << 1])
        for value in [sessionID, sequence, timestamp] {
            withUnsafeBytes(of: value.bigEndian) { data.append(contentsOf: $0) }
        }
        return data
    }
}

/// Control channel messages: one JSON object per line, flat fields next to `type`.
enum ControlMessage: Equatable, Sendable {
    case hello(v: Int, phoneID: String, phoneName: String)
    case pairRequired
    case pair(code: String)
    case paired(token: String)
    case auth(token: String)
    case ready(sessionID: UInt32, udpPort: UInt16, sampleRate: Int)
    case mute(on: Bool)
    case stats(lossPct: Double, jitterMs: Double, latencyMs: Double)
    case transcript(text: String, isFinal: Bool)
    case ping
    case pong
    case bye
    case error(code: String, message: String)

    struct MissingField: Error {
        let type: String
    }

    /// Returns nil for an unknown `type` (ignored for forward compatibility).
    static func decode(_ json: Data) throws -> ControlMessage? {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let wire = try decoder.decode(Wire.self, from: json)
        func need<T>(_ value: T?) throws -> T {
            guard let value else { throw MissingField(type: wire.type) }
            return value
        }
        switch wire.type {
        case "hello": return .hello(v: try need(wire.v), phoneID: try need(wire.phoneId), phoneName: try need(wire.phoneName))
        case "pair_required": return .pairRequired
        case "pair": return .pair(code: try need(wire.code))
        case "paired": return .paired(token: try need(wire.token))
        case "auth": return .auth(token: try need(wire.token))
        case "ready":
            return .ready(sessionID: try need(wire.sessionId), udpPort: try need(wire.udpPort), sampleRate: try need(wire.sampleRate))
        case "mute": return .mute(on: try need(wire.on))
        case "stats":
            return .stats(lossPct: try need(wire.lossPct), jitterMs: try need(wire.jitterMs), latencyMs: try need(wire.latencyMs))
        case "transcript": return .transcript(text: try need(wire.text), isFinal: try need(wire.final))
        case "ping": return .ping
        case "pong": return .pong
        case "bye": return .bye
        case "error": return .error(code: try need(wire.code), message: wire.message ?? "")
        default: return nil
        }
    }

    /// The JSON object, without the trailing newline.
    func encoded() -> Data {
        var wire: Wire
        switch self {
        case let .hello(v, phoneID, phoneName):
            wire = Wire(type: "hello")
            wire.v = v
            wire.phoneId = phoneID
            wire.phoneName = phoneName
        case .pairRequired:
            wire = Wire(type: "pair_required")
        case let .pair(code):
            wire = Wire(type: "pair")
            wire.code = code
        case let .paired(token):
            wire = Wire(type: "paired")
            wire.token = token
        case let .auth(token):
            wire = Wire(type: "auth")
            wire.token = token
        case let .ready(sessionID, udpPort, sampleRate):
            wire = Wire(type: "ready")
            wire.sessionId = sessionID
            wire.udpPort = udpPort
            wire.sampleRate = sampleRate
        case let .mute(on):
            wire = Wire(type: "mute")
            wire.on = on
        case let .stats(lossPct, jitterMs, latencyMs):
            wire = Wire(type: "stats")
            wire.lossPct = lossPct
            wire.jitterMs = jitterMs
            wire.latencyMs = latencyMs
        case let .transcript(text, isFinal):
            wire = Wire(type: "transcript")
            wire.text = text
            wire.final = isFinal
        case .ping:
            wire = Wire(type: "ping")
        case .pong:
            wire = Wire(type: "pong")
        case .bye:
            wire = Wire(type: "bye")
        case let .error(code, message):
            wire = Wire(type: "error")
            wire.code = code
            wire.message = message
        }
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        encoder.outputFormatting = .sortedKeys
        // Wire only holds strings, numbers and bools, so encoding cannot fail.
        return (try? encoder.encode(wire)) ?? Data()
    }

    /// Every field any message can carry; unknown fields are ignored by Codable.
    private struct Wire: Codable {
        var type: String
        var v: Int?
        var phoneId: String?
        var phoneName: String?
        var code: String?
        var token: String?
        var sessionId: UInt32?
        var udpPort: UInt16?
        var sampleRate: Int?
        var on: Bool?
        var lossPct: Double?
        var jitterMs: Double?
        var latencyMs: Double?
        var text: String?
        var final: Bool?
        var message: String?

        init(type: String) {
            self.type = type
        }
    }
}

/// Splits a TCP byte stream into lines.
struct LineBuffer {
    struct LineTooLong: Error {}

    private var pending = Data()

    /// Returns the complete lines (without `\n`, blank lines skipped) in order.
    mutating func append(_ data: Data) throws(LineTooLong) -> [Data] {
        pending.append(data)
        var lines: [Data] = []
        while let newline = pending.firstIndex(of: 0x0A) {
            let line = pending[pending.startIndex..<newline]
            if !line.isEmpty { lines.append(Data(line)) }
            pending = Data(pending[pending.index(after: newline)...])
        }
        if pending.count > AirMicProtocol.maxLineLength { throw LineTooLong() }
        return lines
    }
}

/// Turns 10 ms frames into audio packets for one session (docs/protocol.md §3.2).
struct Packetizer {
    /// While muted, one header-only packet per this many frames: 10 per second.
    static let mutedFrameInterval = 10

    let sessionID: UInt32
    private(set) var sequence: UInt32 = 0
    /// Sample index of the next frame; advances while muted too.
    private(set) var timestamp: UInt32 = 0
    private var mutedFrames = 0

    init(sessionID: UInt32) {
        self.sessionID = sessionID
    }

    /// Call once per captured frame. Returns the packet to send, or nil for a muted frame that is skipped.
    mutating func packet(pcm: Data, muted: Bool) -> Data? {
        defer { timestamp &+= UInt32(FrameAccumulator.samplesPerFrame) }
        if muted {
            defer { mutedFrames += 1 }
            guard mutedFrames % Self.mutedFrameInterval == 0 else { return nil }
            return emit(muted: true, payload: nil)
        }
        mutedFrames = 0
        return emit(muted: false, payload: pcm)
    }

    private mutating func emit(muted: Bool, payload: Data?) -> Data {
        var data = AudioHeader(muted: muted, sessionID: sessionID, sequence: sequence, timestamp: timestamp).encoded()
        if let payload { data.append(payload) }
        sequence &+= 1
        return data
    }
}
