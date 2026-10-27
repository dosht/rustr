package dev.transgate.android.ui

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
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
import androidx.compose.ui.unit.dp
import dev.transgate.android.ShellHost
import dev.transgate.core.Alert
import dev.transgate.core.AppIntent
import dev.transgate.core.ArgWire
import dev.transgate.core.Choice
import dev.transgate.core.Chip
import dev.transgate.core.FileRow
import dev.transgate.core.LanguageOption
import dev.transgate.core.Mode
import dev.transgate.core.Msg
import dev.transgate.core.Ns
import dev.transgate.core.SignedInVm
import dev.transgate.core.UploadIntent
import dev.transgate.core.UploadQueueVm
import dev.transgate.core.YouTubeDialog

@Composable
fun UploadScreen(vm: SignedInVm, send: (AppIntent) -> Unit, host: ShellHost) {
    val queue = vm.queue
    val upload: (UploadIntent) -> Unit = { send(AppIntent.Upload(it)) }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris: List<Uri> ->
        if (uris.isNotEmpty()) upload(UploadIntent.Picked(host.files.hold(uris)))
    }
    val mimeTypes = pickerMimeTypes(queue.pickerAccept)

    Column(Modifier.fillMaxSize()) {
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            UploadAlerts(queue, upload)
            if (queue.rows.isNotEmpty()) {
                val total = queue.totalMinutes
                Text(
                    tk(Ns.COMMON, "Total Duration:") + " " +
                        if (total == null) tk(Ns.COMMON, "Calculating...")
                        else tk(Ns.APP, "p5.duration.minutes", "minutes" to ArgWire.Number(total)),
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.testTag("upload-total"),
                )
            }
            if (!queue.maxFilesReached) {
                Button(
                    onClick = { picker.launch(mimeTypes) },
                    modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("upload-pick"),
                ) { Text(tk(Ns.COMMON, "Upload files")) }
                if (vm.capabilities.youtube) {
                    OutlinedButton(
                        onClick = { upload(UploadIntent.YouTubeDialogOpened) },
                        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("upload-youtube"),
                    ) { Text(tk(Ns.COMMON, "Add from YouTube")) }
                }
            }
            if (queue.rows.isNotEmpty()) {
                Text(tk(Ns.COMMON, "Files"), style = MaterialTheme.typography.titleMedium)
                queue.rows.forEach { row -> UploadFileRow(row, upload) }
                UploadConfiguration(queue, upload)
            } else if (queue.initializing) {
                Box(Modifier.fillMaxWidth().padding(vertical = 48.dp), contentAlignment = Alignment.Center) {
                    Text(tk(Ns.COMMON, "Loading..."), modifier = Modifier.testTag("upload-loading"))
                }
            } else {
                Column(Modifier.fillMaxWidth().padding(vertical = 48.dp).testTag("upload-empty"), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(tk(Ns.COMMON, "No files added yet"), style = MaterialTheme.typography.titleMedium)
                    Text(
                        tk(Ns.COMMON, "Upload audio files or record directly to get started"),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        if (queue.rows.isNotEmpty()) {
            HorizontalDivider()
            val label = when {
                queue.transcribing -> "Starting..."
                !queue.filesReady -> "Preparing files..."
                queue.mode == Mode.TRANSLATION -> "Start Translating"
                else -> "Start Transcribing"
            }
            Button(
                onClick = { upload(UploadIntent.StartPressed) },
                enabled = !queue.startDisabled,
                modifier = Modifier.fillMaxWidth().padding(16.dp).heightIn(min = 48.dp).testTag("upload-start"),
            ) {
                if (queue.transcribing) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                Text(tk(Ns.COMMON, label), modifier = Modifier.padding(start = if (queue.transcribing) 8.dp else 0.dp))
            }
        }
    }
    UploadYouTubeDialog(queue.youtubeDialog, upload)
}

/** The picker's MIME filter: core's accept list, widened to any type when it names extensions. */
private fun pickerMimeTypes(accept: String): Array<String> {
    val entries = accept.split(',').map { it.trim() }.filter { it.isNotEmpty() }
    return if (entries.isEmpty() || entries.any { it.startsWith(".") }) arrayOf("*/*") else entries.toTypedArray()
}

@Composable
private fun UploadAlerts(queue: UploadQueueVm, upload: (UploadIntent) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        queue.alerts.forEach { alert ->
            Card(
                Modifier.fillMaxWidth().testTag("upload-alert-" + alert.name.lowercase().replace('_', '-')),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
            ) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    when (alert) {
                        Alert.BALANCE -> {
                            Text(tk(Ns.COMMON, "Not enough balance."), style = MaterialTheme.typography.titleSmall)
                            Text(tk(Ns.COMMON, "Your balance is not enough to run this job. Please top up your minutes and try again."))
                        }
                        Alert.LANGUAGE -> {
                            Text(tk(Ns.COMMON, "Language Selection Required"), style = MaterialTheme.typography.titleSmall)
                            Text(tk(Ns.COMMON, "Please select the audio language before starting transcription."))
                        }
                        Alert.PROCESSING_TIMEOUT -> {
                            Text(tk(Ns.COMMON, "Processing Timeout"), style = MaterialTheme.typography.titleSmall)
                            Text(tk(Ns.COMMON, "File preparation took longer than expected. This could be due to file processing issues or upload problems."))
                            Text(tk(Ns.COMMON, "Please try uploading the file again. If the problem persists, contact support."))
                        }
                        Alert.YOU_TUBE -> {
                            Text(tk(Ns.COMMON, "YouTube Transcription Failed"), style = MaterialTheme.typography.titleSmall)
                            queue.youtubeError?.let { Text(it.text()) }
                        }
                    }
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                        TextButton(
                            onClick = { upload(UploadIntent.DismissAlert(alert)) },
                            modifier = Modifier.heightIn(min = 48.dp).testTag("upload-alert-dismiss-" + alert.name.lowercase().replace('_', '-')),
                        ) { Text(tk(Ns.COMMON, "OK")) }
                    }
                }
            }
        }
        val error = queue.youtubeError
        if (error != null && Alert.YOU_TUBE !in queue.alerts) {
            Text(error.text(), color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("upload-youtube-error"))
        }
    }
}

