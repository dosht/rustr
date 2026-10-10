import SwiftUI

/// Composition root: the only place that knows about concrete adapters.
@main
struct TaskrApp: App {
    @State private var model = TasksModel(
        presenter: TaskListPresenter(dataDir: TaskrApp.dataDirectory(), sink: OSLogSink())
    )

    var body: some Scene {
        WindowGroup {
            TaskListScreen(view: model.view, onIntent: model.send)
                .task { model.send(.opened) }
        }
    }

    private static func dataDirectory() -> String {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].path
    }
}
