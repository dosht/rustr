package dev.transgate.android.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import dev.transgate.android.ShellHost
import dev.transgate.core.AppIntent
import dev.transgate.core.ConsentDecision
import dev.transgate.core.ConsentIntent
import dev.transgate.core.ConsentPanelVm
import dev.transgate.core.DataManagementVm
import dev.transgate.core.FeedbackIntent
import dev.transgate.core.FeedbackVm
import dev.transgate.core.Gate
import dev.transgate.core.ManageDataIntent
import dev.transgate.core.Msg
import dev.transgate.core.Ns
import dev.transgate.core.SettingsIntent
import dev.transgate.core.SignedInVm
import dev.transgate.core.SubscriptionVm
import dev.transgate.core.BillingMessages
import dev.transgate.core.SupportIntent
import dev.transgate.core.SupportVm
import dev.transgate.core.Tab
import java.text.DateFormat
import java.util.Date
import java.util.Locale

private fun kebab(name: String) = name.lowercase().replace('_', '-')

/** Epoch microseconds as a locale date: unit conversion only, as the web's `localDate`. */
@Composable
private fun localDate(micros: Long): String =
    DateFormat.getDateInstance(DateFormat.DEFAULT, Locale.forLanguageTag(LocalCopy.current.locale)).format(Date(micros / 1000))

@Composable
private fun ErrorText(message: Msg, tag: String) {
    Text(message.text(), color = MaterialTheme.colorScheme.error, modifier = Modifier.fillMaxWidth().testTag(tag))
}

@Composable
private fun LabeledValue(label: String, value: String, tag: String) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(label, fontWeight = FontWeight.Medium)
        Text(value, modifier = Modifier.testTag(tag))
    }
}

// ---------------------------------------------------------------- Settings

/** The settings dialog over the phone tab set core names in `settings.tabs` (WEB-073/075). */
@Composable
fun SettingsDialog(vm: SignedInVm, send: (AppIntent) -> Unit, host: ShellHost) {
    val settings = vm.settings ?: return
    val close = { send(AppIntent.Settings(SettingsIntent.Closed)) }
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize().testTag("settings-dialog")) {
            Column(Modifier.fillMaxSize()) {
                Row(
                    Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(settings.title.text(), style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f).testTag("settings-title"))
                    IconButton(onClick = close, modifier = Modifier.size(48.dp).testTag("settings-close")) {
                        Icon(Icons.Default.Close, contentDescription = tk(Ns.COMMON, "Close"))
                    }
                }
                Row(
                    Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    settings.tabs.forEach { tab ->
                        FilterChip(
                            selected = tab.active,
                            onClick = { send(AppIntent.Settings(SettingsIntent.TabSelected(tab.tab))) },
                            label = { Text(tab.label.text()) },
                            modifier = Modifier.heightIn(min = 48.dp).testTag("settings-tab-${kebab(tab.tab.name)}"),
                        )
                    }
                }
                Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    when (settings.active) {
                        Tab.PROFILE -> ProfileTab(settings.profile.email, vm.joinedOnText)
                        Tab.AI_FEATURES -> ConsentTab(vm.consent, send, host)
                        Tab.SUBSCRIPTION -> SubscriptionTab(vm.subscription, vm.billingMessages, vm.balance)
                        Tab.MANAGE_DATA -> ManageDataTab(vm.manageData, send)
                    }
                }
            }
        }
    }
}

@Composable
private fun ProfileTab(email: String, joinedOnText: String?) {
    LabeledValue(tk(Ns.COMMON, "Email: "), email, "profile-email")
    LabeledValue(tk(Ns.COMMON, "Joined on: "), joinedOnText ?: "", "profile-joined-on")
}

// ---------------------------------------------------------------- AI features

