import AuthenticationServices
import AVFoundation
import Foundation
import UIKit

// The Apple implementations of the Core/ protocols. They are mechanics only:
// what to play, copy, share or open is decided in Rust. Not part of the Linux build
// (needs Xcode, see README.md).

/// Reads a file's duration with AVFoundation; `nil` when it is not playable media.
struct AVMediaDurationReader: MediaDurationReader {
    func seconds(of file: URL) -> Double? {
        let asset = AVURLAsset(url: file)
        let semaphore = DispatchSemaphore(value: 0)
        let box = DurationBox()
        Task.detached {
            if let duration = try? await asset.load(.duration), duration.isNumeric, duration.seconds.isFinite, duration.seconds > 0 {
                box.seconds = duration.seconds
            }
            semaphore.signal()
        }
        // Core calls this on a blocking thread, never the main thread.
        semaphore.wait()
        return box.seconds
    }
}

private final class DurationBox: @unchecked Sendable {
    var seconds: Double?
}

/// The one mounted player (core's `PlayerControl`): an AVPlayer over the URL the transcript
/// screen mounted. Core decides when to pause, resume and seek; the shell reports the player's
/// clock and play state back (`PlayerSnapshot`), because only the player knows them.
final class AVPlayerControl: PlayerControl, @unchecked Sendable {
    private let lock = NSLock()
    private var player: AVPlayer?
    private var clock: Any?
    private var status: NSKeyValueObservation?

    /// The transcript screen mounts a source (or `nil` to unmount). Call on the main thread.
    func mount(_ url: URL?, onSnapshot: (@Sendable (PlayerSnapshot) -> Void)? = nil) {
        lock.withLock {
            unmountLocked()
            guard let url else { return }
            let next = AVPlayer(url: url)
            player = next
            clock = next.addPeriodicTimeObserver(forInterval: CMTime(seconds: 0.25, preferredTimescale: 600), queue: .main) { [weak next] time in
                guard let next else { return }
                onSnapshot?(PlayerSnapshot(time: time.seconds, playing: next.timeControlStatus != .paused))
            }
            status = next.observe(\.timeControlStatus, options: [.new]) { observed, _ in
                onSnapshot?(PlayerSnapshot(time: observed.currentTime().seconds, playing: observed.timeControlStatus != .paused))
            }
        }
    }

    private func unmountLocked() {
        if let player, let clock { player.removeTimeObserver(clock) }
        status?.invalidate()
        status = nil
        clock = nil
        player?.pause()
        player = nil
    }

    /// The play button.
    func toggle() {
        guard let player = lock.withLock({ self.player }) else { return }
        DispatchQueue.main.async { if player.timeControlStatus == .paused { player.play() } else { player.pause() } }
    }

    func pause() -> Bool {
        let player = lock.withLock { self.player }
        guard let player else { return false }
        let wasPlaying = player.timeControlStatus != .paused
        DispatchQueue.main.async { player.pause() }
        return wasPlaying
    }

    func resume() { onMain { $0.play() } }
    func seek(seconds: Double) { onMain { $0.seek(to: CMTime(seconds: seconds, preferredTimescale: 600)) } }

    private func onMain(_ work: @escaping @Sendable (AVPlayer) -> Void) {
        guard let player = lock.withLock({ self.player }) else { return }
        DispatchQueue.main.async { work(player) }
    }
}

/// Clipboard, files, preview and share, through UIKit.
final class UIOutputSink: OutputSink, @unchecked Sendable {
    private let presenter: @Sendable () -> UIViewController?

    init(presenter: @escaping @Sendable () -> UIViewController?) { self.presenter = presenter }

    func copy(text: String, html: String?) -> Bool {
        runOnMain {
            var item: [String: Any] = ["public.utf8-plain-text": text]
            if let html { item["public.html"] = html }
            UIPasteboard.general.setItems([item])
            return true
        }
    }

