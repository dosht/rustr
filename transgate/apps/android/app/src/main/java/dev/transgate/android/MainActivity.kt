package dev.transgate.android

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.transgate.android.ui.TransgateRoot

/**
 * Composition root: the only place that knows the concrete adapters (via the view model). The
 * Custom Tab returns here through the auth scheme (D7); the path and query go to core, which
 * recognizes the `?code=` callback and exchanges it with the stored PKCE verifier.
 */
class MainActivity : ComponentActivity() {
    private val model: TransgateViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val view by model.view.collectAsStateWithLifecycle()
            TransgateRoot(view, model::send, model.host, model::viewportChanged)
        }
        if (savedInstanceState == null) handle(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    override fun onStart() {
        super.onStart()
        model.visibilityChanged(true)
    }

    override fun onStop() {
        super.onStop()
        model.visibilityChanged(false)
    }

    private fun handle(intent: Intent) {
        val uri = intent.data ?: return
        if (uri.scheme != Uri.parse(BuildConfig.SITE_URL).scheme) return
        val query = uri.encodedQuery?.let { "?$it" } ?: ""
        model.authCallback((uri.encodedPath ?: "/") + query)
    }
}
