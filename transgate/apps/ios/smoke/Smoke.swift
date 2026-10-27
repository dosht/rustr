// Linux (and macOS) smoke for the iOS shell's non-UI half: the generated bindings,
// the native ports, the held files and PUT, the vault, the custom-scheme return and
// AppModel, against the host build of the core. SwiftUI files are not part of this
// build. Run with transgate/scripts/check-swift.sh.
//
// Environment (check-swift.sh starts a local fake host and sets the first):
//   TRANSGATE_SMOKE_HOST    base URL of the fake host: PUT /put/ok answers 200, other PUTs 500;
//                           /api/session and /api/jobs answer a signed-in owner
//   TRANSGATE_SMOKE_API / _AUTH / _KEY / _EMAIL / _PASSWORD
//                           optional: a real host and a verified user; adds a password sign-in run
import Foundation

// MARK: - Fakes

final class FakePlayer: PlayerControl, @unchecked Sendable {
    private let lock = NSLock()
    private(set) var calls: [String] = []
    func pause() -> Bool { lock.withLock { calls.append("pause") }; return true }
    func resume() { lock.withLock { calls.append("resume") } }
    func seek(seconds: Double) { lock.withLock { calls.append("seek \(seconds)") } }
}

final class FakeOutput: OutputSink, @unchecked Sendable {
    private let lock = NSLock()
    private(set) var copied: [String] = []
    private(set) var saved: [String] = []
    func copy(text: String, html: String?) -> Bool { lock.withLock { copied.append(text) }; return true }
    func save(bytes: Data, filename: String, mime: String) -> Bool { lock.withLock { saved.append("\(filename) \(mime) \(bytes.count)") }; return true }
    func preview(bytes: Data, filename: String, mime: String) -> Bool { true }
    func share(bytes: Data, filename: String, mime: String) -> ShareResult { .dismissed }
}

final class FakeLauncher: ExternalLauncher, @unchecked Sendable {
    private let lock = NSLock()
    private(set) var authenticated: [String] = []
    private(set) var opened: [String] = []
    func authenticate(_ url: URL, callbackScheme: String) { lock.withLock { authenticated.append("\(callbackScheme) \(url.absoluteString)") } }
    func open(_ url: URL) { lock.withLock { opened.append(url.absoluteString) } }
}

final class FixedDuration: MediaDurationReader, @unchecked Sendable {
    private let lock = NSLock()
    private var measured = 0
    var measurements: Int { lock.withLock { measured } }
    func seconds(of file: URL) -> Double? {
        lock.withLock { measured += 1 }
        return file.pathExtension == "mp3" ? 61.5 : nil
    }
}

// MARK: - Harness

var passed: [String] = []

func check(_ condition: Bool, _ message: @autoclosure () -> String) {
    if !condition {
        print("FAILED: \(message())")
        exit(1)
    }
}

func pass(_ name: String) { passed.append(name) }

@MainActor
func eventually(_ what: String, seconds: Double = 20, _ condition: () -> Bool) async {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if condition() { return }
        try? await Task.sleep(nanoseconds: 20_000_000)
    }
    check(false, "timed out waiting for \(what)")
}

let work: URL = {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("transgate-ios-smoke-\(getpid())")
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}()

let unreachable = AppSettings(apiUrl: "http://127.0.0.1:9/api", authUrl: "http://127.0.0.1:9/auth/v1", authKey: nil, siteUrl: "transgatesmoke://app")

struct Rig {
    var vault: SecretStore
    var launcher = FakeLauncher()
    var output = FakeOutput()
    var player = FakePlayer()
    let reader: FixedDuration
    let files: HeldFiles
    var settings: AppSettings

    init(vault: SecretStore, settings: AppSettings) {
        self.vault = vault
        self.settings = settings
        reader = FixedDuration()
        files = HeldFiles(reader: reader)
    }

