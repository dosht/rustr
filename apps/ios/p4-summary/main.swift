// Generated UniFFI consumer, not a Rust DTO roundtrip. Uses the real taskr host,
// owned completed-work SQL fixture, and controlled loopback completion server.
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

func check(_ condition: Bool, _ message: String) {
    guard condition else { fputs("P4 UNIFFI FAILED: \(message)\n", stderr); exit(1) }
}
func required(_ name: String) -> String {
    guard let value = ProcessInfo.processInfo.environment[name], !value.isEmpty else {
        fatalError("\(name) is required; owned local P4 fixtures only")
    }
    return value
}
final class Vault: SessionVault, @unchecked Sendable {
    private let lock = NSLock()
    private var value: String?
    func load() -> String? { lock.withLock { value } }
    func save(value: String) { lock.withLock { self.value = value } }
    func clear() { lock.withLock { value = nil } }
}
final class Sink: LogSink, @unchecked Sendable {
    func log(level: LogLevel, event: String, fields: [LogField]) {}
}
final class Listener: AudioSummaryListener, @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [AudioSummaryView] = []
    private var acted = false
    private var mainThread = false
    var hook: ((AudioSummaryView) -> Void)?
    var views: [AudioSummaryView] { lock.withLock { recorded } }
    var calledOnMain: Bool { lock.withLock { mainThread } }
    func onView(view: AudioSummaryView) {
        let act = lock.withLock { () -> Bool in
            recorded.append(view)
            mainThread = mainThread || Thread.isMainThread
            if !acted && view.text == "Hello" && !view.durable { acted = true; return true }
            return false
        }
        // Release the listener's own lock too: the hook re-enters Rust current/suspend.
        if act { hook?(view) }
    }
}

func request(_ url: String, method: String = "GET", bearer: String? = nil) -> [String: Any] {
    var request = URLRequest(url: URL(string: url)!)
    request.httpMethod = method
    request.timeoutInterval = 10
    if let bearer { request.setValue("Bearer \(bearer)", forHTTPHeaderField: "Authorization") }
    let done = DispatchSemaphore(value: 0)
    // Foundation's completion is synchronized by the semaphore before reading.
    final class ResultBox: @unchecked Sendable { var data: Data?; var status = 0; var error: Error? }
    let result = ResultBox()
    URLSession.shared.dataTask(with: request) { data, response, error in
        result.data = data; result.status = (response as? HTTPURLResponse)?.statusCode ?? 0; result.error = error; done.signal()
    }.resume()
    check(done.wait(timeout: .now() + 12) == .success, "HTTP control timed out")
    check(result.error == nil && (200..<300).contains(result.status), "HTTP control \(result.status): \(String(describing: result.error))")
    guard let data = result.data, let body = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { fatalError("invalid control JSON") }
    return body
}
func runs(_ control: String) -> [[String: Any]] {
    guard let rows = request("\(control)/runs")["runs"] as? [[String: Any]] else { fatalError("missing run list") }
    return rows
}
func onlyNewRun(_ control: String, after before: Set<String>) -> String {
    let ids = runs(control).compactMap { $0["id"] as? String }.filter { !before.contains($0) }
    check(ids.count == 1, "expected exactly one explicit provider dispatch; got \(ids)")
    return ids[0]
}
func watchOffMain(_ presenter: AudioSummaryPresenter, _ listener: Listener) {
    let done = DispatchSemaphore(value: 0)
    DispatchQueue.global(qos: .userInitiated).async { _ = presenter.watch(listener: listener); done.signal() }
    check(done.wait(timeout: .now() + 20) == .success, "foreign listener watch did not settle")
    check(!listener.calledOnMain, "native callback must run off the UI thread")
}

