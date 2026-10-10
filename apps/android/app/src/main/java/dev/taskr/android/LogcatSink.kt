package dev.taskr.android

import android.util.Log
import dev.taskr.core.LogField
import dev.taskr.core.LogLevel
import dev.taskr.core.LogSink

/** The telemetry port, fulfilled by logcat. */
class LogcatSink : LogSink {
    override fun log(level: LogLevel, event: String, fields: List<LogField>) {
        val message = (listOf(event) + fields.map { "${it.key}=${it.value}" }).joinToString(" ")
        when (level) {
            LogLevel.DEBUG -> Log.d(TAG, message)
            LogLevel.INFO -> Log.i(TAG, message)
            LogLevel.WARN -> Log.w(TAG, message)
            LogLevel.ERROR -> Log.e(TAG, message)
        }
    }

    private companion object {
        const val TAG = "taskr"
    }
}