    func save(bytes: Data, filename: String, mime: String) -> Bool { present(bytes, filename, wait: false) != .unsupported }
    func preview(bytes: Data, filename: String, mime: String) -> Bool { present(bytes, filename, wait: false) != .unsupported }
    func share(bytes: Data, filename: String, mime: String) -> ShareResult { present(bytes, filename, wait: true) }

    /// Shows the system share sheet for a file written to a temporary location. On iOS
    /// "save" is "Save to Files" in the same sheet.
    private func present(_ bytes: Data, _ filename: String, wait: Bool) -> ShareResult {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let name = (filename as NSString).lastPathComponent
        let url = directory.appendingPathComponent(name.isEmpty ? "transcript" : name)
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try bytes.write(to: url)
        } catch {
            return .unsupported
        }
        let outcome = ShareOutcome()
        let done = DispatchSemaphore(value: 0)
        DispatchQueue.main.async { [presenter] in
            guard let top = presenter() else { outcome.result = .unsupported; return done.signal() }
            let sheet = UIActivityViewController(activityItems: [url], applicationActivities: nil)
            sheet.completionWithItemsHandler = { _, completed, _, _ in
                outcome.result = completed ? .shared : .dismissed
                try? FileManager.default.removeItem(at: directory)
                done.signal()
            }
            sheet.popoverPresentationController?.sourceView = top.view
            top.present(sheet, animated: true)
        }
        if wait { done.wait() } else { _ = done.wait(timeout: .now() + 0.5) }
        return outcome.result
    }

    private func runOnMain<T: Sendable>(_ work: @MainActor @Sendable () -> T) -> T {
        if Thread.isMainThread { return MainActor.assumeIsolated(work) }
        return DispatchQueue.main.sync { MainActor.assumeIsolated(work) }
    }
}

private final class ShareOutcome: @unchecked Sendable {
    var result: ShareResult = .shared
}

/// Leaves the app: `ASWebAuthenticationSession` for the GoTrue redirect (D7), the
/// system browser for web handoffs and the YouTube app link (D6, D11).
final class SystemLauncher: NSObject, ExternalLauncher, ASWebAuthenticationPresentationContextProviding, @unchecked Sendable {
    private let lock = NSLock()
    private var session: ASWebAuthenticationSession?
    private let window: @Sendable () -> UIWindow?
    private let returned: @Sendable (URL) -> Void

    /// `returned` receives the custom-scheme URL the session ended on (`AppModel.handleReturn`).
    init(window: @escaping @Sendable () -> UIWindow?, returned: @escaping @Sendable (URL) -> Void) {
        self.window = window
        self.returned = returned
    }

    func authenticate(_ url: URL, callbackScheme: String) {
        DispatchQueue.main.async { [self] in
            let started = ASWebAuthenticationSession(url: url, callbackURLScheme: callbackScheme) { [returned] callback, _ in
                // A cancelled session has no callback; core stays on the sign-in form.
                if let callback { returned(callback) }
            }
            started.presentationContextProvider = self
            started.prefersEphemeralWebBrowserSession = false
            lock.withLock { session = started }
            started.start()
        }
    }

    func open(_ url: URL) {
        DispatchQueue.main.async { UIApplication.shared.open(url) }
    }

    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        window() ?? ASPresentationAnchor()
    }
}

/// Bundled fonts and the invoice logo (D10): resources copied flat into the app bundle.
struct BundleAssets: AssetSource {
    var bundle: Bundle = .main

    func asset(named name: String) -> Data? {
        guard !name.contains("/"), let url = bundle.url(forResource: (name as NSString).deletingPathExtension, withExtension: (name as NSString).pathExtension) else { return nil }
        return try? Data(contentsOf: url)
    }
}

/// The window scene's top view controller, for the share sheet.
@MainActor
enum Presenters {
    static func window() -> UIWindow? {
        UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows).first { $0.isKeyWindow }
    }

    static func top() -> UIViewController? {
        var controller = window()?.rootViewController
        while let next = controller?.presentedViewController { controller = next }
        return controller
    }
}
