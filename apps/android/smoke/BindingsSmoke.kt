// Exercises the generated Kotlin bindings on a plain JVM against the host
// build of the core, so the FFI contract can be checked on Linux without the
// Android SDK. Run with scripts/smoke-kotlin.sh.
import dev.taskr.core.AppConfig
import dev.taskr.core.AppIntent
import dev.taskr.core.AppPresenter
import dev.taskr.core.AppView
import dev.taskr.core.AppViewListener
import dev.taskr.core.LogField
import dev.taskr.core.LogLevel
import dev.taskr.core.LogSink
import dev.taskr.core.ScreenView
import dev.taskr.core.SessionVault
import dev.taskr.core.TaskIntent
import dev.taskr.core.TaskListPresenter
import dev.taskr.core.TaskScope
import dev.taskr.core.previewScreens
import java.nio.file.Files

class RecordingSink : LogSink {
    val events = mutableListOf<String>()

    override fun log(level: LogLevel, event: String, fields: List<LogField>) {
        synchronized(events) { events += "$level $event" }
    }
}

class MemoryVault : SessionVault {
    @Volatile var value: String? = null

    override fun load(): String? = value
    override fun save(value: String) {
        this.value = value
    }
    override fun clear() {
        value = null
    }
}

fun check(condition: Boolean, message: () -> String) {
    if (!condition) throw IllegalStateException(message())
}

fun main() {
    val dir = Files.createTempDirectory("taskr-kotlin").toString()
    val sink = RecordingSink()
    TaskListPresenter(dir, sink).use { presenter ->
        presenter.dispatch(TaskIntent.Opened)
        var view = presenter.dispatch(TaskIntent.AddSubmitted("Buy milk"))
        check(view.rows.map { it.title } == listOf("Buy milk")) { "rows: ${view.rows}" }
        check(view.notice == "Added \"Buy milk\"") { "notice: ${view.notice}" }

        view = presenter.dispatch(TaskIntent.AddSubmitted("buy MILK"))
        check(view.error == "There is already an open task with that title.") { "error: ${view.error}" }

        view = presenter.dispatch(TaskIntent.ToggleRequested(view.rows[0].id))
        check(view.tabs.first { it.scope == TaskScope.DONE }.label == "Done (1)") { "tabs: ${view.tabs}" }
        check(view.columns.first { it.title == "ID" }.wide) { "ID should be a wide column" }
    }
    check(sink.events.first() == "INFO app.started") { "events: ${sink.events}" }
    check("WARN task.rejected" in sink.events) { "events: ${sink.events}" }
    check(previewScreens().map { it.name } == listOf("empty", "loading", "sample", "error", "read-only")) { "previews" }
    signedIn(sink)
    println("kotlin bindings smoke test passed (${sink.events.size} log events via LogSink)")
}

/** With TASKR_SMOKE_API (a server started with TASKR_DEV_AUTH=1) it goes end to end. */
fun signedIn(sink: LogSink) {
    val api = System.getenv("TASKR_SMOKE_API")
    val vault = MemoryVault()
    AppPresenter(AppConfig(api ?: "http://127.0.0.1:9/api", null, null), vault, sink).use { app ->
        val views = mutableListOf<AppView>()
        val listener = object : AppViewListener {
            override fun onView(view: AppView) {
                views += view
            }
        }
        check(app.dispatchWith(AppIntent.Started, listener) is AppView.SignedOut) { "starts signed out" }
        check(views.firstOrNull() == AppView.Starting) { "views while restoring: $views" }
        val email = "kotlin-${System.nanoTime()}@example.com"
        var view = app.dispatch(AppIntent.SignInSubmitted(email, ""))
        check(vault.value?.contains("dev:$email") == true) { "the session is kept in the vault" }
        val signedIn = (view as AppView.SignedIn).view
        if (api == null) {
            check(signedIn.error?.contains("could not reach the server") == true) { "error: ${signedIn.error}" }
            return
        }
        val workspace = checkNotNull(signedIn.workspace) { "a workspace is open" }
        view = app.dispatch(AppIntent.Tasks(workspace, TaskIntent.AddSubmitted("From Kotlin")))
        val tasks = ((view as AppView.SignedIn).view.screen as ScreenView.Tasks).view
        check(tasks.rows.map { it.title } == listOf("From Kotlin")) { "rows: ${tasks.rows}" }
        println("signed in as $email against $api")
    }
}
