// Exercises the generated Kotlin bindings on a plain JVM against the host
// build of the core, so the FFI contract can be checked on Linux without the
// Android SDK. Run with scripts/smoke-kotlin.sh.
import dev.taskr.core.LogField
import dev.taskr.core.LogLevel
import dev.taskr.core.LogSink
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
    check(previewScreens().map { it.name } == listOf("empty", "loading", "sample", "error")) { "previews" }
    println("kotlin bindings smoke test passed (${sink.events.size} log events via LogSink)")
}
