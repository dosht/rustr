import Foundation
#if canImport(Security)
import Security
#endif

/// Secure storage behind the session vault port (D8). Core decides what is
/// stored (the session, the PKCE verifier) and under which key; the shell only
/// keeps strings. `NativePorts.storage_*` are plain synchronous calls.
protocol SecretStore: Sendable {
    func get(_ key: String) -> String?
    func set(_ key: String, _ value: String)
    func remove(_ key: String)
}

/// Process-local store for tests and previews.
final class MemorySecretStore: SecretStore, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: String] = [:]

    func get(_ key: String) -> String? { lock.withLock { values[key] } }
    func set(_ key: String, _ value: String) { lock.withLock { values[key] = value } }
    func remove(_ key: String) { lock.withLock { _ = values.removeValue(forKey: key) } }
}

/// A file-backed store for Linux runs (smoke tests); owner read/write only.
/// Not used by the app: on a device the Keychain is the only vault.
final class FileSecretStore: SecretStore, @unchecked Sendable {
    private let lock = NSLock()
    private let url: URL

    init(url: URL) { self.url = url }

    private func read() -> [String: String] {
        guard let data = try? Data(contentsOf: url),
              let values = try? JSONDecoder().decode([String: String].self, from: data) else { return [:] }
        return values
    }

    private func write(_ values: [String: String]) {
        guard let data = try? JSONEncoder().encode(values) else { return }
        try? data.write(to: url, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }

    func get(_ key: String) -> String? { lock.withLock { read()[key] } }
    func set(_ key: String, _ value: String) { lock.withLock { var values = read(); values[key] = value; write(values) } }
    func remove(_ key: String) { lock.withLock { var values = read(); values.removeValue(forKey: key); write(values) } }
}

#if canImport(Security)
/// The Keychain, this device only (D8): items are readable after the first
/// unlock (background refresh works) and never migrate to another device or into
/// a backup.
final class KeychainSecretStore: SecretStore, @unchecked Sendable {
    private let service: String

    init(service: String = "dev.transgate.ios.session") { self.service = service }

    private func query(_ key: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: key]
    }

    func get(_ key: String) -> String? {
        var request = query(key)
        request[kSecReturnData as String] = true
        request[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        guard SecItemCopyMatching(request as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    func set(_ key: String, _ value: String) {
        remove(key)
        var item = query(key)
        item[kSecValueData as String] = Data(value.utf8)
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    func remove(_ key: String) {
        SecItemDelete(query(key) as CFDictionary)
    }
}
#endif
