package dev.transgate.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import dev.transgate.android.ShellHost
import dev.transgate.core.AppIntent
import dev.transgate.core.ArgWire
import dev.transgate.core.AuthIntent
import dev.transgate.core.AuthMode
import dev.transgate.core.AuthVm
import dev.transgate.core.AppVm
import dev.transgate.core.LocaleOption
import dev.transgate.core.NewPasswordIntent
import dev.transgate.core.NewPasswordVm
import dev.transgate.core.Ns
import dev.transgate.core.Route
import dev.transgate.core.SignedOutVm
import dev.transgate.core.Tone

/** The `auth` catalog section of a mode: the web view builds `${mode}.label` the same way. */
private fun AuthMode.section(): String = name.lowercase()

private fun AuthMode.tag(): String = name.lowercase().replace('_', '-')

private fun AuthVm.key(suffix: String): String = "${mode.section()}.$suffix"

/**
 * Sign in, sign up, magic link, recovery (WEB-051) and the recovery link's new-password
 * form. Core's [AuthVm] decides mode, busy, can-submit, errors and notices; the Google
 * button only sends the intent, core asks the shell to open the Custom Tab.
 */
@Composable
fun AuthScreen(vm: SignedOutVm, send: (AppIntent) -> Unit, host: ShellHost) {
    Box(Modifier.fillMaxSize().imePadding(), contentAlignment = Alignment.Center) {
        Column(
            Modifier.fillMaxWidth().widthIn(max = 420.dp).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            LocalePicker(vm.locales, send)
            if (vm.route is Route.NewPassword) {
                NewPasswordForm(vm.newPassword) { send(AppIntent.NewPassword(it)) }
            } else {
                SignInForm(vm.auth) { send(AppIntent.Auth(it)) }
            }
        }
    }
}

@Composable
private fun LocalePicker(locales: List<LocaleOption>, send: (AppIntent) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val current = locales.firstOrNull { it.current }
    Box(Modifier.fillMaxWidth()) {
        OutlinedButton(
            onClick = { open = true },
            modifier = Modifier.align(Alignment.CenterEnd).heightIn(min = 48.dp).testTag("locale-picker"),
        ) {
            Text(current?.let { "${it.flag} ${it.name.text()}" } ?: tk(Ns.COMMON, "Change Language"))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            locales.forEach { option ->
                DropdownMenuItem(
                    text = { Text("${option.flag} ${option.name.text()}") },
                    onClick = {
                        open = false
                        send(AppIntent.LocaleChosen(option.code))
                    },
                    modifier = Modifier.heightIn(min = 48.dp).testTag("locale-${option.code}"),
                )
            }
        }
    }
}

