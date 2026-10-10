package dev.taskr.android

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dev.taskr.core.TaskIntent
import dev.taskr.core.TaskListPresenter
import dev.taskr.core.TaskListView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * Android's container. Holds the presenter across configuration changes and
 * moves calls off the main thread. The state machine itself is in Rust.
 */
class TasksViewModel(private val presenter: TaskListPresenter) : ViewModel() {
    // One lane, so intents reach the core in the order the user made them.
    @OptIn(ExperimentalCoroutinesApi::class)
    private val core = Dispatchers.IO.limitedParallelism(1)

    private val _view = MutableStateFlow<TaskListView?>(null)
    val view: StateFlow<TaskListView?> = _view.asStateFlow()

    init {
        send(TaskIntent.Opened)
    }

    fun send(intent: TaskIntent) {
        viewModelScope.launch(core) { _view.value = presenter.dispatch(intent) }
    }

    override fun onCleared() {
        presenter.close()
    }
}