@Composable
private fun UploadFileRow(row: FileRow, upload: (UploadIntent) -> Unit) {
    val uploading = row.chip as? Chip.Uploading
    Card(Modifier.fillMaxWidth().testTag("upload-row-${row.id}")) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Column(Modifier.weight(1f)) {
                    Text(row.name, maxLines = 1, style = MaterialTheme.typography.bodyLarge)
                    val duration = row.durationText
                    Text(
                        if (duration != null) row.phoneSize + " · " + duration.text() else row.phoneSize,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text(row.phoneLabel.text(), style = MaterialTheme.typography.bodySmall, modifier = Modifier.testTag("upload-row-label-${row.id}"))
                IconButton(onClick = { upload(UploadIntent.Removed(row.id)) }, modifier = Modifier.size(48.dp).testTag("upload-remove-${row.id}")) {
                    Icon(Icons.Default.Delete, contentDescription = tk(Ns.APP, "Delete"))
                }
            }
            if (uploading != null && row.progress.toInt() < 100) {
                LinearProgressIndicator(
                    progress = { row.progress.toInt() / 100f },
                    modifier = Modifier.fillMaxWidth().testTag("upload-row-progress-${row.id}"),
                )
            }
        }
    }
}

@Composable
private fun UploadConfiguration(queue: UploadQueueVm, upload: (UploadIntent) -> Unit) {
    val translation = queue.mode == Mode.TRANSLATION
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        UploadSwitch(
            label = tk(Ns.COMMON, "Translation"),
            description = null,
            checked = translation,
            tag = "upload-mode-translation",
            onChange = { upload(UploadIntent.TabChanged(if (it) Mode.TRANSLATION else Mode.TRANSCRIPTION)) },
        )
        val chosen = queue.language
        UploadLanguagePicker(
            tag = "upload-language",
            label = tk(Ns.COMMON, "Audio Language"),
            value = if (translation) queue.sourceLanguage else (chosen as? Choice.Language)?.v1,
            options = if (translation) queue.sourceLanguages else queue.available,
            preferred = queue.phonePreferred,
            autoDetect = if (queue.autoDetectOffered) queue.autoDetectLabel else null,
            autoDetectSelected = chosen is Choice.AutoDetect,
            onChange = { language ->
                upload(
                    if (translation) UploadIntent.SourceLanguageChosen(language)
                    else UploadIntent.LanguageChosen(if (language == null) Choice.AutoDetect else Choice.Language(language)),
                )
            },
        )
        if (translation) {
            UploadLanguagePicker(
                tag = "upload-target-language",
                label = tk(Ns.COMMON, "Translation Language"),
                value = queue.targetLanguage,
                options = queue.targetLanguages,
                preferred = queue.preferredTargets,
                autoDetect = null,
                autoDetectSelected = false,
                onChange = { upload(UploadIntent.TargetLanguageChosen(it)) },
            )
        }
        UploadSwitch(
            label = tk(Ns.COMMON, "Speaker Recognition"),
            description = tk(Ns.COMMON, "Identify different speakers in the transcript"),
            checked = queue.speakers,
            tag = "upload-speakers",
            onChange = { upload(UploadIntent.SpeakersToggled(it)) },
        )
    }
}

