package dev.transgate.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.transgate.android.ShellHost
import dev.transgate.core.AppIntent
import dev.transgate.core.HomeView
import dev.transgate.core.Ns
import dev.transgate.core.Route
import dev.transgate.core.SettingsIntent
import dev.transgate.core.SignedInVm
import dev.transgate.core.SupportIntent
import kotlinx.coroutines.launch

/**
 * The signed-in phone frame (WEB-051..076): a top bar, the navigation drawer, and the one
 * body core's route names. Everything shown comes from [SignedInVm]; every tap is an intent.
 * D6: the balance is shown and nothing here sells minutes; invoices and API keys are
 * handoffs to the web app.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SignedInScreen(vm: SignedInVm, send: (AppIntent) -> Unit, host: ShellHost) {
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val route = vm.route
    val job = (route as? Route.Home)?.v1 as? HomeView.Job
    val all = (route as? Route.Home)?.v1 is HomeView.AllTranscripts

    // The system back gesture steps out of a transcript to the list, and out of the list to
    // a new transcript: the same two hops the phone header's back arrow makes.
    BackHandler(enabled = drawer.isOpen || job != null || all) {
        when {
            drawer.isOpen -> scope.launch { drawer.close() }
            job != null -> send(AppIntent.Navigate(Route.Home(HomeView.AllTranscripts)))
            else -> send(AppIntent.Navigate(vm.newTranscriptRoute))
        }
    }

    ModalNavigationDrawer(
        drawerState = drawer,
        gesturesEnabled = drawer.isOpen,
        drawerContent = { ModalDrawerSheet { DrawerContent(vm, send, host, close = { scope.launch { drawer.close() } }) } },
    ) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text(vm.headerTitleText ?: vm.headerTitle.text(), maxLines = 1, modifier = Modifier.testTag("header-title")) },
                    navigationIcon = {
                        IconButton(onClick = { scope.launch { drawer.open() } }, modifier = Modifier.testTag("menu")) {
                            Icon(Icons.Default.Menu, contentDescription = tk(Ns.APP, "Main navigation menu"))
                        }
                    },
                )
            },
        ) { padding ->
            Box(Modifier.fillMaxSize().padding(padding)) {
                Column(Modifier.fillMaxSize()) {
                    vm.sessionError?.let { error ->
                        Row(Modifier.fillMaxWidth().padding(12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                            Text(error.text(), color = MaterialTheme.colorScheme.error, modifier = Modifier.weight(1f).testTag("session-error"))
                            Button(onClick = { send(AppIntent.SessionRetried) }, modifier = Modifier.testTag("session-retry")) { Text(tk(Ns.APP, "Try Again")) }
                        }
                    }
                    Box(Modifier.weight(1f).fillMaxWidth()) {
                        when {
                            vm.transcript != null -> TranscriptScreen(vm, send, host)
                            all -> JobsScreen(vm, send, host)
                            else -> UploadScreen(vm, send, host)
                        }
                    }
                }
                if (vm.signingOut) CircularProgressIndicator(Modifier.align(Alignment.Center).testTag("signing-out"))
            }
        }
    }

    if (vm.settings?.open == true) SettingsDialog(vm, send, host)
    if (vm.feedback.open) FeedbackDialog(vm.feedback, send)
    if (vm.support.open) SupportDialog(vm.support, send)
}

@Composable
private fun DrawerContent(vm: SignedInVm, send: (AppIntent) -> Unit, host: ShellHost, close: () -> Unit) {
    fun go(intent: AppIntent) {
        close()
        send(intent)
    }
    LazyColumn(Modifier.fillMaxSize().testTag("drawer"), contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        item {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                Text(tk(Ns.COMMON, "mobile.navigation.mainSection"), style = MaterialTheme.typography.labelMedium)
                IconButton(onClick = close) { Icon(Icons.Default.Close, contentDescription = tk(Ns.COMMON, "Close")) }
            }
        }
        item { DrawerRow(tk(Ns.COMMON, "mobile.navigation.newTranscript"), "nav-new") { go(AppIntent.Navigate(vm.newTranscriptRoute)) } }
        item { DrawerRow(tk(Ns.COMMON, "mobile.navigation.allTranscripts"), "nav-all") { go(AppIntent.Navigate(Route.Home(HomeView.AllTranscripts))) } }
        item { HorizontalDivider(Modifier.padding(vertical = 8.dp)) }
        item { Text(tk(Ns.COMMON, "mobile.navigation.recentSection"), style = MaterialTheme.typography.labelMedium) }
        if (vm.jobs.recent.isEmpty()) {
            item { Text(tk(Ns.COMMON, "mobile.navigation.noRecentTranscripts"), color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        items(vm.jobs.recent, key = { it.id }) { row ->
            DrawerRow(
                row.fileName ?: vm.jobs.untitled.text(),
                "nav-recent-${row.id}",
                selected = row.selected,
            ) { go(AppIntent.Navigate(Route.Home(HomeView.Job(row.id)))) }
        }
        item { HorizontalDivider(Modifier.padding(vertical = 8.dp)) }
        item {
            Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(tk(Ns.COMMON, "mobile.navigation.bottomSheet.balance"))
                Text(vm.balance.text(), modifier = Modifier.testTag("balance"))
            }
        }
        item { DrawerRow(tk(Ns.COMMON, "mobile.navigation.bottomSheet.title"), "nav-settings") { go(AppIntent.Settings(SettingsIntent.Opened)) } }
        item { DrawerRow(tk(Ns.COMMON, "mobile.navigation.bottomSheet.support"), "nav-support") { go(AppIntent.Support(SupportIntent.Opened)) } }
        item { DrawerRow(tk(Ns.COMMON, "Billing History"), "nav-billing") { close(); host.openWeb(Route.Billing, vm.locale) } }
        item { DrawerRow(tk(Ns.COMMON, "API Keys"), "nav-api-keys") { close(); host.openWeb(Route.ApiKeys, vm.locale) } }
        item { DrawerRow(tk(Ns.COMMON, "mobile.navigation.bottomSheet.logout"), "nav-sign-out") { go(AppIntent.SignOutRequested) } }
    }
}

@Composable
private fun DrawerRow(label: String, tag: String, selected: Boolean = false, onClick: () -> Unit) {
    Text(
        label,
        maxLines = 1,
        color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
        style = MaterialTheme.typography.bodyLarge,
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 12.dp).testTag(tag),
    )
}
