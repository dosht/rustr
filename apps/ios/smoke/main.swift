// Exercises the generated Swift bindings and the iOS container (TasksModel)
// against the host build of the core, so the FFI contract and the threading
// model can be checked on Linux before Xcode. Run with scripts/smoke-swift.sh.
import Foundation

final class RecordingSink: LogSink, @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [String] = []

    var events: [String] { lock.withLock { recorded } }

    func log(level: LogLevel, event: String, fields: [LogField]) {
        lock.withLock { recorded.append("\(level) \(event)") }
    }
}

final class MemoryVault: SessionVault, @unchecked Sendable {
    private let lock = NSLock()
    private var stored: String?

    func load() -> String? { lock.withLock { stored } }
    func save(value: String) { lock.withLock { stored = value } }
    func clear() { lock.withLock { stored = nil } }
}

func check(_ condition: Bool, _ message: @autoclosure () -> String) {
    if !condition {
        print("FAILED: \(message())")
        exit(1)
    }
}

@MainActor
func waitFor(_ model: TasksModel, _ done: (TaskListView) -> Bool) async -> TaskListView {
    for _ in 0..<200 {
        if let view = model.view, done(view) { return view }
        try? await Task.sleep(nanoseconds: 5_000_000)
    }
    check(false, "timed out, last view: \(String(describing: model.view))")
    fatalError()
}

@MainActor
func run() async {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("taskr-swift-\(ProcessInfo.processInfo.processIdentifier)").path
    let sink = RecordingSink()
    let model = TasksModel(presenter: TaskListPresenter(dataDir: dir, sink: sink))

    model.send(.opened)
    var view = await waitFor(model) { $0.emptyMessage != nil }
    check(view.emptyMessage == "Nothing here yet. Add your first task.", "empty: \(view)")

    model.send(.addSubmitted(title: "Buy milk"))
    model.send(.addSubmitted(title: "Call mom"))
    view = await waitFor(model) { $0.rows.count == 2 }
    check(view.rows.map(\.title) == ["Buy milk", "Call mom"], "rows: \(view.rows)")

    model.send(.addSubmitted(title: "BUY MILK"))
    view = await waitFor(model) { $0.error != nil }
    check(view.error == "There is already an open task with that title.", "error: \(view)")

    model.send(.toggleRequested(id: 1))
    view = await waitFor(model) { $0.rows.count == 1 }
    check(view.tabs.first { $0.scope == .done }?.label == "Done (1)", "tabs: \(view.tabs)")
    check(view.columns.first { $0.title == "ID" }?.wide == true, "ID is a wide column")

    check(previewScreens().map(\.name) == ["empty", "loading", "sample", "error", "read-only"], "previews")
    check(sink.events.first == "info app.started", "events: \(sink.events)")
    check(sink.events.contains("warn task.rejected"), "events: \(sink.events)")
    await signedIn(sink)
    print("swift bindings smoke test passed (\(sink.events.count) log events via LogSink)")
}

/// With TASKR_SMOKE_API (a server started with TASKR_DEV_AUTH=1) it goes end to end.
@MainActor
func signedIn(_ sink: LogSink) async {
    let api = ProcessInfo.processInfo.environment["TASKR_SMOKE_API"]
    let vault = MemoryVault()
    let model = AppModel(presenter: AppPresenter(
        config: AppConfig(apiUrl: api ?? "http://127.0.0.1:9/api", authUrl: nil, authKey: nil),
        vault: vault,
        sink: sink
    ))
    func settle(_ done: (AppView) -> Bool) async -> AppView {
        for _ in 0..<400 {
            if done(model.view) { return model.view }
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
        check(false, "timed out, last view: \(model.view)")
        fatalError()
    }
    model.send(.started)
    _ = await settle { if case .signedOut = $0 { return true }; return false }
    let email = "swift-\(ProcessInfo.processInfo.processIdentifier)@example.com"
    model.send(.signInSubmitted(email: email, password: ""))
    let view = await settle {
        if case .signedIn(let v) = $0, v.error != nil || v.workspace != nil { return true }
        return false
    }
    check(vault.load()?.contains("dev:\(email)") == true, "the session is kept in the vault")
    guard case .signedIn(let signedIn) = view else { fatalError() }
    if api == nil {
        check(signedIn.error?.contains("could not reach the server") == true, "error: \(String(describing: signedIn.error))")
        return
    }
    guard let workspace = signedIn.workspace else { fatalError("no workspace open: \(signedIn)") }
    model.send(.tasks(workspace: workspace, intent: .addSubmitted(title: "From Swift")))
    let added = await settle {
        if case .signedIn(let v) = $0, case .tasks(let t) = v.screen, !t.rows.isEmpty { return true }
        return false
    }
    guard case .signedIn(let v) = added, case .tasks(let tasks) = v.screen else { fatalError() }
    check(tasks.rows.map(\.title) == ["From Swift"], "rows: \(tasks.rows)")
    print("signed in as \(email) against \(api!)")
}

@main
struct Smoke {
    static func main() async {
        await run()
    }
}
