package dev.transgate.android.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.transgate.core.AppIntent
import dev.transgate.core.ChatIntent
import dev.transgate.core.ChatLineVm
import dev.transgate.core.ChatPanelVm
import dev.transgate.core.ChatRating
import dev.transgate.core.ChatRole
import dev.transgate.core.LimitsVm
import dev.transgate.core.Ns
import dev.transgate.core.ResetLabel
import dev.transgate.core.SignedInVm
import dev.transgate.core.Tone
import dev.transgate.core.UsageVm
import kotlinx.coroutines.delay
import java.text.DateFormat
import java.util.Date

/** The Ask AI tab (WEB-069): the consent gate, then the streamed chat core describes. */
@Composable
fun ChatScreen(vm: SignedInVm, send: (AppIntent) -> Unit) {
    AiGate(vm.consent, AiFeature.CHAT, send) {
        val chat = vm.chat
        if (chat != null) ChatPanel(chat, send)
    }
}

@Composable
private fun ChatPanel(view: ChatPanelVm, send: (AppIntent) -> Unit) {
    fun chat(intent: ChatIntent) = send(AppIntent.Chat(intent))
    // The draft is UI mechanics only: it is the text field's content until it is sent.
    var draft by rememberSaveable { mutableStateOf("") }
    val list = rememberLazyListState()
    val last = view.lines.lastOrNull()

    // Follow the newest message as lines arrive and as the streamed answer grows.
    LaunchedEffect(view.lines.size, last?.content?.length, view.typing, view.error) {
        val total = list.layoutInfo.totalItemsCount
        if (total > 0) list.scrollToItem(total - 1)
    }
    // The copy-chat toast clears itself, as the web view's timer does.
    LaunchedEffect(view.notice) {
        if (view.notice != null) {
            delay(2000)
            chat(ChatIntent.NoticeDismissed)
        }
    }

    Column(Modifier.fillMaxSize().testTag("chat-panel")) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                tk(Ns.AI, if (view.showHistoryHeader) "Chat History" else "Ask AI"),
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.weight(1f).testTag("chat-title"),
            )
            if (view.showHistoryHeader) {
                TextButton(onClick = { chat(ChatIntent.CopyChatRequested) }, modifier = Modifier.heightIn(min = 48.dp).testTag("chat-copy")) {
                    Text(tk(Ns.AI, "Copy chat"))
                }
                TextButton(onClick = { chat(ChatIntent.DownloadRequested) }, modifier = Modifier.heightIn(min = 48.dp).testTag("chat-download")) {
                    Text(tk(Ns.AI, "Download chat"))
                }
            }
        }

        view.limits?.let { LimitsCard(it) }

        LazyColumn(
            Modifier.weight(1f).fillMaxWidth().testTag("chat-list"),
            state = list,
            contentPadding = PaddingValues(12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (view.loadingHistory) {
                item(key = "loading") {
                    Row(Modifier.testTag("chat-loading"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        CircularProgressIndicator(Modifier.padding(4.dp), strokeWidth = 2.dp)
                        Text(tk(Ns.COMMON, "Loading..."))
                    }
                }
            }
            if (view.showWelcome) {
                item(key = "welcome") {
                    Card(
                        Modifier.fillMaxWidth().testTag("chat-welcome"),
                        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    ) {
                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text(tk(Ns.AI, "Start a conversation"), style = MaterialTheme.typography.titleMedium)
                            Text(
                                tk(Ns.AI, "Ask questions about your transcript and get AI-powered insights"),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }
            items(view.lines.size, key = { it }) { index ->
                ChatMessage(view.lines[index], index.toULong(), ::chat)
            }
            if (view.typing) {
                item(key = "typing") {
                    Row(Modifier.testTag("chat-typing"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        CircularProgressIndicator(Modifier.padding(4.dp), strokeWidth = 2.dp)
                        Text(tk(Ns.AI, "AI is thinking..."), color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
            view.error?.let { error ->
                item(key = "error") {
                    Card(
                        Modifier.fillMaxWidth().testTag("chat-error"),
                        border = BorderStroke(1.dp, Tone.DANGER.color()),
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    ) {
                        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text(tk(Ns.AI, "Failed to get response"), style = MaterialTheme.typography.bodyMedium)
                            error.message?.let { Text(it.text(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                            error.detail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                            TextButton(onClick = { chat(ChatIntent.ErrorDismissed) }, modifier = Modifier.heightIn(min = 48.dp).testTag("chat-error-dismiss")) {
                                Text(tk(Ns.AI, "Dismiss"))
                            }
                        }
                    }
                }
            }
            view.syncWarning?.let { warning ->
                item(key = "sync-warning") {
                    Text(warning.text(), style = MaterialTheme.typography.bodySmall, modifier = Modifier.testTag("chat-sync-warning"))
                }
            }
        }

        view.notice?.let { notice ->
            Text(
                notice.text(),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp).testTag("chat-notice"),
            )
        }

        Row(Modifier.fillMaxWidth().padding(8.dp), verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                modifier = Modifier.weight(1f).heightIn(min = 56.dp).testTag("chat-input"),
                enabled = !view.inputLocked,
                maxLines = 4,
                placeholder = { Text(tk(Ns.AI, "Ask about the transcript...")) },
            )
            Button(
                onClick = {
                    val text = draft
                    draft = ""
                    chat(ChatIntent.SendRequested(text))
                },
                enabled = draft.isNotBlank() && view.canSend && !view.inputLocked,
                modifier = Modifier.heightIn(min = 56.dp).testTag("chat-send"),
            ) { Text(tk(Ns.COMMON, "Send message")) }
        }
    }
}

@Composable
private fun ChatMessage(line: ChatLineVm, index: ULong, chat: (ChatIntent) -> Unit) {
    val user = line.role == ChatRole.USER
    Column(Modifier.fillMaxWidth().testTag("chat-message-$index"), horizontalAlignment = if (user) Alignment.End else Alignment.Start) {
        Card(
            Modifier.widthIn(max = 320.dp),
            colors = CardDefaults.cardColors(
                containerColor = if (user) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.surface,
                contentColor = if (user) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSurface,
            ),
            border = if (user) null else BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        ) {
            Column(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                SelectionContainer { Text(line.content, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.testTag("chat-message-text-$index")) }
                if (line.streaming) {
                    CircularProgressIndicator(
                        Modifier.padding(top = 4.dp).testTag("chat-streaming-$index"),
                        strokeWidth = 2.dp,
                    )
                }
            }
        }
        if (line.showActions && !line.streaming) {
            Row(horizontalArrangement = Arrangement.spacedBy(0.dp), verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = { chat(ChatIntent.CopyMessageRequested(index)) }, modifier = Modifier.heightIn(min = 48.dp).testTag("chat-copy-$index")) {
                    Text(tk(Ns.AI, "Copy"))
                }
                TextButton(onClick = { chat(ChatIntent.RetryRequested(index)) }, modifier = Modifier.heightIn(min = 48.dp).testTag("chat-retry")) {
                    Text(tk(Ns.AI, "Retry"))
                }
                TextButton(onClick = { chat(ChatIntent.RateRequested(index, ChatRating.LIKE)) }, modifier = Modifier.heightIn(min = 48.dp).testTag("chat-like-$index")) {
                    Text(tk(Ns.AI, "Helpful"))
                }
                TextButton(onClick = { chat(ChatIntent.RateRequested(index, ChatRating.DISLIKE)) }, modifier = Modifier.heightIn(min = 48.dp).testTag("chat-dislike-$index")) {
                    Text(tk(Ns.AI, "Not helpful"))
                }
            }
        }
    }
}

@Composable
private fun LimitsCard(limits: LimitsVm) {
    Card(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp).testTag("chat-limits"),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(tk(Ns.AI, "Usage Limits"), style = MaterialTheme.typography.labelLarge)
            limits.questions?.let { Usage(tk(Ns.AI, "Questions"), it, "chat-limits-questions") }
            limits.tokens?.let { Usage(tk(Ns.AI, "Tokens"), it, "chat-limits-tokens") }
            limits.reset?.let { reset ->
                Text(resetText(reset), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.testTag("chat-limits-reset"))
            }
        }
    }
}

@Composable
private fun Usage(label: String, usage: UsageVm, tag: String) {
    Column(Modifier.testTag(tag), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(label, style = MaterialTheme.typography.bodySmall)
            Text("${usage.current} / ${usage.max}", style = MaterialTheme.typography.bodySmall)
        }
        LinearProgressIndicator(
            progress = { usage.widthPercent.toInt() / 100f },
            modifier = Modifier.fillMaxWidth(),
            color = usage.tone.color(),
        )
    }
}

@Composable
private fun resetText(reset: ResetLabel): String = when (reset) {
    ResetLabel.LessThanHour -> tk(Ns.AI, "Resets in less than 1 hour")
    is ResetLabel.InHours -> "${tk(Ns.AI, "Resets in")} ${reset.hours} ${tk(Ns.AI, if (reset.hours > 1) "hours" else "hour")}"
    is ResetLabel.InDays -> "${tk(Ns.AI, "Resets in")} ${reset.days} ${tk(Ns.AI, if (reset.days > 1) "days" else "day")}"
    is ResetLabel.On -> "${tk(Ns.AI, "Resets on")} ${DateFormat.getDateInstance().format(Date(reset.at / 1000))}"
}