@Composable
private fun ConsentTab(consent: ConsentPanelVm, send: (AppIntent) -> Unit, host: ShellHost) {
    fun act(intent: ConsentIntent) = send(AppIntent.Consent(intent))
    var confirming by remember { mutableStateOf(false) }
    LaunchedEffect(consent.hasConsented) { if (!consent.hasConsented) confirming = false }

    if (consent.gate == Gate.LOADING) {
        Text(tk(Ns.COMMON, "Loading..."), modifier = Modifier.testTag("consent-loading"))
        return
    }
    consent.loadError?.let {
        ErrorText(it, "consent-load-error")
        return
    }

    Column(verticalArrangement = Arrangement.spacedBy(12.dp), modifier = Modifier.testTag("ai-consent-settings")) {
        Column {
            Text(
                if (consent.hasConsented) tk(Ns.AI, "AI Features Enabled") else tk(Ns.AI, "AI Features Disabled"),
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.testTag("consent-status"),
            )
            Text(
                if (consent.hasConsented) tk(Ns.AI, "Summaries, highlights, and AI chat are available")
                else tk(Ns.AI, "Enable AI features to access summaries, highlights, and chat"),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (consent.cleanupPending) {
            Text(tk(Ns.APP, "p5.consent.cleanup_pending"), modifier = Modifier.testTag("consent-cleanup-pending"))
        }
        consent.providerName?.let { provider ->
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                consent.consentDate?.let { LabeledValue(tk(Ns.AI, "Consent given:"), localDate(it), "consent-date") }
                consent.revokedDate?.let { LabeledValue(tk(Ns.AI, "Consent revoked:"), localDate(it), "consent-revoked-date") }
                LabeledValue(tk(Ns.AI, "AI Provider:"), provider, "consent-provider")
                consent.consentVersion?.let { LabeledValue(tk(Ns.AI, "Consent Version:"), it, "consent-version") }
            }
        }

        if (!consent.hasConsented) {
            val enabling = consent.busy == ConsentDecision.ENABLE
            Button(
                onClick = { act(ConsentIntent.ENABLE_REQUESTED) },
                enabled = consent.busy == null,
                modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("consent-enable"),
            ) {
                Text(if (enabling) tk(Ns.AI, "Enabling...") else tk(Ns.AI, "Enable AI Features"))
            }
        } else if (!confirming) {
            Button(
                onClick = { confirming = true },
                enabled = consent.canRevoke && consent.busy == null,
                colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error, contentColor = MaterialTheme.colorScheme.onError),
                modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("consent-turn-off"),
            ) {
                Text(tk(Ns.APP, "p5.consent.turn_off_delete_action"))
            }
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.testTag("consent-confirm")) {
                Text(tk(Ns.APP, "p5.consent.turn_off_delete_title"), style = MaterialTheme.typography.titleSmall)
                Text(tk(Ns.APP, "p5.consent.turn_off_delete_confirm"))
                Button(
                    onClick = { act(ConsentIntent.DISABLE_CONFIRMED) },
                    colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error, contentColor = MaterialTheme.colorScheme.onError),
                    modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("consent-disable-confirm"),
                ) {
                    Text(tk(Ns.APP, "p5.consent.turn_off_delete_button"))
                }
                OutlinedButton(
                    onClick = { confirming = false },
                    enabled = consent.busy == null,
                    modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("consent-disable-cancel"),
                ) {
                    Text(tk(Ns.AI, "Cancel"))
                }
            }
        }

        consent.actionError?.let { ErrorText(it, "consent-action-error") }

        Text(tk(Ns.AI, "For more information about how we handle your data, please review our"), style = MaterialTheme.typography.bodySmall)
        TextButton(
            onClick = { host.open(consent.privacyPolicyUrl) },
            modifier = Modifier.heightIn(min = 48.dp).testTag("privacy-policy-link"),
        ) {
            Text(tk(Ns.AI, "Privacy Policy"))
        }
    }
}

// ---------------------------------------------------------------- Subscription (balance only, D6)