@Composable
private fun SignInForm(vm: AuthVm, onIntent: (AuthIntent) -> Unit) {
    val social = vm.mode == AuthMode.SIGN_IN || vm.mode == AuthMode.SIGN_UP
    val withPassword = social
    val emailLabelKey = if (vm.mode == AuthMode.MAGIC_LINK) vm.key("email_input_label") else vm.key("email_label")
    val links: List<AuthMode> = when (vm.mode) {
        AuthMode.SIGN_IN -> listOf(AuthMode.MAGIC_LINK, AuthMode.FORGOTTEN_PASSWORD, AuthMode.SIGN_UP)
        AuthMode.SIGN_UP -> listOf(AuthMode.MAGIC_LINK, AuthMode.SIGN_IN)
        AuthMode.MAGIC_LINK, AuthMode.FORGOTTEN_PASSWORD -> listOf(AuthMode.SIGN_IN)
    }

    if (social) {
        OutlinedButton(
            onClick = { onIntent(AuthIntent.GoogleChosen) },
            enabled = !vm.busy,
            modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("auth-google"),
        ) {
            Text(tk(Ns.AUTH, vm.key("social_provider_text"), "provider" to ArgWire.Text("Google")))
        }
        HorizontalDivider()
    }

    OutlinedTextField(
        value = vm.email,
        onValueChange = { onIntent(AuthIntent.EmailChanged(it)) },
        label = { Text(tk(Ns.AUTH, emailLabelKey)) },
        placeholder = { Text(tk(Ns.AUTH, vm.key("email_input_placeholder"))) },
        singleLine = true,
        enabled = !vm.busy,
        keyboardOptions = KeyboardOptions(
            keyboardType = KeyboardType.Email,
            imeAction = if (withPassword) ImeAction.Next else ImeAction.Done,
        ),
        keyboardActions = KeyboardActions(onDone = { onIntent(AuthIntent.Submitted) }),
        modifier = Modifier.fillMaxWidth().testTag("auth-email"),
    )
    if (withPassword) {
        OutlinedTextField(
            value = vm.password,
            onValueChange = { onIntent(AuthIntent.PasswordChanged(it)) },
            label = { Text(tk(Ns.AUTH, vm.key("password_label"))) },
            placeholder = { Text(tk(Ns.AUTH, vm.key("password_input_placeholder"))) },
            singleLine = true,
            enabled = !vm.busy,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { onIntent(AuthIntent.Submitted) }),
            modifier = Modifier.fillMaxWidth().testTag("auth-password"),
        )
    }
    Button(
        onClick = { onIntent(AuthIntent.Submitted) },
        enabled = vm.canSubmit && !vm.busy,
        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("auth-submit"),
    ) {
        if (vm.busy) {
            CircularProgressIndicator(Modifier.size(20.dp).testTag("auth-busy"), strokeWidth = 2.dp)
            Text(tk(Ns.AUTH, vm.key("loading_button_label")), modifier = Modifier.padding(start = 8.dp))
        } else {
            Text(tk(Ns.AUTH, vm.key("button_label")))
        }
    }
    vm.error?.let { Text(it.text(), color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("auth-error")) }
    vm.notice?.let { Text(it.text(), color = Tone.SUCCESS.color(), modifier = Modifier.testTag("auth-notice")) }

    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        links.forEach { mode ->
            val linkKey = "${mode.section()}.link_text"
            TextButton(
                onClick = { onIntent(AuthIntent.ModeChosen(mode)) },
                modifier = Modifier.heightIn(min = 48.dp).testTag("auth-mode-${mode.tag()}"),
            ) {
                Text(tk(Ns.AUTH, linkKey))
            }
        }
    }
}

@Composable
private fun NewPasswordForm(vm: NewPasswordVm, onIntent: (NewPasswordIntent) -> Unit) {
    OutlinedTextField(
        value = vm.password,
        onValueChange = { onIntent(NewPasswordIntent.PasswordChanged(it)) },
        label = { Text(tk(Ns.COMMON, "New Password")) },
        placeholder = { Text(tk(Ns.COMMON, "New Password")) },
        singleLine = true,
        enabled = !vm.busy,
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = { onIntent(NewPasswordIntent.Submitted) }),
        modifier = Modifier.fillMaxWidth().testTag("new-password-input"),
    )
    vm.error?.let { Text(it.text(), color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("new-password-error")) }
    vm.success?.let { Text(it.text(), color = Tone.SUCCESS.color(), modifier = Modifier.testTag("new-password-success")) }
    Button(
        onClick = { onIntent(NewPasswordIntent.Submitted) },
        enabled = !vm.busy,
        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("new-password-submit"),
    ) {
        if (vm.busy) CircularProgressIndicator(Modifier.size(20.dp).testTag("new-password-busy"), strokeWidth = 2.dp)
        Text(tk(Ns.COMMON, "Change Password"), modifier = Modifier.padding(start = if (vm.busy) 8.dp else 0.dp))
    }
}

/**
 * The checkout result page (`ConfirmPage.svelte`): core's translated [AppVm.CheckoutConfirm.messages]
 * or a loading label while they are not yet known. No purchase action exists natively (D6).
 */
@Composable
fun CheckoutConfirmScreen(view: AppVm.CheckoutConfirm, send: (AppIntent) -> Unit) {
    val messages = view.messages
    Box(Modifier.fillMaxSize().padding(16.dp), contentAlignment = Alignment.Center) {
        Column(
            Modifier.fillMaxWidth().widthIn(max = 480.dp).verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (messages == null) {
                CircularProgressIndicator(Modifier.testTag("confirm-loading"))
                Text(tk(Ns.COMMON, "Loading..."))
            } else {
                Text(messages.title.text(), style = MaterialTheme.typography.headlineSmall, modifier = Modifier.testTag("confirm-title"))
                messages.status?.let { Text(it.text(), style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("confirm-status")) }
                Text(messages.message.text(), modifier = Modifier.testTag("confirm-message"))
                messages.detail?.let { Text(it.text(), modifier = Modifier.testTag("confirm-detail")) }
                messages.redirect?.let {
                    Text(it.text(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.testTag("confirm-redirect"))
                }
            }
        }
    }
}