    @MainActor
    func model() -> (AppModel, TransgateNativePorts) {
        let ports = TransgateNativePorts(vault: vault, files: files, player: player, output: output, launcher: launcher,
                                         assets: DirectoryAssets(directory: work), settings: settings)
        return (AppModel(settings: settings, ports: ports, files: files, userAgent: "transgate-ios-smoke", width: 390), ports)
    }
}

@MainActor
func signedOut(_ model: AppModel) -> SignedOutVm? {
    if case .signedOut(let vm) = model.view { return vm }
    return nil
}

@MainActor
func signedIn(_ model: AppModel) -> SignedInVm? {
    if case .signedIn(let vm) = model.view { return vm }
    return nil
}

// MARK: - Pure pieces

func purePieces() {
    // translate(): strict, 16 locales, no English fallback (D3).
    let locales = uiLocales()
    check(locales.count == 16, "uiLocales: \(locales)")
    let signIn = Msg(ns: .auth, key: "sign_in.button_label", args: [:])
    let english = try! translate(locale: "en", message: signIn)
    check(english == "Sign in", "en: \(english)")
    for locale in locales where locale != "en" {
        let text = try! translate(locale: locale, message: signIn)
        check(text != english && !text.isEmpty, "\(locale) is still English: \(text)")
    }
    check((try? translate(locale: "en", message: Msg(ns: .auth, key: "no such key", args: [:]))) == nil, "an unknown key must fail")
    let copy = Copy(locale: "de")
    check(copy.text(signIn) == "Anmelden", "Copy de: \(copy.text(signIn))")
    check(copy.text(Msg(ns: .auth, key: "no such key", args: [:])) == "no such key", "an untranslatable message shows its key")
    pass("translate-16-locales-strict")

    // Every static label the UI asks for exists in all sixteen locales
    // in the catalogs, so the SwiftUI layer never meets a strict-i18n error.
    for label in Labels.all {
        let reference = try! translate(locale: "en", message: label.message)
        check(!reference.isEmpty, "empty English for \(label.key)")
        for locale in locales {
            check((try? translate(locale: locale, message: label.message)) != nil, "\(locale) lacks \(label.ns)/\(label.key)")
        }
    }
    pass("label-registry-16-locales")

    // The custom-scheme return and the picker accept string.
    check(AuthReturn.startLocation(for: URL(string: "transgatesmoke://app/tr/home?code=abc&x=1")!, scheme: "transgatesmoke") == "/tr/home?code=abc&x=1", "return location")
    check(AuthReturn.startLocation(for: URL(string: "transgatesmoke://app")!, scheme: "transgatesmoke") == "/home", "bare return")
    check(AuthReturn.startLocation(for: URL(string: "https://example.test/home?code=abc")!, scheme: "transgatesmoke") == nil, "foreign scheme")
    check(unreachable.callbackScheme == "transgatesmoke", "scheme")
    let accept = PickerAccept("audio/mpeg,audio/mp4,.mp3,.m4a")
    check(accept.mimeTypes == ["audio/mpeg", "audio/mp4"] && accept.extensions == ["mp3", "m4a"], "accept: \(accept)")
    pass("auth-return-and-picker-accept")
}

// MARK: - Vault

func vaultPieces() {
    let stores: [(String, SecretStore)] = [("memory", MemorySecretStore()), ("file", FileSecretStore(url: work.appendingPathComponent("vault.json")))]
    for (name, store) in stores {
        check(store.get("session") == nil, "\(name) starts empty")
        store.set("session", "{\"token\":1}")
        store.set("verifier", "v1")
        check(store.get("session") == "{\"token\":1}" && store.get("verifier") == "v1", "\(name) round trip")
        store.remove("session")
        check(store.get("session") == nil && store.get("verifier") == "v1", "\(name) remove")
    }
    let file = FileSecretStore(url: work.appendingPathComponent("vault.json"))
    check(file.get("verifier") == "v1", "the file vault survives a new instance")
    let permissions = (try? FileManager.default.attributesOfItem(atPath: work.appendingPathComponent("vault.json").path)[.posixPermissions] as? Int) ?? 0
    check(permissions == 0o600, "vault permissions \(String(permissions, radix: 8))")
    pass("vault-round-trip-and-permissions")
}