@Composable
private fun SubscriptionTab(sub: SubscriptionVm, messages: BillingMessages, balance: Msg) {
    LabeledValue(tk(Ns.COMMON, "mobile.navigation.bottomSheet.balance"), balance.text(), "subscription-balance")
    HorizontalDivider()
    val plan = sub.plan
    when {
        sub.loading -> Text(tk(Ns.COMMON, "Loading subscription information..."), modifier = Modifier.testTag("subscription-loading"))
        plan != null -> {
            LabeledValue(tk(Ns.COMMON, "Current Plan:"), plan.name, "subscription-plan")
            LabeledValue(tk(Ns.COMMON, "Total Minutes:"), plan.minutesIncluded.toString(), "subscription-minutes")
            LabeledValue(tk(Ns.COMMON, "Plan Valid Until:"), localDate(plan.validUntil), "subscription-valid-until")
            LabeledValue(tk(Ns.COMMON, "Next Payment:"), sub.nextPayment?.let { localDate(it) } ?: "", "subscription-next-payment")
            if (sub.canceled) {
                Text(tk(Ns.COMMON, "Your subscription is canceled, but you can still use your minutes."), modifier = Modifier.testTag("subscription-canceled"))
            }
        }
        else -> Text(tk(Ns.COMMON, " No Package"), modifier = Modifier.testTag("subscription-no-package"))
    }
    messages.subscriptionFailure?.let { ErrorText(it, "subscription-failure") }
}

// ---------------------------------------------------------------- Manage data

@Composable
private fun SectionTitle(text: String) = Text(text, style = MaterialTheme.typography.titleMedium)

@Composable
private fun SwitchRow(label: String, description: String?, checked: Boolean, tag: String, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp).selectable(selected = checked, role = Role.Switch, onClick = { onChange(!checked) }),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(label)
            description?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        Switch(checked = checked, onCheckedChange = null, modifier = Modifier.testTag(tag))
    }
}

