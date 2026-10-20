import SwiftUI

/// Composition root: the only place that knows about concrete adapters. With
/// `TASKR_API_URL` set (build setting, see project.yml) the app signs in to a
/// taskr server; without it, tasks stay on the device.
@main
struct TaskrApp: App {
    private static let apiUrl = TaskrApp.setting("TaskrApiUrl")
    private static let authUrl = TaskrApp.setting("TaskrAuthUrl")
    private static let authKey = TaskrApp.setting("TaskrAuthKey")

    @State private var tasks = TasksModel(
        presenter: TaskListPresenter(dataDir: TaskrApp.dataDirectory(), sink: OSLogSink())
    )
    @State private var app: AppModel? = TaskrApp.apiUrl.map { url in
        AppModel(presenter: AppPresenter(
            config: AppConfig(apiUrl: url, authUrl: TaskrApp.authUrl, authKey: TaskrApp.authKey),
            vault: KeychainSessionVault(),
            sink: OSLogSink()
        ))
    }

    var body: some Scene {
        WindowGroup {
            if let app {
                AppRootView(view: app.view, devSignIn: TaskrApp.authUrl == nil, onIntent: app.send)
                    .task { app.send(.started) }
            } else {
                TaskListScreen(view: tasks.view, onIntent: tasks.send)
                    .task { tasks.send(.opened) }
            }
        }
    }

    private static func dataDirectory() -> String {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].path
    }

    /// Empty build settings arrive as empty strings; treat them as unset.
    private static func setting(_ key: String) -> String? {
        let value = Bundle.main.object(forInfoDictionaryKey: key) as? String
        return value?.isEmpty == false ? value : nil
    }
}