// MARK: - Held files and PUT

final class Ticks: @unchecked Sendable {
    private let lock = NSLock()
    private var values: [UInt8] = []
    func add(_ value: UInt8) { lock.withLock { values.append(value) } }
    var all: [UInt8] { lock.withLock { values } }
}

func filePieces() {
    let audio = work.appendingPathComponent("talk.mp3")
    try! Data(repeating: 7, count: 3_000_000).write(to: audio)
    let note = work.appendingPathComponent("notes.txt")
    try! Data("hello".utf8).write(to: note)
    let files = HeldFiles(reader: FixedDuration())
    let held = files.hold([audio, note])
    check(held.map(\.name) == ["talk.mp3", "notes.txt"] && held[0].size == 3_000_000 && held[0].mime == "audio/mpeg" && held[1].mime == "application/octet-stream", "hold: \(held)")
    check(held[0].handle != held[1].handle, "distinct handles")
    check(files.measure(held[0].handle) == 61.5 && files.measure(held[1].handle) == nil, "measure")
    check(files.measure(9_999) == nil, "an unknown handle measures nothing")

    if let base = ProcessInfo.processInfo.environment["TRANSGATE_SMOKE_HOST"].map({ $0 + "/put" }) {
        let ticks = Ticks()
        let ok = files.upload(held[0].handle, to: base + "/ok") { ticks.add($0) }
        check(ok, "PUT /ok must be accepted")
        check(ticks.all.last == 100 && ticks.all == ticks.all.sorted(), "progress is monotonic and ends at 100: \(ticks.all)")
        let refused = files.upload(held[0].handle, to: base + "/fail") { _ in }
        check(!refused, "a non-200 answer is a failed upload")
        check(!files.upload(held[0].handle, to: "http://127.0.0.1:9/never") { _ in }, "an unreachable host is a failed upload")
        pass("put-200-only-with-progress")
    }
    check(!files.upload(9_999, to: "http://127.0.0.1:9/x") { _ in }, "no file, no PUT")
    files.release(held[0].handle)
    check(files.url(for: held[0].handle) == nil && files.heldCount == 1, "release forgets the handle")
    pass("held-files-measure-release")
}

// MARK: - Ports

@MainActor
func portPieces() {
    let rig = Rig(vault: MemorySecretStore(), settings: unreachable)
    let (_, ports) = rig.model()
    ports.storageSet(key: "k", value: "v")
    check(ports.storageGet(key: "k") == "v", "ports storage")
    ports.storageRemove(key: "k")
    check(ports.storageGet(key: "k") == nil, "ports storage remove")
    ports.leave(url: "http://127.0.0.1:9/auth/v1/authorize?provider=google")
    ports.leave(url: "https://app.transgate.example/tr")
    check(rig.launcher.authenticated == ["transgatesmoke http://127.0.0.1:9/auth/v1/authorize?provider=google"], "auth leave: \(rig.launcher.authenticated)")
    check(rig.launcher.opened == ["https://app.transgate.example/tr"], "web leave: \(rig.launcher.opened)")
    ports.navigate(path: "/tr/home", replace: true)
    check(ports.currentPath == "/tr/home", "navigate")
    check(ports.pausePlayer() && rig.player.calls == ["pause"], "player pause")
    ports.resumePlayer()
    ports.seekPlayer(seconds: 12.5)
    check(rig.player.calls == ["pause", "resume", "seek 12.5"], "player: \(rig.player.calls)")
    check(ports.copy(text: "line", html: nil) && rig.output.copied == ["line"], "copy")
    check(ports.save(bytes: Data([1, 2, 3]), filename: "a.txt", mime: "text/plain") && rig.output.saved == ["a.txt text/plain 3"], "save")
    check(ports.share(bytes: Data(), filename: "a.txt", mime: "text/plain") == .dismissed, "share result passes through")
    let offsets = ports.calendar()
    check(abs(offsets.nowMinutes) <= 14 * 60 && abs(offsets.thenMinutes) <= 14 * 60, "calendar offsets: \(offsets)")
    try? Data("font".utf8).write(to: work.appendingPathComponent("NotoSans.ttf"))
    check(ports.asset(name: "NotoSans.ttf") == Data("font".utf8) && ports.asset(name: "../etc/passwd") == nil && ports.asset(name: "missing.ttf") == nil, "assets")
    pass("native-ports-forwarding")
}