@Composable
private fun ManageDataTab(vm: DataManagementVm, send: (AppIntent) -> Unit) {
    fun act(intent: ManageDataIntent) = send(AppIntent.ManageData(intent))
    val danger = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error, contentColor = MaterialTheme.colorScheme.onError)

    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(tk(Ns.COMMON, "Privacy Information:"))
        vm.privacyInfo.forEach { Text("\u2022 ${it.text()}", style = MaterialTheme.typography.bodyMedium) }
    }

    SectionTitle(tk(Ns.COMMON, "Data Retention"))
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(tk(Ns.COMMON, "Automatic Transcript Cleanup Policy"), style = MaterialTheme.typography.titleSmall)
        Text(vm.cleanupPolicy.text(), modifier = Modifier.testTag("manage-cleanup-policy"))
        vm.cleanupWarning?.let { Text(it.text(), color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("manage-cleanup-warning")) }
    }
    SwitchRow(
        label = tk(Ns.COMMON, "Automatic transcript cleanup (30 days)"),
        description = tk(Ns.COMMON, "Automatically delete transcripts older than 30 days to save storage. You can disable this to keep transcripts indefinitely.") +
            if (vm.cleanupOn) {
                tk(Ns.COMMON, "Transcripts older than 30 days will be automatically deleted daily. Disable to keep transcripts indefinitely.")
            } else {
                tk(Ns.COMMON, "Automatic cleanup disabled. Your transcripts will be kept indefinitely, which may increase storage usage.")
            },
        checked = vm.cleanupOn,
        tag = "manage-cleanup",
    ) { act(ManageDataIntent.CleanupToggled(it)) }

    SectionTitle(tk(Ns.COMMON, "Legacy Settings"))
    SwitchRow(
        label = tk(Ns.COMMON, "Delete my data automatically"),
        description = null,
        checked = vm.deleteMonthlyOn,
        tag = "manage-delete-monthly",
    ) { act(ManageDataIntent.DeleteMonthlyToggled(it)) }

    SectionTitle(tk(Ns.COMMON, "Manual Actions"))
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(tk(Ns.COMMON, "Delete all Transcripted Files"), Modifier.weight(1f))
        Button(onClick = { act(ManageDataIntent.DeleteAllPressed) }, colors = danger, modifier = Modifier.heightIn(min = 48.dp).testTag("manage-delete-all")) {
            Text(tk(Ns.COMMON, "Delete All"))
        }
    }
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(tk(Ns.COMMON, "Delete my Account"), Modifier.weight(1f))
        Button(onClick = { act(ManageDataIntent.DeleteAccountPressed) }, colors = danger, modifier = Modifier.heightIn(min = 48.dp).testTag("manage-delete-account")) {
            Text(tk(Ns.COMMON, "Delete Account"))
        }
    }
    Text(vm.filesGoneNote.text(), color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("manage-files-gone-note"))

    vm.erasure?.let { Text(it.text(), color = MaterialTheme.colorScheme.primary, modifier = Modifier.fillMaxWidth().testTag("manage-erasure")) }
    vm.error?.let { ErrorText(it, "manage-error") }
    vm.saveError?.let { ErrorText(it, "manage-save-error") }

    vm.confirm?.let { confirm ->
        AlertDialog(
            onDismissRequest = { act(ManageDataIntent.Canceled) },
            title = { Text(confirm.title.text(), modifier = Modifier.testTag("manage-confirm-title")) },
            text = { Text(confirm.description.text(), modifier = Modifier.testTag("manage-confirm-description")) },
            confirmButton = {
                TextButton(onClick = { act(ManageDataIntent.Confirmed) }, modifier = Modifier.heightIn(min = 48.dp).testTag("manage-confirm-yes")) {
                    Text(confirm.confirmLabel.text())
                }
            },
            dismissButton = {
                TextButton(onClick = { act(ManageDataIntent.Canceled) }, modifier = Modifier.heightIn(min = 48.dp).testTag("manage-confirm-no")) {
                    Text(tk(Ns.COMMON, "No"))
                }
            },
            modifier = Modifier.testTag("manage-confirm"),
        )
    }
}

// ---------------------------------------------------------------- Feedback

