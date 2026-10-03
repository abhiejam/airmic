import Foundation
import Network

/// Sends datagrams to a fixed host and port.
final class UDPSender: Sendable {
    private let connection: NWConnection
    private let queue = DispatchQueue(label: "io.airmic.udp")

    init?(host: String, port: UInt16) {
        guard let port = NWEndpoint.Port(rawValue: port) else { return nil }
        let parameters = NWParameters.udp
        parameters.serviceClass = .interactiveVoice
        connection = NWConnection(host: NWEndpoint.Host(host), port: port, using: parameters)
    }

    func start(onStateChange: (@Sendable (NWConnection.State) -> Void)? = nil) {
        connection.stateUpdateHandler = onStateChange
        connection.start(queue: queue)
    }

    func send(_ data: Data) {
        connection.send(content: data, completion: .idempotent)
    }

    func cancel() {
        connection.cancel()
    }
}
