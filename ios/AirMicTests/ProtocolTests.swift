import Foundation
import Testing
@testable import AirMic

/// The shared vectors in docs/protocol/vectors.json, the same file the Rust side tests against.
/// Simulator tests can read the repo directly, so there is no copy to drift.
private struct Vectors: Decodable {
    struct Header: Decodable {
        struct Fields: Decodable {
            let muted: Bool
            let codec: UInt8
            let session_id: UInt32
            let sequence: UInt32
            let timestamp: UInt32
        }
        let name: String
        let fields: Fields
        let hex: String
        let decode_only: Bool?
    }
    struct Invalid: Decodable {
        let name: String
        let hex: String
        let error: String
    }
    struct Codec: Decodable {
        let name: String
        let hex: String
        let codec: UInt8
    }
    struct PCM: Decodable {
        let samples: [Int16]
        let hex: String
    }
    struct Message: Decodable {
        let type: String
        let json: String
    }
    struct Tolerated: Decodable {
        let name: String
        let json: String
        let parses_as: String?
    }
    let protocol_version: Int
    let headers: [Header]
    let invalid_headers: [Invalid]
    let unsupported_codec: [Codec]
    let pcm_payload: PCM
    let messages: [Message]
    let messages_tolerated: [Tolerated]

    static func load() throws -> Vectors {
        let url = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent() // AirMicTests
            .deletingLastPathComponent() // ios
            .deletingLastPathComponent() // repo
            .appending(path: "docs/protocol/vectors.json")
        return try JSONDecoder().decode(Vectors.self, from: Data(contentsOf: url))
    }
}

private func data(hex: String) -> Data {
    var data = Data()
    var index = hex.startIndex
    while index < hex.endIndex {
        let next = hex.index(index, offsetBy: 2)
        data.append(UInt8(hex[index..<next], radix: 16)!)
        index = next
    }
    return data
}

private func hex(_ data: Data) -> String {
    data.map { String(format: "%02x", $0) }.joined()
}

/// Parsed JSON object, for comparing values rather than strings.
private func object(_ data: Data) throws -> NSDictionary {
    try #require(JSONSerialization.jsonObject(with: data) as? NSDictionary)
}

struct ProtocolVectorTests {
    fileprivate let vectors: Vectors

    init() throws {
        vectors = try Vectors.load()
    }

    @Test func versionMatches() {
        #expect(vectors.protocol_version == AirMicProtocol.version)
    }

    @Test func headersEncodeAndDecode() throws {
        for vector in vectors.headers {
            let f = vector.fields
            let header = AudioHeader(muted: f.muted, codec: f.codec, sessionID: f.session_id, sequence: f.sequence, timestamp: f.timestamp)
            if vector.decode_only != true {
                #expect(hex(header.encoded()) == vector.hex, "\(vector.name)")
            }
            #expect(try AudioHeader(decoding: data(hex: vector.hex)) == header, "\(vector.name)")
        }
    }

    @Test func invalidHeadersAreRejected() {
        let expected: [String: AudioHeader.DecodeError] = [
            "bad_magic": .badMagic, "bad_version": .badVersion, "too_short": .tooShort,
        ]
        for vector in vectors.invalid_headers {
            #expect(throws: expected[vector.error]!, "\(vector.name)") {
                try AudioHeader(decoding: data(hex: vector.hex))
            }
        }
    }

    @Test func codecBitsDecode() throws {
        for vector in vectors.unsupported_codec {
            #expect(try AudioHeader(decoding: data(hex: vector.hex)).codec == vector.codec, "\(vector.name)")
        }
    }

    @Test func pcmIsLittleEndian() {
        let frame = AudioFrame(index: 0, samples: vectors.pcm_payload.samples)
        #expect(hex(frame.pcm) == vectors.pcm_payload.hex)
    }

    @Test func messagesRoundTrip() throws {
        #expect(vectors.messages.count == 13)
        for vector in vectors.messages {
            let json = Data(vector.json.utf8)
            let message = try #require(try ControlMessage.decode(json), "\(vector.type)")
            let encoded = try object(message.encoded())
            #expect(encoded == (try object(json)), "\(vector.type)")
        }
    }

    @Test func toleratedMessages() throws {
        for vector in vectors.messages_tolerated {
            let message = try ControlMessage.decode(Data(vector.json.utf8))
            if let expected = vector.parses_as {
                #expect(message == (try ControlMessage.decode(Data(expected.utf8))), "\(vector.name)")
            } else {
                #expect(message == nil, "\(vector.name)")
            }
        }
    }
}