// MARK: - The core through the real bindings

@MainActor
func offlineCore() async {
    let rig = Rig(vault: MemorySecretStore(), settings: unreachable)
    let (model, _) = rig.model()
    await model.start()
    await eventually("sign-out view without a stored session") { signedOut(model) != nil }
    check(model.startError == nil, "start error \(String(describing: model.startError))")
    let auth = signedOut(model)!
    check(auth.locale == "en" && auth.auth.mode == .signIn, "initial signed-out view: \(auth.auth.mode)")
    check(model.copy.text(Msg(ns: .auth, key: "sign_in.button_label", args: [:])) == "Sign in", "copy follows the view's locale")

    model.send(.auth(.emailChanged(text: "ada@example.test")))
    model.send(.auth(.modeChosen(mode: .magicLink)))
    await eventually("form edits reach the view") { signedOut(model)?.auth.email == "ada@example.test" && signedOut(model)?.auth.mode == .magicLink }
    pass("restore-signed-out-and-auth-intents")

    // A returned URL restarts the presenter from that location (D7), in its locale.
    model.handleReturn(URL(string: "transgatesmoke://app/de/home")!)
    await eventually("the German start") { signedOut(model)?.locale == "de" }
    check(model.copy.text(Msg(ns: .auth, key: "sign_in.button_label", args: [:])) == "Anmelden", "German copy")
    check(signedOut(model)?.auth.email == "", "a restart is a fresh core")
    model.handleReturn(URL(string: "https://elsewhere.test/home")!)
    pass("custom-scheme-return-restarts-core")

    let (tr, _) = Rig(vault: MemorySecretStore(), settings: unreachable).model()
    await tr.start(location: "/tr/home")
    await eventually("the Turkish start") { signedOut(tr)?.locale == "tr" }
    check(tr.copy.text(Msg(ns: .auth, key: "sign_in.button_label", args: [:])) != "Sign in", "a localized start is not English")
    tr.stop()
    model.stop()
    pass("locale-from-start-url")
}

// MARK: - Signed in against a running host

@MainActor
func signedInRun(_ environment: [String: String]) async {
    if let host = environment["TRANSGATE_SMOKE_HOST"] { await storedSessionRun(host: host) }
    guard let api = environment["TRANSGATE_SMOKE_API"], let auth = environment["TRANSGATE_SMOKE_AUTH"],
          let email = environment["TRANSGATE_SMOKE_EMAIL"], let password = environment["TRANSGATE_SMOKE_PASSWORD"] else { return }
    let settings = AppSettings(apiUrl: api, authUrl: auth, authKey: environment["TRANSGATE_SMOKE_KEY"], siteUrl: "transgatesmoke://app")
    let store = FileSecretStore(url: work.appendingPathComponent("password-vault.json"))
    let rig = Rig(vault: store, settings: settings)
    let (model, _) = rig.model()
    await model.start()
    await eventually("sign-in form") { signedOut(model) != nil }
    model.send(.auth(.emailChanged(text: email)))
    model.send(.auth(.passwordChanged(text: password)))
    await eventually("a submittable form") { signedOut(model)?.auth.canSubmit == true }
    model.send(.auth(.submitted))
    await eventually("the signed-in view", seconds: 40) { signedIn(model) != nil }
    check(signedIn(model)?.profile?.email == email, "profile email")
    check(store.get("transgate.session") != nil, "the session reached the vault")
    model.send(.signOutRequested)
    await eventually("signed out", seconds: 40) { signedOut(model) != nil }
    check(store.get("transgate.session") == nil, "sign-out clears the vault")
    model.stop()
    pass("password-sign-in-real-host")
}

