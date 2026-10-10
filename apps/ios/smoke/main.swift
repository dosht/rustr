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

    check(previewScreens().map(\.name) == ["empty", "loading", "sample", "error"], "previews")
    check(sink.events.first == "info app.started", "events: \(sink.events)")
    check(sink.events.contains("warn task.rejected"), "events: \(sink.events)")
    print("swift bindings smoke test passed (\(sink.events.count) log events via LogSink)")
}

@main
struct Smoke {
    static func main() async {
        await run()
    }
}