/** The job feedback dialog (`FeedbackDialog.svelte`): rating, comment, submit, then the thank-you body. */
@Composable
fun FeedbackDialog(vm: FeedbackVm, send: (AppIntent) -> Unit) {
    fun act(intent: FeedbackIntent) = send(AppIntent.Feedback(intent))
    AlertDialog(
        onDismissRequest = { act(FeedbackIntent.Closed) },
        properties = DialogProperties(dismissOnBackPress = !vm.submitting, dismissOnClickOutside = !vm.submitting),
        title = { Text(if (vm.thanked) tk(Ns.COMMON, "Thank You!") else tk(Ns.COMMON, "Rate Your Experience"), modifier = Modifier.testTag("feedback-title")) },
        text = {
            if (vm.thanked) {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.testTag("feedback-thanks")) {
                    Text(tk(Ns.COMMON, "Thank you for your feedback!"), style = MaterialTheme.typography.bodyLarge)
                    Text(tk(Ns.COMMON, "Your response helps us improve our service."), color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            } else {
                Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(tk(Ns.COMMON, "How was your experience?"), color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        vm.options.forEach { option ->
                            FilterChip(
                                selected = option.selected,
                                onClick = { act(FeedbackIntent.Selected(option.rating)) },
                                enabled = !vm.submitting,
                                label = { Text(option.label.text(), maxLines = 2) },
                                modifier = Modifier.weight(1f).heightIn(min = 48.dp).testTag("feedback-rating-${kebab(option.rating.name)}"),
                            )
                        }
                    }
                    OutlinedTextField(
                        value = vm.comment,
                        onValueChange = { act(FeedbackIntent.CommentChanged(it)) },
                        label = { Text(tk(Ns.COMMON, "Tell us more about your experience")) },
                        placeholder = { Text(tk(Ns.COMMON, "Share your thoughts...")) },
                        enabled = !vm.submitting,
                        minLines = 4,
                        modifier = Modifier.fillMaxWidth().testTag("feedback-comment"),
                    )
                    vm.notice?.let { ErrorText(it, "feedback-notice") }
                }
            }
        },
        confirmButton = {
            if (vm.thanked) {
                Button(onClick = { act(FeedbackIntent.Closed) }, modifier = Modifier.heightIn(min = 48.dp).testTag("feedback-close")) {
                    Text(tk(Ns.COMMON, "Close"))
                }
            } else {
                Button(
                    onClick = { act(FeedbackIntent.Submitted) },
                    enabled = vm.canSubmit && !vm.submitting,
                    modifier = Modifier.heightIn(min = 48.dp).testTag("feedback-submit"),
                ) {
                    if (vm.submitting) CircularProgressIndicator(Modifier.size(18.dp).testTag("feedback-busy"), strokeWidth = 2.dp)
                    Text(
                        if (vm.submitting) tk(Ns.COMMON, "Submitting...") else tk(Ns.COMMON, "Submit"),
                        modifier = Modifier.padding(start = if (vm.submitting) 8.dp else 0.dp),
                    )
                }
            }
        },
        dismissButton = {
            if (!vm.thanked) {
                TextButton(
                    onClick = { act(FeedbackIntent.Closed) },
                    enabled = !vm.submitting,
                    modifier = Modifier.heightIn(min = 48.dp).testTag("feedback-cancel"),
                ) {
                    Text(tk(Ns.COMMON, "Cancel"))
                }
            }
        },
        modifier = Modifier.testTag("feedback-dialog"),
    )
}

// ---------------------------------------------------------------- Support

/** The support message dialog (`SupportDialog.svelte`); a failed send keeps the message and shows core's notice. */
@Composable
fun SupportDialog(vm: SupportVm, send: (AppIntent) -> Unit) {
    fun act(intent: SupportIntent) = send(AppIntent.Support(intent))
    AlertDialog(
        onDismissRequest = { act(SupportIntent.Closed) },
        properties = DialogProperties(dismissOnBackPress = !vm.sending, dismissOnClickOutside = !vm.sending),
        title = { Text(tk(Ns.COMMON, "Support"), modifier = Modifier.testTag("support-title")) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(
                    value = vm.message,
                    onValueChange = { act(SupportIntent.MessageChanged(it)) },
                    label = { Text(tk(Ns.COMMON, "Message")) },
                    placeholder = { Text(tk(Ns.COMMON, "Enter your message here")) },
                    enabled = !vm.sending,
                    minLines = 6,
                    modifier = Modifier.fillMaxWidth().testTag("support-message"),
                )
                vm.notice?.let { ErrorText(it, "support-notice") }
            }
        },
        confirmButton = {
            Button(
                onClick = { act(SupportIntent.Submitted) },
                enabled = !vm.sending,
                modifier = Modifier.heightIn(min = 48.dp).testTag("support-send"),
            ) {
                if (vm.sending) CircularProgressIndicator(Modifier.size(18.dp).testTag("support-busy"), strokeWidth = 2.dp)
                Text(tk(Ns.COMMON, "Send"), modifier = Modifier.padding(start = if (vm.sending) 8.dp else 0.dp))
            }
        },
        dismissButton = {
            TextButton(
                onClick = { act(SupportIntent.Closed) },
                enabled = !vm.sending,
                modifier = Modifier.heightIn(min = 48.dp).testTag("support-close"),
            ) {
                Text(tk(Ns.COMMON, "Close"))
            }
        },
        modifier = Modifier.testTag("support-dialog"),
    )
}
