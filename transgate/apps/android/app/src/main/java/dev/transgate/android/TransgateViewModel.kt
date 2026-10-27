package dev.transgate.android

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.transgate.android.ports.AndroidNativePorts
import dev.transgate.android.ports.KeystoreVault
import dev.transgate.android.ports.Outputs
import dev.transgate.android.ports.PickedFiles
import dev.transgate.android.ports.PlayerHost
import dev.transgate.core.AppIntent
import dev.transgate.core.AppVm
import dev.transgate.core.PresenterConfig
import dev.transgate.core.TranscriptIntent
import dev.transgate.core.TransgatePresenter
import dev.transgate.core.ViewListener
import dev.transgate.core.Viewport
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.util.TimeZone

/**
 * Holds the one `TransgatePresenter` and the latest view. It decides nothing: every intent is
 * forwarded in the order the person made it, and the view core answers with is published.
 *
 * `dispatch` and `current` block on the driver thread, so they run on a single lane off the
 * main thread. The listener only schedules a refresh on that same lane, so a view that an
 * effect produced can never overtake the answer of a later intent: every published view is
 * read from core in lane order.
 */
class TransgateViewModel(app: Application) : AndroidViewModel(app) {
    @OptIn(ExperimentalCoroutinesApi::class)
    private val lane = Dispatchers.IO.limitedParallelism(1)

    private val outputs = Outputs(app)
    private val files = PickedFiles(app)
    val player = PlayerHost(
        onPlaying = { playing -> send(AppIntent.Transcript(TranscriptIntent.PlayerState(playing))) },
        openUrl = outputs::leave,
    )
    private val ports = AndroidNativePorts(app, KeystoreVault(app), files, player, outputs, reload = ::reload)

    private val _view = MutableStateFlow<AppVm>(AppVm.Restoring(locale = "en"))
    val view: StateFlow<AppVm> = _view.asStateFlow()

    private var presenter: TransgatePresenter? = null
    private var viewport = Viewport(touch = true, width = 360u)

    private val listener = object : ViewListener {
        override fun viewChanged(view: AppVm) {
            viewModelScope.launch(lane) { presenter?.let { active -> runCatching { active.current() }.onSuccess { _view.value = it } } }
        }
    }

    val host = ShellHost(
        player = player,
        files = files,
        openUrl = outputs::leave,
        webUrl = BuildConfig.TRANSGATE_WEB_URL,
    )

    init {
        start("/home")
    }

    fun send(intent: AppIntent) {
        viewModelScope.launch(lane) {
            val active = presenter ?: return@launch
            // A disposed presenter (a reload in flight) answers `Disposed`; its intent is moot.
            runCatching { active.dispatch(intent) }.onSuccess { _view.value = it }
        }
    }

    /** A custom-scheme return from the Custom Tab (D7): core parses the path and `?code=`. */
    fun authCallback(pathAndQuery: String) = send(AppIntent.RouteChanged(pathAndQuery))

    fun viewportChanged(widthDp: Int) {
        viewport = Viewport(touch = true, width = widthDp.toUInt())
        send(AppIntent.ViewportChanged(viewport))
    }

    fun visibilityChanged(visible: Boolean) = send(AppIntent.VisibilityChanged(visible))

    /** What a browser does on `Leave` with an app path (a locale switch): load it afresh. */
    private fun reload(path: String) {
        viewModelScope.launch(lane) {
            presenter?.dispose()
            presenter = null
            create(path)
        }
    }

    private fun start(path: String) {
        viewModelScope.launch(lane) { create(path) }
    }

    private fun create(path: String) {
        val config = PresenterConfig(
            apiUrl = BuildConfig.TRANSGATE_API_URL,
            authUrl = BuildConfig.TRANSGATE_AUTH_URL,
            authKey = BuildConfig.TRANSGATE_AUTH_KEY.ifEmpty { null },
            siteUrl = BuildConfig.SITE_URL,
            captcha = false,
            userAgent = "Transgate-Android/${BuildConfig.VERSION_NAME}",
        )
        val fresh = TransgatePresenter(config, ports, listener)
        presenter = fresh
        _view.value = fresh.dispatch(AppIntent.Started(path, viewport, (TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 60_000).toShort()))
    }

    override fun onCleared() {
        presenter?.dispose()
        presenter = null
    }
}
