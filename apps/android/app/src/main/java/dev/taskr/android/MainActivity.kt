package dev.taskr.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import dev.taskr.android.ui.TaskListScreen
import dev.taskr.core.TaskListPresenter

/** Composition root: the only place that knows about concrete adapters. */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val dataDir = filesDir.absolutePath
        setContent {
            MaterialTheme {
                val model: TasksViewModel = viewModel {
                    TasksViewModel(TaskListPresenter(dataDir, LogcatSink()))
                }
                val view by model.view.collectAsStateWithLifecycle()
                TaskListScreen(view = view, onIntent = model::send)
            }
        }
    }
}
