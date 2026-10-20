package dev.taskr.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import dev.taskr.android.ui.AppScreen
import dev.taskr.android.ui.TaskListScreen
import dev.taskr.core.AppConfig
import dev.taskr.core.AppPresenter
import dev.taskr.core.TaskListPresenter

/**
 * Composition root: the only place that knows about concrete adapters. Built
 * with `-Ptaskr.apiUrl=...` the app signs in to a taskr server; without it,
 * tasks stay on the device.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val dataDir = filesDir.absolutePath
        val apiUrl = BuildConfig.TASKR_API_URL
        val authUrl = BuildConfig.TASKR_AUTH_URL.ifEmpty { null }
        val authKey = BuildConfig.TASKR_AUTH_KEY.ifEmpty { null }
        setContent {
            MaterialTheme {
                if (apiUrl.isNotEmpty()) {
                    val model: AppViewModel = viewModel {
                        val config = AppConfig(apiUrl, authUrl, authKey)
                        AppViewModel(AppPresenter(config, PrefsSessionVault(applicationContext), LogcatSink()))
                    }
                    val view by model.view.collectAsStateWithLifecycle()
                    AppScreen(view = view, devSignIn = authUrl == null, onIntent = model::send)
                } else {
                    val model: TasksViewModel = viewModel {
                        TasksViewModel(TaskListPresenter(dataDir, LogcatSink()))
                    }
                    val view by model.view.collectAsStateWithLifecycle()
                    TaskListScreen(view = view, onIntent = model::send)
                }
            }
        }
    }
}