/// A stored session restores against the fake host, the native flows run through the
/// real core, and sign-out clears the vault.
@MainActor
func storedSessionRun(host: String) async {
    let userId = "6f1d2c3b-4a5e-4f60-8a7b-9c0d1e2f3a4b"
    let email = "ios-smoke@example.test"
    let settings = AppSettings(apiUrl: host + "/api", authUrl: host + "/auth/v1", authKey: nil, siteUrl: "transgatesmoke://app")
    let store = FileSecretStore(url: work.appendingPathComponent("signed-in-vault.json"))
    let session = "{\"access_token\":\"t\",\"refresh_token\":\"r\",\"expires_at\":4000000000,\"user_id\":\"\(userId)\",\"email\":\"\(email)\"}"
    store.set("transgate.session", session)
    let audio = work.appendingPathComponent("talk.mp3")
    try? Data(repeating: 7, count: 2_000).write(to: audio)
    let rig = Rig(vault: store, settings: settings)
    let (model, _) = rig.model()
    await model.start()
    await eventually("the stored session restores", seconds: 40) { signedIn(model) != nil }
    check(signedIn(model)?.profile?.email == email, "profile email \(String(describing: signedIn(model)?.profile?.email))")
    check(signedIn(model)?.layouts.frame == .phone, "the native shell starts in the phone layout")
    check(signedIn(model)?.apiKeys == nil && signedIn(model)?.invoices == nil && signedIn(model)?.purchase == nil, "no purchase, invoice or key screen at home")
    check(!model.copy.text(signedIn(model)!.balance).isEmpty, "the balance translates")
    await eventually("jobs loaded") { signedIn(model)?.jobs.initialLoading == false }
    pass("stored-session-restores-and-lists-jobs")

    // The picker returned a file: core holds the handle, the shell measures it, the row appears.
    model.picked([audio])
    await eventually("the picked file in the queue") { signedIn(model)?.queue.rows.count == 1 }
    let row = signedIn(model)!.queue.rows[0]
    check(row.name == "talk.mp3", "queue row \(row.name)")
    // Core asked the shell to measure the held file through the port (minutes are core's rounding).
    await eventually("core measured the file through the port") { rig.reader.measurements >= 1 }
    check(rig.files.heldCount == 1, "the shell holds the picked file")
    model.send(.upload(.removed(file: row.id)))
    await eventually("the row removed") { signedIn(model)?.queue.rows.isEmpty == true }
    // The driver forgets held handles when the presenter is disposed (checked after stop below).
    pass("pick-measure-remove-through-core")

    model.send(.settings(.opened))
    await eventually("settings open") { signedIn(model)?.settings?.open == true }
    model.send(.settings(.closed))
    await eventually("settings closed") { signedIn(model)?.settings?.open != true }
    pass("settings-round-trip")

    // A second launch with the same vault restores without a form; sign-out clears it.
    model.stop()
    await eventually("dispose released the held handle") { rig.files.heldCount == 0 }
    let (again, _) = rig.model()
    await again.start()
    await eventually("session restored from the vault", seconds: 40) { signedIn(again) != nil }
    again.send(.signOutRequested)
    await eventually("signed out again", seconds: 40) { signedOut(again) != nil }
    check(store.get("transgate.session") == nil, "sign-out clears the vault")
    again.stop()
    pass("vault-restore-and-sign-out")
}

@main
struct Smoke {
    @MainActor
    static func main() async {
        purePieces()
        uiContract()
        vaultPieces()
        filePieces()
        portPieces()
        await offlineCore()
        await signedInRun(ProcessInfo.processInfo.environment)
        try? FileManager.default.removeItem(at: work)
        print("P8_IOS_PASS \(passed.joined(separator: " "))")
    }
}
