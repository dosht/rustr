package dev.transgate.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.transgate.android.ShellHost
import dev.transgate.core.AnalysisIntent
import dev.transgate.core.AppIntent
import dev.transgate.core.FeedbackIntent
import dev.transgate.core.Media
import dev.transgate.core.Ns
import dev.transgate.core.SignedInVm
import dev.transgate.core.Tone
import dev.transgate.core.TranscriptIntent
import dev.transgate.core.TranscriptVm

private const val TAB_LINES = 0
private const val TAB_ANALYSIS = 1
private const val TAB_CHAT = 2

/**
 * WEB-063 / WEB-059 / WEB-067: renders `vm.transcript` (the phone arrangement of the web's
 * `TranscriptContent` + `MobileTranscript`). Every rule is core's; what is local is the selected
 * tab, the sheet flag and the playhead mechanics the web container also keeps in the view.
 */
@Composable
fun TranscriptScreen(vm: SignedInVm, send: (AppIntent) -> Unit, host: ShellHost) {
    val transcript = vm.transcript ?: return
    val media = transcript.media

    LaunchedEffect(media) {
        when (media) {
            Media.None -> {
                host.player.mount(null)
                host.player.mountYouTube(null)
            }
            is Media.Audio -> {
                host.player.mountYouTube(null)
                host.player.mount(media.url)
            }
            is Media.YouTube -> {
                host.player.mount(null)
                host.player.mountYouTube(media.videoId)
            }
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            host.player.mount(null)
            host.player.mountYouTube(null)
        }
    }

    // `activeLineIndex` of the web player: the first row whose end has not passed; none at time 0.
    val activeIndex by remember(transcript.rows, host.player) {
        derivedStateOf {
            val time = host.player.seconds
            if (time <= 0) -1 else transcript.rows.indexOfFirst { time <= it.endSeconds }
        }
    }

    val hasAnalysis = vm.capabilities.ai && vm.analysis != null
    val hasChat = vm.capabilities.aiChat && vm.chat != null
    var requested by rememberSaveable(transcript.job) { mutableIntStateOf(TAB_LINES) }
    val tab = when {
        requested == TAB_ANALYSIS && hasAnalysis -> TAB_ANALYSIS
        requested == TAB_CHAT && hasChat -> TAB_CHAT
        else -> TAB_LINES
    }
    DisposableEffect(tab == TAB_ANALYSIS) {
        if (tab == TAB_ANALYSIS) send(AppIntent.Analysis(AnalysisIntent.Shown))
        onDispose { if (tab == TAB_ANALYSIS) send(AppIntent.Analysis(AnalysisIntent.Hidden)) }
    }

    Column(Modifier.fillMaxSize().testTag("transcript")) {
        TranscriptHeader(transcript)
        transcript.noAudioAlert?.let { alert ->
            Row(
                Modifier.fillMaxWidth().background(LocalSemantic.current.errorBg).padding(12.dp).testTag("no-audio-alert"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(alert.text(), Modifier.weight(1f), color = LocalSemantic.current.error)
                TextButton(onClick = { send(AppIntent.Transcript(TranscriptIntent.NoAudioAlertDismissed)) }, Modifier.heightIn(min = 48.dp).testTag("no-audio-alert-dismiss")) {
                    Text(tk(Ns.COMMON, "Close"))
                }
            }
        }
        TranscriptActions(transcript, vm.transcript?.job, send)
        if (hasAnalysis || hasChat) {
            TabRow(selectedTabIndex = listOfNotNull(TAB_LINES, TAB_ANALYSIS.takeIf { hasAnalysis }, TAB_CHAT.takeIf { hasChat }).indexOf(tab)) {
                Tab(selected = tab == TAB_LINES, onClick = { requested = TAB_LINES }, modifier = Modifier.heightIn(min = 48.dp).testTag("tab-lines"), text = { Text(transcript.linesTabLabel.text()) })
                if (hasAnalysis) Tab(selected = tab == TAB_ANALYSIS, onClick = { requested = TAB_ANALYSIS }, modifier = Modifier.heightIn(min = 48.dp).testTag("tab-analysis"), text = { Text(tk(Ns.AI, "AI Analysis")) })
                if (hasChat) Tab(selected = tab == TAB_CHAT, onClick = { requested = TAB_CHAT }, modifier = Modifier.heightIn(min = 48.dp).testTag("tab-chat"), text = { Text(tk(Ns.AI, "Ask AI")) })
            }
        }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            when (tab) {
                TAB_ANALYSIS -> AnalysisScreen(vm, send)
                TAB_CHAT -> ChatScreen(vm, send)
                else -> TranscriptLinesPanel(transcript, activeIndex, send)
            }
        }
        if (media is Media.Audio) {
            HorizontalDivider()
            AudioControls(media, host)
        }
    }
}

