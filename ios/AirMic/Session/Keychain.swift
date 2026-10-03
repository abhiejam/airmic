import Foundation
import Security

/// Pairing tokens, one per computer, in the iOS Keychain.
enum PairingTokens {
    private static let service = "io.airmic.pairing"

    static func token(for computerID: String) -> String? {
        var query = baseQuery(computerID)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data
        else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static func save(_ token: String, for computerID: String) {
        delete(for: computerID)
        var query = baseQuery(computerID)
        query[kSecValueData as String] = Data(token.utf8)
        // Reconnecting with the screen locked needs the token.
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(query as CFDictionary, nil)
    }

    static func delete(for computerID: String) {
        SecItemDelete(baseQuery(computerID) as CFDictionary)
    }

    private static func baseQuery(_ computerID: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: computerID,
        ]
    }
}
