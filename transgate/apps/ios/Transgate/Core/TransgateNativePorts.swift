import Foundation

// The device capabilities core reaches through `NativePorts`. Each one is a small
// protocol so this file, and everything that does not draw, builds on Linux; UIKit,
// AVFoundation and the authentication session implement them in `Platform/`.

/// The mounted audio player. Core decides when to pause, resume or seek (PLY rules).
protocol PlayerControl: Sendable {
    /// Pauses; whether it was playing before.
    func pause() -> Bool
    func resume()
    func seek(seconds: Double)
}

/// Output of the signed-in app: clipboard, files, preview and the share sheet.
/// Called on a blocking thread; implementations hop to the main thread themselves.
protocol OutputSink: Sendable {
    func copy(text: String, html: String?) -> Bool
    func save(bytes: Data, filename: String, mime: String) -> Bool
    func preview(bytes: Data, filename: String, mime: String) -> Bool
    func share(bytes: Data, filename: String, mime: String) -> ShareResult
}

/// Leaving the app (D7): a web authentication session for the GoTrue redirect, or
/// the system browser for everything else (web handoffs). Never blocks.
protocol ExternalLauncher: Sendable {
    func authenticate(_ url: URL, callbackScheme: String)
    func open(_ url: URL)
}

/// The bundled document assets by file name (D10: all fonts, the invoice logo).
protocol AssetSource: Sendable {
    func asset(named name: String) -> Data?
}

/// A directory of assets; used by Linux runs and by the app (resources copied flat
/// into the bundle's directory).
struct DirectoryAssets: AssetSource {
    let directory: URL

    func asset(named name: String) -> Data? {
        // Core asks for fixed file names; refuse anything that is not one.
        guard !name.isEmpty, !name.contains("/"), name != ".", name != ".." else { return nil }
        return try? Data(contentsOf: directory.appendingPathComponent(name))
    }
}

/// `NativePorts` for iOS (and the Linux smoke): thin forwarding, no decisions.
final class TransgateNativePorts: NativePorts, @unchecked Sendable {
    private let vault: SecretStore
    private let files: HeldFiles
    private let player: PlayerControl
    private let output: OutputSink
    private let launcher: ExternalLauncher
    private let assets: AssetSource
    private let authBase: String
    private let callbackScheme: String
    private let lock = NSLock()
    private var path = "/home"

    init(vault: SecretStore, files: HeldFiles, player: PlayerControl, output: OutputSink, launcher: ExternalLauncher, assets: AssetSource, settings: AppSettings) {
        self.vault = vault
        self.files = files
        self.player = player
        self.output = output
        self.launcher = launcher
        self.assets = assets
        self.authBase = settings.authUrl
        self.callbackScheme = settings.callbackScheme
    }

    /// The location core last asked for; a native app has no address bar, so this is
    /// only for diagnostics and tests.
    var currentPath: String { lock.withLock { path } }

    func storageGet(key: String) -> String? { vault.get(key) }
    func storageSet(key: String, value: String) { vault.set(key, value) }
    func storageRemove(key: String) { vault.remove(key) }

    func navigate(path: String, replace: Bool) {
        lock.withLock { self.path = path }
    }

    func leave(url: String) {
        guard let target = URL(string: url) else { return }
        // The GoTrue redirect (Google) returns through the custom scheme; any other
        // address is a page of the web app.
        if url.hasPrefix(authBase) {
            launcher.authenticate(target, callbackScheme: callbackScheme)
        } else {
            launcher.open(target)
        }
    }

    func calendar() -> CalendarOffsets {
        let calendar = Calendar.current
        let now = Date()
        let then = calendar.date(byAdding: .month, value: -1, to: now) ?? now
        return CalendarOffsets(
            nowMinutes: Int32(calendar.timeZone.secondsFromGMT(for: now) / 60),
            thenMinutes: Int32(calendar.timeZone.secondsFromGMT(for: then) / 60)
        )
    }

    func measure(file: UInt64) -> Double? { files.measure(file) }

    func upload(file: UInt64, url: String, progress: ProgressSink) -> Bool {
        files.upload(file, to: url) { percent in progress.report(percent: percent) }
    }

    func release(file: UInt64) { files.release(file) }

    func pausePlayer() -> Bool { player.pause() }
    func resumePlayer() { player.resume() }
    func seekPlayer(seconds: Double) { player.seek(seconds: seconds) }

    func copy(text: String, html: String?) -> Bool { output.copy(text: text, html: html) }
    func save(bytes: Data, filename: String, mime: String) -> Bool { output.save(bytes: bytes, filename: filename, mime: mime) }
    func preview(bytes: Data, filename: String, mime: String) -> Bool { output.preview(bytes: bytes, filename: filename, mime: mime) }
    func share(bytes: Data, filename: String, mime: String) -> ShareResult { output.share(bytes: bytes, filename: filename, mime: mime) }

    func asset(name: String) -> Data? { assets.asset(named: name) }
}
