import Foundation
import Network
import Observation

/// Browses for computers running AirMic (`_airmic._tcp`, docs/protocol.md §1).
@MainActor
@Observable
final class Discovery {
    enum State: Equatable {
        case idle
        case searching
        /// The user turned off Local Network access for AirMic.
        case denied
        case failed(String)
    }

    nonisolated static let serviceType = "_airmic._tcp"

    private(set) var state: State = .idle
    private(set) var computers: [Computer] = []
    private var browser: NWBrowser?

    func start() {
        guard browser == nil else { return }
        let browser = NWBrowser(for: .bonjourWithTXTRecord(type: Self.serviceType, domain: nil), using: .tcp)
        browser.stateUpdateHandler = { [weak self] newState in
            let state = Self.state(for: newState)
            MainActor.assumeIsolated { self?.state = state }
        }
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            let computers = Self.computers(from: results)
            MainActor.assumeIsolated { self?.computers = computers }
        }
        browser.start(queue: .main)
        self.browser = browser
    }

    func stop() {
        browser?.cancel()
        browser = nil
        state = .idle
    }

    private nonisolated static func state(for state: NWBrowser.State) -> State {
        switch state {
        case .ready, .setup: return .searching
        case .waiting(let error), .failed(let error):
            // kDNSServiceErr_PolicyDenied: Local Network permission is off.
            if case .dns(let code) = error, code == -65570 { return .denied }
            return .failed(error.localizedDescription)
        case .cancelled: return .idle
        @unknown default: return .searching
        }
    }

    nonisolated static func computers(from results: Set<NWBrowser.Result>) -> [Computer] {
        var byID: [String: Computer] = [:]
        for result in results {
            guard case let .service(serviceName, _, _, _) = result.endpoint else { continue }
            var txt: [String: String] = [:]
            if case let .bonjour(record) = result.metadata { txt = record.dictionary }
            if let version = txt["v"], version != String(AirMicProtocol.version) { continue }
            let id = txt["id"].flatMap { $0.isEmpty ? nil : $0 } ?? "service:\(serviceName)"
            // One computer can show up once per network interface.
            byID[id] = Computer(
                id: id, name: txt["name"].flatMap { $0.isEmpty ? nil : $0 } ?? serviceName,
                host: nil, port: AirMicProtocol.controlPort, serviceName: serviceName)
        }
        return byID.values.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }
}
