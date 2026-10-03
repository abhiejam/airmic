import Foundation

/// The QR code payload: `airmic://pair?host=192.168.20.42&port=47800&id=<uuid>&code=0427`.
struct PairingLink: Equatable {
    var host: String
    var port: UInt16
    var computerID: String
    var code: String

    init?(_ string: String) {
        guard let url = URL(string: string) else { return nil }
        self.init(url: url)
    }

    init?(url: URL) {
        guard url.scheme == "airmic", url.host() == "pair",
              let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems
        else { return nil }
        func value(_ name: String) -> String? {
            items.first { $0.name == name }?.value.flatMap { $0.isEmpty ? nil : $0 }
        }
        guard let host = value("host"), let id = value("id"), let code = value("code"),
              code.count == 4, code.allSatisfy(\.isASCII), code.allSatisfy(\.isNumber)
        else { return nil }
        let port = value("port").flatMap(UInt16.init) ?? AirMicProtocol.controlPort
        guard port > 0 else { return nil }
        self.host = host
        self.port = port
        computerID = id
        self.code = code
    }

    var computer: Computer {
        Computer(id: computerID, name: host, host: host, port: port, serviceName: nil)
    }
}
