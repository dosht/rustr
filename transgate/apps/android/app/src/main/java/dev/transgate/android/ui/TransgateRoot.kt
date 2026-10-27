package dev.transgate.android.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import dev.transgate.android.ShellHost
import dev.transgate.core.AppIntent
import dev.transgate.core.AppVm

/** The locale core chose for this view; the whole tree is translated into it. */
fun AppVm.locale(): String = when (this) {
    is AppVm.Restoring -> locale
    is AppVm.SignedOut -> v1.locale
    is AppVm.SignedIn -> v1.locale
    is AppVm.CheckoutConfirm -> locale
    is AppVm.Erased -> locale
}

/**
 * The composition root's one screen: the app state core reports picks the body. Test tags are
 * exposed as resource ids so UiAutomator can drive the screens by name.
 */
@Composable
fun TransgateRoot(view: AppVm, send: (AppIntent) -> Unit, host: ShellHost, onWidthDp: (Int) -> Unit) {
    val width = LocalConfiguration.current.screenWidthDp
    LaunchedEffect(width) { onWidthDp(width) }
    TransgateTheme(view.locale()) {
        Surface(Modifier.fillMaxSize().semantics { testTagsAsResourceId = true }, color = MaterialTheme.colorScheme.background) {
            when (view) {
                is AppVm.Restoring -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(Modifier.testTag("restoring"))
                }
                is AppVm.SignedOut -> AuthScreen(view.v1, send, host)
                is AppVm.SignedIn -> SignedInScreen(view.v1, send, host)
                is AppVm.CheckoutConfirm -> CheckoutConfirmScreen(view, send)
                is AppVm.Erased -> Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
                    Text(view.message.text(), modifier = Modifier.testTag("erased"))
                }
            }
        }
    }
}
