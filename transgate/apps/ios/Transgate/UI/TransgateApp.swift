import SwiftUI

/// Composition root: the only place that knows the concrete adapters (D8 Keychain, D7
/// authentication session, AVFoundation, UIKit). Everything else renders core's views.
@main
struct TransgateApp: App {
    @State private var composition = Composition()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            Group {
                if let model = composition.model {
                    RootView(model: model, settings: composition.settings, player: composition.player) { composition.launcher.open($0) }
                        .task { await model.start() }
                        .onOpenURL { model.handleReturn($0) }
                        .onChange(of: scenePhase) { _, phase in model.setVisible(phase == .active) }
                } else {
                    ConfigErrorScreen()
                }
            }
        }
    }
}

/// Builds the app's objects once. A missing build setting is a configuration error shown
/// as such; the app never starts against a guessed address.
@MainActor
final class Composition {
    let settings: AppSettings
    let model: AppModel?
    let player = AVPlayerControl()
    let launcher: SystemLauncher
    private let relay: ReturnRelay

    init() {
        let relay = ReturnRelay()
        self.relay = relay
        launcher = SystemLauncher(window: { MainActor.assumeIsolated { Presenters.window() } }, returned: { relay.fire($0) })
        guard let loaded = AppSettings.fromBundle() else {
            settings = AppSettings(apiUrl: "", authUrl: "", authKey: nil, siteUrl: "")
            model = nil
            return
        }
        settings = loaded
        let files = HeldFiles(reader: AVMediaDurationReader())
        let ports = TransgateNativePorts(
            vault: KeychainSecretStore(),
            files: files,
            player: player,
            output: UIOutputSink(presenter: { MainActor.assumeIsolated { Presenters.top() } }),
            launcher: launcher,
            assets: BundleAssets(),
            settings: loaded
        )
        let built = AppModel(settings: loaded, ports: ports, files: files, userAgent: "Transgate-iOS/\(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0")",
                             width: UInt32(UIScreen.main.bounds.width))
        model = built
        relay.handler = { url in Task { @MainActor in built.handleReturn(url) } }
    }
}

/// Lets the authentication session (created before the model exists) hand its return URL to the model.
final class ReturnRelay: @unchecked Sendable {
    private let lock = NSLock()
    private var current: (@Sendable (URL) -> Void)?
    var handler: (@Sendable (URL) -> Void)? {
        get { lock.withLock { current } }
        set { lock.withLock { current = newValue } }
    }
    func fire(_ url: URL) { handler?(url) }
}

struct RootView: View {
    let model: AppModel
    let settings: AppSettings
    let player: AVPlayerControl
    let openURL: (URL) -> Void

    var body: some View {
        let copy = model.copy
        GeometryReader { proxy in
            Group {
                if let failure = model.startError {
                    // A technical start failure (the driver thread did not start), not product copy.
                    Text(failure).padding().frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    switch model.view {
                    case .restoring, .checkoutConfirm:
                        ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).accessibilityLabel(copy.text(Labels.loading))
                    case .signedOut(let vm):
                        if vm.route == .newPassword { NewPasswordScreen(vm: vm.newPassword, copy: copy, send: model.send) }
                        else { AuthScreen(vm: vm.auth, copy: copy, send: model.send) }
                    case .signedIn(let vm):
                        SignedInFrame(vm: vm, model: model, settings: settings, player: player, openURL: openURL)
                    case .erased(_, let message):
                        Text(copy.text(message)).multilineTextAlignment(.center).padding().frame(maxWidth: .infinity, maxHeight: .infinity)
                    }
                }
            }
            .screenBackground()
            .onChange(of: proxy.size.width, initial: true) { _, width in model.setWidth(UInt32(max(width, 0))) }
        }
    }
}

struct ConfigErrorScreen: View {
    var body: some View {
        // The build settings are missing, so no host exists; the strings are still the embedded catalogs'.
        Text(Copy(locale: "en").text(Labels.configUnavailable))
            .padding().frame(maxWidth: .infinity, maxHeight: .infinity).screenBackground()
    }
}