@main struct P4SummaryProof {
    static func main() {
        let api = required("TASKR_P4_API")
        let email = required("TASKR_P4_EMAIL")
        let work = required("TASKR_P4_WORK_ID")
        let control = required("TASKR_P4_PROVIDER_CONTROL")
        let vault = Vault()
        // Use the existing auth presenter, not a new session/auth implementation.
        let app = AppPresenter(config: AppConfig(apiUrl: api, authUrl: nil, authKey: nil), vault: vault, sink: Sink())
        _ = app.dispatch(intent: .started)
        _ = app.dispatch(intent: .signInSubmitted(email: email, password: ""))
        check(vault.load() != nil, "existing sign-in must populate SessionVault")
        let presenter = AudioSummaryPresenter(apiUrl: api, vault: vault)
        let initialRuns = Set(runs(control).compactMap { $0["id"] as? String })
        _ = presenter.refresh()
        check(presenter.select(workId: work).canStart, "fixture must be enrolled, owned and completed")
        _ = presenter.start(requestId: UUID().uuidString, accepted: false)
        check(Set(runs(control).compactMap { $0["id"] as? String }) == initialRuns, "refusing disclosure must not dispatch")
        let started = presenter.start(requestId: UUID().uuidString, accepted: true)
        guard let generation = started.generationId else { fatalError("start must return a real generation: \(String(describing: started.error))") }
        let partial = Listener()
        partial.hook = { _ in
            check(presenter.current().text == "Hello", "listener can re-enter current outside Rust reducer locks")
            _ = presenter.suspend()
        }
        watchOffMain(presenter, partial)
        check(partial.views.contains { $0.text == "Hello" && !$0.durable }, "foreign callback must arrive before upstream completion")
        check(presenter.current().canResume, "suspend detaches without cancelling generation")
        let run = onlyNewRun(control, after: initialRuns)
        check(runs(control).first { $0["id"] as? String == run }?["finished"] as? Bool == false, "upstream must still be held")
        let resumed = Listener()
        resumed.hook = { view in
            check(view.generationId == generation, "reconnect retains generation identity")
            _ = request("\(control)/runs/\(run)/finish", method: "POST")
        }
        watchOffMain(presenter, resumed)
        let final = presenter.current()
        check(final.durable && final.text == "Hello world", "authoritative final replaces trimmed snapshot, never Helloworld or trailing whitespace")
        check(final.generationId == generation, "resume must not create another generation")
        check(runs(control).count == initialRuns.count + 1, "reconnect must not redispatch provider")
        let reloaded = AudioSummaryPresenter(apiUrl: api, vault: vault)
        _ = reloaded.refresh()
        let saved = reloaded.select(workId: work)
        check(saved.durable && saved.text == "Hello world", "new foreign presenter reloads canonical saved summary")
        reloaded.dispose()

        let beforeCancel = Set(runs(control).compactMap { $0["id"] as? String })
        _ = presenter.start(requestId: UUID().uuidString, accepted: true)
        let cancelling = Listener()
        cancelling.hook = { _ in _ = presenter.suspend() }
        watchOffMain(presenter, cancelling)
        let cancelRun = onlyNewRun(control, after: beforeCancel)
        let cancelled = presenter.cancel()
        check(cancelled.statusLabel.contains("Stopped locally") && cancelled.remoteOutcomeUnknown, "cancel is local, not proof of remote deletion")
        _ = request("\(control)/runs/\(cancelRun)/finish", method: "POST")
        let canonical = request("\(api)/audio-work/\(work)/summary", bearer: "dev:\(email)")
        check((canonical["summary"] as? [String: Any])?["generation_id"] as? String == generation, "late cancelled response must not replace saved result")

        let beforeLoss = Set(runs(control).compactMap { $0["id"] as? String })
        _ = presenter.start(requestId: UUID().uuidString, accepted: true)
        let losingSession = Listener()
        losingSession.hook = { _ in
            vault.clear()
            check(presenter.current().text.isEmpty, "session loss clears old owner content synchronously")
        }
        watchOffMain(presenter, losingSession)
        let lostRun = onlyNewRun(control, after: beforeLoss)
        presenter.dispose()
        let callbacks = losingSession.views.count
        _ = request("\(control)/runs/\(lostRun)/finish", method: "POST")
        Thread.sleep(forTimeInterval: 0.2)
        check(losingSession.views.count == callbacks, "disposed foreign listener receives no late content")
        check(presenter.current().text.isEmpty && presenter.current().works.isEmpty, "disposed session remains clear")
        check(runs(control).count == initialRuns.count + 3, "only three explicit starts may dispatch")
        print("P4_UNIFFI_PASS progressive-before-final authoritative-replacement detach/resume canonical-reload cancellation session-loss one-dispatch-per-start")
    }
}