@Composable
private fun UploadSwitch(label: String, description: String?, checked: Boolean, tag: String, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.bodyLarge)
            if (description != null) Text(description, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Switch(checked = checked, onCheckedChange = onChange, modifier = Modifier.testTag(tag))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun UploadLanguagePicker(
    tag: String,
    label: String,
    value: String?,
    options: List<LanguageOption>,
    preferred: List<LanguageOption>,
    autoDetect: Msg?,
    autoDetectSelected: Boolean,
    onChange: (String?) -> Unit,
) {
    var open by remember { mutableStateOf(false) }
    var search by remember { mutableStateOf("") }
    val selected = options.firstOrNull { it.language == value }
    val shown = when {
        selected != null -> selected.flag + " " + selected.name
        autoDetect != null && autoDetectSelected -> autoDetect.text()
        else -> tk(Ns.COMMON, "Choose the language")
    }
    OutlinedButton(
        onClick = { open = true },
        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag(tag),
    ) {
        Text(label, Modifier.weight(1f), color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(shown)
    }
    if (open) {
        val close = { open = false; search = "" }
        ModalBottomSheet(onDismissRequest = close) {
            Column(Modifier.padding(horizontal = 16.dp).padding(bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(label, style = MaterialTheme.typography.titleMedium)
                OutlinedTextField(
                    value = search,
                    onValueChange = { search = it },
                    singleLine = true,
                    placeholder = { Text(tk(Ns.COMMON, "Search languages...")) },
                    modifier = Modifier.fillMaxWidth().testTag("$tag-search"),
                )
                val matches = options.filter { it.name.contains(search, ignoreCase = true) }
                LazyColumn(Modifier.heightIn(max = 360.dp)) {
                    if (autoDetect != null && search.isEmpty()) {
                        item {
                            UploadOptionRow(autoDetect.text(), "$tag-option-auto") { close(); onChange(null) }
                        }
                    }
                    if (search.isEmpty() && preferred.isNotEmpty()) {
                        items(preferred, key = { "p-" + it.language }) { option ->
                            UploadOptionRow(option.flag + " " + option.name, "$tag-option-${option.language}") { close(); onChange(option.language) }
                        }
                        item { HorizontalDivider(Modifier.padding(vertical = 8.dp)) }
                    }
                    items(matches, key = { it.language }) { option ->
                        UploadOptionRow(option.flag + " " + option.name, "$tag-option-${option.language}") { close(); onChange(option.language) }
                    }
                    if (matches.isEmpty()) {
                        item { Text(tk(Ns.COMMON, "No languages found"), Modifier.padding(12.dp)) }
                    }
                }
            }
        }
    }
}

@Composable
private fun UploadOptionRow(text: String, tag: String, onClick: () -> Unit) {
    Text(
        text,
        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable(onClick = onClick).padding(horizontal = 12.dp, vertical = 12.dp).testTag(tag),
        style = MaterialTheme.typography.bodyLarge,
    )
}

@Composable
private fun UploadYouTubeDialog(dialog: YouTubeDialog, upload: (UploadIntent) -> Unit) {
    if (!dialog.open) return
    AlertDialog(
        onDismissRequest = { if (!dialog.busy) upload(UploadIntent.YouTubeDialogClosed) },
        title = { Text(tk(Ns.COMMON, "Add YouTube Video")) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(tk(Ns.COMMON, "Paste a YouTube link to transcribe. Videos with available captions are transcribed instantly."), style = MaterialTheme.typography.bodySmall)
                OutlinedTextField(
                    value = dialog.url,
                    onValueChange = { upload(UploadIntent.YouTubeUrlChanged(it)) },
                    enabled = !dialog.busy,
                    singleLine = true,
                    label = { Text(tk(Ns.COMMON, "YouTube URL")) },
                    modifier = Modifier.fillMaxWidth().testTag("youtube-url"),
                )
                dialog.error?.let { Text(it.text(), color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("youtube-error")) }
            }
        },
        confirmButton = {
            Button(
                onClick = { if (!dialog.busy) upload(UploadIntent.YouTubeSubmitted) },
                modifier = Modifier.heightIn(min = 48.dp).testTag("youtube-submit"),
            ) {
                if (dialog.busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                Text(tk(Ns.COMMON, if (dialog.busy) "Adding..." else "Add Video"), Modifier.padding(start = if (dialog.busy) 8.dp else 0.dp))
            }
        },
        dismissButton = {
            TextButton(
                onClick = { upload(UploadIntent.YouTubeDialogClosed) },
                enabled = !dialog.busy,
                modifier = Modifier.heightIn(min = 48.dp).testTag("youtube-cancel"),
            ) { Text(tk(Ns.COMMON, "Cancel")) }
        },
    )
}
