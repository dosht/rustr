package dev.taskr.android

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dev.taskr.core.AppIntent
import dev.taskr.core.AppPresenter
import dev.taskr.core.AppView
import dev.taskr.core.AppViewListener
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * The signed-in app's container: same shape as TasksViewModel. Sign-in,
 * navigation and session expiry are all decided in Rust.
 */
class AppViewModel(private val presenter: AppPresenter) : ViewModel() {
    // One lane, so intents reach the core in the order the user made them.
    @OptIn(ExperimentalCoroutinesApi::class)
    private val core = Dispatchers.IO.limitedParallelism(1)

    private val _view = MutableStateFlow<AppView>(AppView.Starting)
    val view: StateFlow<AppView> = _view.asStateFlow()

    // The views the core shows while requests run (e.g. the new workspace
    // loading), so the previous screen does not stay interactive meanwhile.
    private val listener = object : AppViewListener {
        override fun onView(view: AppView) {
            _view.value = view
        }
    }

    init {
        send(AppIntent.Started)
    }

    fun send(intent: AppIntent) {
        viewModelScope.launch(core) { _view.value = presenter.dispatchWith(intent, listener) }
    }

    override fun onCleared() {
        presenter.close()
    }
}
