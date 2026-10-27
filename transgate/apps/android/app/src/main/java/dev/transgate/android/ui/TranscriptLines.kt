package dev.transgate.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import dev.transgate.core.AppIntent
import dev.transgate.core.FreemiumBanner
import dev.transgate.core.LineRow
import dev.transgate.core.Ns
import dev.transgate.core.SpeakerLabel
import dev.transgate.core.SpeakerOption
import dev.transgate.core.TranscriptIntent
import dev.transgate.core.TranscriptVm

/** The lines tab: language tabs, the loading/error/empty states, the rows and the freemium banner. */
@Composable
internal fun TranscriptLinesPanel(vm: TranscriptVm, activeIndex: Int, send: (AppIntent) -> Unit) {
    fun intent(value: TranscriptIntent) = send(AppIntent.Transcript(value))
    val error = vm.loadError
    if (error != null) {
        Column(Modifier.fillMaxWidth().padding(16.dp).testTag("transcript-load-error")) {
            Text(error.text(), color = MaterialTheme.colorScheme.error)
            vm.loadErrorDetail?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
        return
    }
    Column(Modifier.fillMaxWidth()) {
        if (vm.showTabs) {
            val selected = vm.tabs.indexOfFirst { it.tab == vm.selected }.coerceAtLeast(0)
            ScrollableTabRow(selectedTabIndex = selected, modifier = Modifier.testTag("language-tabs")) {
                vm.tabs.forEachIndexed { index, option ->
                    Tab(
                        selected = index == selected,
                        onClick = { intent(TranscriptIntent.TabSelected(option.tab)) },
                        modifier = Modifier.heightIn(min = 48.dp).testTag("language-tab-$index"),
                        text = { Text("${option.flag} ${option.name}") },
                    )
                }
            }
        }
        val fades = remember(vm.fades) { vm.fades.associate { it.row.toInt() to it.opacityPercent.toInt() } }
        val direction = if (vm.rtl) LayoutDirection.Rtl else LayoutDirection.Ltr
        CompositionLocalProvider(LocalLayoutDirection provides direction) {
            LazyColumn(Modifier.fillMaxWidth().testTag("transcript-lines")) {
                val loading = vm.loadingMessage
                if (vm.rows.isEmpty() && loading != null) {
                    item(key = "loading") {
                        Row(Modifier.padding(16.dp).testTag("transcript-loading"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            CircularProgressIndicator(Modifier.size(20.dp))
                            Text(loading.text(), color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
                items(vm.rows.size, key = { vm.rows[it].id }) { index ->
                    val row = vm.rows[index]
                    LineItem(vm, row, active = index == activeIndex && !row.editing, fade = fades[index], send = send)
                    HorizontalDivider()
                }
                vm.empty?.let { empty ->
                    item(key = "empty") {
                        Text(empty.text(), Modifier.fillMaxWidth().padding(16.dp).testTag("transcript-empty"), color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                vm.freemium?.let { banner -> item(key = "freemium") { FreemiumCard(banner) } }
            }
        }
    }
}

@Composable
private fun LineItem(vm: TranscriptVm, row: LineRow, active: Boolean, fade: Int?, send: (AppIntent) -> Unit) {
    fun intent(value: TranscriptIntent) = send(AppIntent.Transcript(value))
    Box(Modifier.fillMaxWidth().testTag("transcript-line-${row.id}")) {
        if (fade != null) {
            // Core's fade is decorative: only this background layer carries the opacity, never the text.
            Box(Modifier.matchParentSize().alpha(fade / 100f).background(LocalSemantic.current.noticeBg).testTag("line-fade-${row.id}"))
        }
        Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp)) {
            if (vm.showSpeakers) SpeakerCell(vm, row, send)
            Box(Modifier.fillMaxWidth().then(if (active) Modifier.background(MaterialTheme.colorScheme.primaryContainer, RoundedCornerShape(8.dp)) else Modifier).testTag(if (active) "line-active" else "line-body-${row.id}")) {
                Column(Modifier.fillMaxWidth().padding(4.dp)) {
                    if (row.editing) {
                        LineEditor(row, send)
                    } else {
                        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
                            Text(row.text, Modifier.weight(1f).padding(top = 12.dp).testTag("line-text-${row.id}"))
                            IconButton(onClick = { intent(TranscriptIntent.EditStarted(row.id)) }, Modifier.size(48.dp).testTag("line-edit-${row.id}")) {
                                Icon(Icons.Default.Edit, contentDescription = tk(Ns.APP, "p5.transcript.edit_line"))
                            }
                        }
                    }
                    if (vm.showTime) {
                        CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
                            TextButton(onClick = { intent(TranscriptIntent.TimeClicked(row.id)) }, Modifier.heightIn(min = 48.dp).testTag("line-time-${row.id}")) {
                                Text(row.timeText, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun LineEditor(row: LineRow, send: (AppIntent) -> Unit) {
    // A text field's draft is UI mechanics: seeded from the row each time editing begins.
    var draft by remember(row.id) { mutableStateOf(row.text) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(row.id) { focus.requestFocus() }
    OutlinedTextField(
        value = draft,
        onValueChange = { draft = it },
        modifier = Modifier.fillMaxWidth().focusRequester(focus).testTag("line-editor"),
        label = { Text(tk(Ns.APP, "p5.transcript.edit_line")) },
        minLines = 2,
    )
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        IconButton(onClick = { send(AppIntent.Transcript(TranscriptIntent.EditCanceled)) }, Modifier.size(48.dp).testTag("line-cancel")) {
            Icon(Icons.Default.Close, contentDescription = tk(Ns.COMMON, "Cancel"))
        }
        IconButton(onClick = { send(AppIntent.Transcript(TranscriptIntent.EditSubmitted(draft))) }, Modifier.size(48.dp).testTag("line-save")) {
            Icon(Icons.Default.Check, contentDescription = tk(Ns.APP, "p5.transcript.save_line"))
        }
    }
}

/** `shared/speaker.ts` `speakerText`: the only mapping from core's label to words. */
@Composable
private fun SpeakerLabel.text(): String = when (this) {
    is SpeakerLabel.Numbered -> "${tk(Ns.COMMON, "Speaker")} $number"
    is SpeakerLabel.Named -> name
    SpeakerLabel.None -> tk(Ns.APP, "N/A")
}

@Composable
private fun SpeakerCell(vm: TranscriptVm, row: LineRow, send: (AppIntent) -> Unit) {
    val label = row.speaker
    if (row.speakerPending) {
        Box(Modifier.padding(8.dp).size(width = 64.dp, height = 16.dp).background(LocalSemantic.current.cursorBg, RoundedCornerShape(4.dp)).testTag("speaker-pending-${row.id}"))
    } else if (label != null) {
        SpeakerMenu(row, label, vm.speakerOptions, send)
    }
}

@Composable
private fun SpeakerMenu(row: LineRow, label: SpeakerLabel, options: List<SpeakerOption>, send: (AppIntent) -> Unit) {
    fun intent(value: TranscriptIntent) = send(AppIntent.Transcript(value))
    var open by remember(row.id) { mutableStateOf(false) }
    var renaming by remember(row.id) { mutableStateOf(false) }
    var draft by remember(row.id) { mutableStateOf("") }
    if (renaming) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                modifier = Modifier.widthIn(min = 120.dp).testTag("speaker-rename-${row.id}"),
                label = { Text(tk(Ns.APP, "p5.transcript.edit_speaker_label")) },
                singleLine = true,
            )
            IconButton(onClick = { renaming = false }, Modifier.size(48.dp).testTag("speaker-rename-cancel-${row.id}")) {
                Icon(Icons.Default.Close, contentDescription = tk(Ns.COMMON, "Cancel"))
            }
            IconButton(onClick = {
                renaming = false
                intent(TranscriptIntent.SpeakerRenamed(row.speakerValue ?: "", draft))
            }, Modifier.size(48.dp).testTag("speaker-rename-save-${row.id}")) {
                Icon(Icons.Default.Check, contentDescription = tk(Ns.APP, "p5.transcript.save_line"))
            }
        }
        return
    }
    Box {
        TextButton(onClick = { open = true }, Modifier.heightIn(min = 48.dp).testTag("speaker-${row.id}")) {
            Text(label.text(), color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, modifier = Modifier.testTag("speaker-menu-${row.id}")) {
            options.forEachIndexed { index, option ->
                val checkMark: @Composable () -> Unit = { Icon(Icons.Default.Check, contentDescription = null) }
                DropdownMenuItem(
                    text = { Text(option.label.text()) },
                    onClick = {
                        open = false
                        intent(TranscriptIntent.SpeakerChosen(row.id, option.value))
                    },
                    modifier = Modifier.heightIn(min = 48.dp).testTag("speaker-option-${row.id}-$index"),
                    trailingIcon = if (option.value == row.speakerValue) checkMark else null,
                )
            }
            HorizontalDivider()
            DropdownMenuItem(
                text = { Text(tk(Ns.COMMON, "Edit speaker")) },
                onClick = {
                    open = false
                    draft = row.speakerValue ?: ""
                    renaming = true
                },
                modifier = Modifier.heightIn(min = 48.dp).testTag("speaker-edit-${row.id}"),
            )
        }
    }
}

/** FRE-020's limit card: texts and progress only; the upgrade purchase is a web handoff (D6), so no button. */
@Composable
private fun FreemiumCard(banner: FreemiumBanner) {
    Column(
        Modifier.fillMaxWidth().padding(16.dp).background(LocalSemantic.current.noticeBg, RoundedCornerShape(12.dp)).padding(16.dp).testTag("freemium-banner"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(tk(Ns.COMMON, "freemiumLimitReached"), style = MaterialTheme.typography.titleSmall)
        Text(
            "${banner.currentMinutes} / ${banner.maxMinutes} ${tk(Ns.COMMON, "min")} ${tk(Ns.COMMON, "transcriptionProgress").lowercase()}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        LinearProgressIndicator(progress = { banner.percent.toInt() / 100f }, modifier = Modifier.fillMaxWidth().testTag("freemium-progress"))
        Text(tk(Ns.COMMON, "freemiumLimitMessage"))
        Text("${tk(Ns.COMMON, "upgradeToUnlock")}:", style = MaterialTheme.typography.labelLarge)
        Text(tk(Ns.COMMON, "unlimitedTranscriptionTime"), color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(tk(Ns.COMMON, "priorityProcessing"), color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
