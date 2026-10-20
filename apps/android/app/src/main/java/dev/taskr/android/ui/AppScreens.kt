package dev.taskr.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import dev.taskr.core.AppIntent
import dev.taskr.core.AppView
import dev.taskr.core.MemberRole
import dev.taskr.core.MembersIntent
import dev.taskr.core.MembersView
import dev.taskr.core.ScreenView
import dev.taskr.core.SignInView
import dev.taskr.core.SignedInView
import kotlinx.coroutines.launch

/** The signed-in app. Which screen shows comes from Rust; how it looks is Compose's call. */
@Composable
fun AppScreen(view: AppView, devSignIn: Boolean, onIntent: (AppIntent) -> Unit) {
    when (view) {
        is AppView.Starting -> Box(Modifier.fillMaxSize()) {
            CircularProgressIndicator(Modifier.align(Alignment.Center))
        }
        is AppView.SignedOut -> SignInScreen(view.view, devSignIn, onIntent)
        is AppView.SignedIn -> AppFrame(view.view, onIntent)
    }
}

@Composable
fun SignInScreen(view: SignInView, devSignIn: Boolean, onIntent: (AppIntent) -> Unit) {
    var email by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    Column(
        Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("taskr", style = MaterialTheme.typography.headlineMedium, color = MaterialTheme.colorScheme.primary)
        OutlinedTextField(
            value = email,
            onValueChange = { email = it },
            label = { Text("Email") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
            modifier = Modifier.widthIn(max = 400.dp).fillMaxWidth(),
        )
        if (!devSignIn) {
            OutlinedTextField(
                value = password,
                onValueChange = { password = it },
                label = { Text("Password") },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                modifier = Modifier.widthIn(max = 400.dp).fillMaxWidth(),
            )
        }
        val message = view.error ?: view.notice
        if (message != null) {
            MessageBanner(message, isError = view.error != null) { onIntent(AppIntent.MessageDismissed) }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(enabled = !view.busy, onClick = { onIntent(AppIntent.SignInSubmitted(email, password)) }) {
                Text("Sign in")
            }
            if (!devSignIn) {
                OutlinedButton(enabled = !view.busy, onClick = { onIntent(AppIntent.SignUpSubmitted(email, password)) }) {
                    Text("Create account")
                }
            }
        }
        if (devSignIn) {
            Text(
                "Dev sign-in: any email, no password.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AppFrame(view: SignedInView, onIntent: (AppIntent) -> Unit) {
    val drawer = rememberDrawerState(initialValue = androidx.compose.material3.DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val go = { intent: AppIntent ->
        scope.launch { drawer.close() }
        onIntent(intent)
    }
    ModalNavigationDrawer(
        drawerState = drawer,
        drawerContent = {
            ModalDrawerSheet {
                Text("Workspaces", style = MaterialTheme.typography.labelMedium, modifier = Modifier.padding(16.dp))
                view.workspaces.forEach { w ->
                    NavigationDrawerItem(
                        label = { Text(w.name) },
                        badge = { Text(w.roleLabel, style = MaterialTheme.typography.labelSmall) },
                        selected = w.selected,
                        onClick = { go(AppIntent.WorkspaceSelected(w.id)) },
                        modifier = Modifier.padding(horizontal = 12.dp),
                    )
                }
                HorizontalDivider(Modifier.padding(vertical = 8.dp))
                view.screens.forEach { s ->
                    NavigationDrawerItem(
                        label = { Text(s.label) },
                        selected = s.selected,
                        onClick = { go(AppIntent.ScreenSelected(s.screen)) },
                        modifier = Modifier.padding(horizontal = 12.dp),
                    )
                }
                Spacer(Modifier.weight(1f))
                Text(view.email, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 16.dp))
                TextButton(onClick = { go(AppIntent.SignOutRequested) }, modifier = Modifier.padding(8.dp)) {
                    Text("Sign out")
                }
            }
        },
    ) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text(view.title) },
                    navigationIcon = {
                        IconButton(onClick = { scope.launch { drawer.open() } }) { Text("☰") }
                    },
                )
            },
        ) { padding ->
            Column(Modifier.padding(padding).fillMaxSize()) {
                view.error?.let { MessageBanner(it, isError = true) { onIntent(AppIntent.MessageDismissed) } }
                when (val screen = view.screen) {
                    is ScreenView.Loading -> Box(Modifier.fillMaxSize()) {
                        CircularProgressIndicator(Modifier.align(Alignment.Center))
                    }
                    is ScreenView.Tasks -> TaskListContent(screen.view) { intent ->
                        view.workspace?.let { onIntent(AppIntent.Tasks(it, intent)) }
                    }
                    is ScreenView.Members -> MembersContent(screen.view) { intent ->
                        view.workspace?.let { onIntent(AppIntent.Members(it, intent)) }
                    }
                }
            }
        }
    }
}

/** Pure view: renders a MembersView. What an owner can do comes from `canManage`. */
@Composable
fun MembersContent(view: MembersView, onIntent: (MembersIntent) -> Unit) {
    var email by rememberSaveable { mutableStateOf("") }
    var role by remember { mutableStateOf(MemberRole.EDITOR) }
    Column(Modifier.fillMaxSize()) {
        if (view.canManage) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OutlinedTextField(
                    value = email,
                    onValueChange = { email = it },
                    label = { Text("Email to add") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                    modifier = Modifier.weight(1f),
                )
                RolePicker(role, view) { role = it }
                Button(onClick = {
                    onIntent(MembersIntent.AddSubmitted(email, role))
                    email = ""
                }) { Text("Add") }
            }
        }
        val message = view.error ?: view.notice
        if (message != null) {
            MessageBanner(message, isError = view.error != null) { onIntent(MembersIntent.MessageDismissed) }
        }
        LazyColumn(Modifier.fillMaxSize()) {
            items(view.rows, key = { it.email }) { row ->
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        if (row.isYou) "${row.email} (you)" else row.email,
                        modifier = Modifier.weight(1f),
                    )
                    if (view.canManage) {
                        RolePicker(row.role, view) { onIntent(MembersIntent.RoleChanged(row.email, it)) }
                        TextButton(onClick = { onIntent(MembersIntent.RemoveRequested(row.email)) }) { Text("Remove") }
                    } else {
                        Text(row.roleLabel, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                HorizontalDivider()
            }
        }
    }
}

@Composable
private fun RolePicker(selected: MemberRole, view: MembersView, onPick: (MemberRole) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { open = true }) {
            Text(view.roleOptions.firstOrNull { it.role == selected }?.label ?: selected.name)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            view.roleOptions.forEach { option ->
                DropdownMenuItem(
                    text = { Text(option.label) },
                    onClick = {
                        open = false
                        onPick(option.role)
                    },
                )
            }
        }
    }
}