@Composable
private fun TranscriptHeader(vm: TranscriptVm) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Column(Modifier.weight(1f)) {
            Text(vm.title, style = MaterialTheme.typography.titleLarge, maxLines = 2, modifier = Modifier.testTag("transcript-title"))
            vm.durationMinutes?.let { Text("$it ${vm.minutesUnit.text()}", color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.testTag("transcript-duration")) }
        }
        val flag = vm.translationFlag
        if (vm.isTranslation && flag != null) Text(flag, Modifier.testTag("translation-flag"))
        vm.statusBadge?.let { badge ->
            Row(
                Modifier.background(LocalSemantic.current.cursorBg, RoundedCornerShape(50)).padding(horizontal = 8.dp, vertical = 4.dp).testTag("transcript-status"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (badge.spinning) CircularProgressIndicator(Modifier.size(12.dp), strokeWidth = 2.dp)
                Text(badge.label.text(), style = MaterialTheme.typography.labelMedium, color = Tone.NEUTRAL.color())
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TranscriptActions(vm: TranscriptVm, job: Long?, send: (AppIntent) -> Unit) {
    fun intent(value: TranscriptIntent) = send(AppIntent.Transcript(value))
    var sheet by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        Button(onClick = { intent(TranscriptIntent.DownloadPressed(false)) }, enabled = vm.downloadEnabled, modifier = Modifier.heightIn(min = 48.dp).testTag("transcript-download")) {
            Text(tk(Ns.APP, "Download Transcript"))
        }
        OutlinedButton(onClick = { intent(TranscriptIntent.CopyPressed) }, enabled = vm.canCopy, modifier = Modifier.heightIn(min = 48.dp).testTag("transcript-copy")) {
            Text(tk(Ns.APP, "Copy all transcripts"))
        }
        OutlinedButton(onClick = { sheet = true }, modifier = Modifier.heightIn(min = 48.dp).testTag("transcript-more")) {
            Text(tk(Ns.APP, "More options"))
        }
    }
    if (sheet) {
        ModalBottomSheet(onDismissRequest = { sheet = false }, modifier = Modifier.testTag("transcript-options")) {
            Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(tk(Ns.APP, "More options"), style = MaterialTheme.typography.titleMedium)
                if (vm.timeToggle) {
                    ToggleRow(tk(Ns.APP, "Show Time"), vm.showTime, "toggle-time") { intent(TranscriptIntent.ShowTimeToggled(it)) }
                }
                if (vm.phoneSpeakerToggle) {
                    ToggleRow(tk(Ns.APP, "Show Speaker"), vm.showSpeakers, "toggle-speakers") { intent(TranscriptIntent.ShowSpeakersToggled(it)) }
                }
                OutlinedButton(
                    onClick = {
                        sheet = false
                        intent(TranscriptIntent.DownloadPressed(true))
                    },
                    enabled = vm.downloadEnabled,
                    modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("transcript-download-all"),
                ) { Text(tk(Ns.COMMON, "Download all formats")) }
                if (job != null) {
                    OutlinedButton(
                        onClick = {
                            sheet = false
                            send(AppIntent.Feedback(FeedbackIntent.Opened(job)))
                        },
                        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("transcript-feedback"),
                    ) { Text(tk(Ns.APP, "Feedback")) }
                }
            }
        }
    }
}

@Composable
private fun ToggleRow(label: String, checked: Boolean, tag: String, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label)
        Switch(checked = checked, onCheckedChange = onChange, modifier = Modifier.testTag(tag))
    }
}