struct ControlMessageTests {
    @Test func missingFieldThrows() {
        #expect(throws: ControlMessage.MissingField.self) {
            try ControlMessage.decode(Data(#"{"type":"ready","session_id":1}"#.utf8))
        }
    }

    @Test func invalidJSONThrows() {
        #expect(throws: (any Error).self) {
            try ControlMessage.decode(Data("not json".utf8))
        }
    }
}

struct LineBufferTests {
    @Test func splitsAcrossChunks() throws {
        var buffer = LineBuffer()
        #expect(try buffer.append(Data(#"{"type":"pi"#.utf8)).isEmpty)
        let lines = try buffer.append(Data("ng\"}\n\n{\"type\":\"pong\"}\n{\"ty".utf8))
        #expect(lines.map { String(decoding: $0, as: UTF8.self) } == [#"{"type":"ping"}"#, #"{"type":"pong"}"#])
    }

    @Test func rejectsOverlongLine() {
        var buffer = LineBuffer()
        #expect(throws: LineBuffer.LineTooLong.self) {
            try buffer.append(Data(repeating: 0x61, count: AirMicProtocol.maxLineLength + 1))
        }
    }
}

struct PacketizerTests {
    private let pcm = Data(repeating: 0, count: 960)

    private func next(_ packetizer: inout Packetizer, muted: Bool) -> Data? {
        packetizer.packet(pcm: pcm, muted: muted)
    }

    @Test func unmutedPacketsCountUp() throws {
        var packetizer = Packetizer(sessionID: 439_041_101)
        let first = try #require(next(&packetizer, muted: false))
        let second = try #require(next(&packetizer, muted: false))
        #expect(first.count == 976)
        #expect(hex(first.prefix(16)) == "414d01001a2b3c4d0000000000000000")
        #expect(hex(second.prefix(16)) == "414d01001a2b3c4d00000001000001e0")
    }

    @Test func mutedSendsHeaderOnlyTenPerSecond() throws {
        var packetizer = Packetizer(sessionID: 1)
        _ = packetizer.packet(pcm: pcm, muted: false) // seq 0, ts 0
        // One second of muted frames.
        let muted = (0..<100).compactMap { _ in packetizer.packet(pcm: pcm, muted: true) }
        #expect(muted.count == 10)
        let headers = try muted.map { try AudioHeader(decoding: $0) }
        #expect(muted.allSatisfy { $0.count == 16 })
        #expect(headers.allSatisfy { $0.muted })
        #expect(headers.map(\.sequence) == Array(1...10))
        #expect(headers.map(\.timestamp) == (0..<10).map { 480 + UInt32($0) * 4800 })

        // Unmute: next sequence, current timestamp.
        let resumed = try AudioHeader(decoding: try #require(next(&packetizer, muted: false)))
        #expect(!resumed.muted)
        #expect(resumed.sequence == 11)
        #expect(resumed.timestamp == 480 * 101)
    }
}

struct ReconnectTests {
    @Test func backoffIsCappedAtTwoSeconds() {
        let delays = (0..<8).map { StreamSession.reconnectDelay(attempt: $0) }
        #expect(delays.prefix(4) == [.milliseconds(250), .milliseconds(500), .seconds(1), .seconds(2)])
        #expect(delays.dropFirst(3).allSatisfy { $0 == .seconds(2) })
    }
}

struct PairingLinkTests {
    @Test func parsesTheSpecExample() throws {
        let link = try #require(PairingLink("airmic://pair?host=192.168.20.42&port=47800&id=3f2504e0-4f89-41d3-9a0c-0305e82c3301&code=0427"))
        #expect(link.host == "192.168.20.42")
        #expect(link.port == 47800)
        #expect(link.computerID == "3f2504e0-4f89-41d3-9a0c-0305e82c3301")
        #expect(link.code == "0427")
        #expect(link.computer.id == link.computerID)
    }

    @Test func portDefaultsToControlPort() {
        #expect(PairingLink("airmic://pair?host=h&id=x&code=1234")?.port == 47800)
    }

    @Test func rejectsOtherLinks() {
        #expect(PairingLink("https://airmic.io/pair?host=h&id=x&code=1234") == nil)
        #expect(PairingLink("airmic://pair?host=h&id=x&code=12") == nil)
        #expect(PairingLink("airmic://pair?host=h&id=x&code=12a4") == nil)
        #expect(PairingLink("airmic://pair?id=x&code=1234") == nil)
    }
}
