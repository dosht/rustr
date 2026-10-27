package dev.transgate.android.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.transgate.core.AnalysisIntent
import dev.transgate.core.AnalysisPanelVm
import dev.transgate.core.AnalysisType
import dev.transgate.core.AppIntent
import dev.transgate.core.ConsentDecision
import dev.transgate.core.ConsentIntent
import dev.transgate.core.ConsentPanelVm
import dev.transgate.core.DocumentFormat
import dev.transgate.core.Gate
import dev.transgate.core.Ns
import dev.transgate.core.PanelState
import dev.transgate.core.SignedInVm
import dev.transgate.core.Tone

/** Which AI tab the consent introduction is worded for (web `AiGate` `feature`). */
internal enum class AiFeature { SUMMARY, CHAT }

/**
 * The AI Analysis tab (WEB-069): the consent gate, then the analysis panel core describes.
 * The tab announces itself with `Shown`/`Hidden` as it enters and leaves composition.
 */
@Composable
fun AnalysisScreen(vm: SignedInVm, send: (AppIntent) -> Unit) {
    DisposableEffect(Unit) {
        send(AppIntent.Analysis(AnalysisIntent.Shown))
        onDispose { send(AppIntent.Analysis(AnalysisIntent.Hidden)) }
    }
    AiGate(vm.consent, AiFeature.SUMMARY, send) {
        val panel = vm.analysis
        if (panel != null) AnalysisPanel(panel, send)
    }
}

/** The web `AiGate`: a spinner until consent is known, the introduction while required, else [content]. */
@Composable
internal fun AiGate(consent: ConsentPanelVm, feature: AiFeature, send: (AppIntent) -> Unit, content: @Composable () -> Unit) {
    when (consent.gate) {
        Gate.LOADING -> Box(Modifier.fillMaxSize().padding(32.dp).testTag("ai-consent-loading"), contentAlignment = Alignment.Center) {
            CircularProgressIndicator()
        }
        Gate.GRANTED -> content()
        Gate.REQUIRED -> ConsentIntroduction(consent, feature, send)
    }
}

@Composable
private fun ConsentIntroduction(consent: ConsentPanelVm, feature: AiFeature, send: (AppIntent) -> Unit) {
    var learnMore by remember { mutableStateOf(false) }
    val title = when (feature) {
        AiFeature.SUMMARY -> "AI Summary"
        AiFeature.CHAT -> "AI Chat Assistant"
    }
    val description = when (feature) {
        AiFeature.SUMMARY -> "Get an intelligent summary of your transcript content"
        AiFeature.CHAT -> "Ask questions about your transcript content"
    }
    val benefits = when (feature) {
        AiFeature.SUMMARY -> listOf(
            "Automatically extracts key points",
            "Saves time reading long transcripts",
            "Highlights main discussion topics",
        )
        AiFeature.CHAT -> listOf(
            "Interactive Q&A about content",
            "Get clarifications instantly",
            "Explore topics in depth",
        )
    }
    val button = when (feature) {
        AiFeature.SUMMARY -> "Start Summarizing"
        AiFeature.CHAT -> "Start Chatting"
    }
    LazyColumn(
        Modifier.fillMaxSize().testTag("ai-introduction"),
        contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        item {
            Button(
                onClick = { send(AppIntent.Consent(ConsentIntent.ENABLE_REQUESTED)) },
                enabled = consent.busy == null,
                modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("ai-consent-accept"),
            ) {
                if (consent.busy == ConsentDecision.ENABLE) {
                    CircularProgressIndicator(Modifier.padding(end = 8.dp), strokeWidth = 2.dp)
                }
                Text(tk(Ns.AI, button))
            }
        }
        if (consent.providerName != null) {
            item {
                TextButton(onClick = { learnMore = true }, modifier = Modifier.heightIn(min = 48.dp).testTag("ai-consent-learn-more")) {
                    Text(tk(Ns.AI, "Learn More"))
                }
            }
        }
        item {
            Text(
                tk(Ns.AI, "AI features process your transcript content securely. You can disable them anytime in settings."),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        consent.actionError?.let { error ->
            item {
                Text(error.text(), color = Tone.DANGER.color(), style = MaterialTheme.typography.bodySmall, modifier = Modifier.testTag("ai-consent-error"))
            }
        }
        item { Text(tk(Ns.AI, title), style = MaterialTheme.typography.titleMedium) }
        item { Text(tk(Ns.AI, description), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        items(benefits.size) { index ->
            Text("• " + tk(Ns.AI, benefits[index]), style = MaterialTheme.typography.bodyMedium)
        }
    }
    if (learnMore && consent.providerName != null) {
        ConsentDialog(consent, consent.providerName!!, send) { learnMore = false }
    }
}

@Composable
private fun ConsentDialog(consent: ConsentPanelVm, provider: String, send: (AppIntent) -> Unit, close: () -> Unit) {
    val uri = LocalUriHandler.current
    AlertDialog(
        onDismissRequest = close,
        modifier = Modifier.testTag("ai-consent-dialog"),
        title = { Text(tk(Ns.AI, "AI Features Consent")) },
        text = {
            LazyColumn(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                colItem {
                    Text(tk(Ns.AI, "AI Processing Information"), style = MaterialTheme.typography.titleSmall)
                    Text(
                        tk(Ns.AI, "To provide AI-powered features like summaries, highlights, and chat assistance, we need to send your transcript content to our AI service provider") +
                            " ($provider).",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                colItem {
                    Text(tk(Ns.AI, "What this means:"), style = MaterialTheme.typography.titleSmall)
                    Text("• " + tk(Ns.APP, "p5.consent.provider_processing", "provider" to dev.transgate.core.ArgWire.Text(provider)), style = MaterialTheme.typography.bodySmall)
                    Text("• " + tk(Ns.AI, "We use secure HTTPS connections to protect your data in transit"), style = MaterialTheme.typography.bodySmall)
                    Text("• " + tk(Ns.APP, "p5.consent.provider_retention"), style = MaterialTheme.typography.bodySmall)
                    Text("• " + tk(Ns.APP, "p5.consent.account_results"), style = MaterialTheme.typography.bodySmall)
                    Text("• " + tk(Ns.APP, "p5.consent.decline_keeps"), style = MaterialTheme.typography.bodySmall)
                    Text("• " + tk(Ns.APP, "p5.consent.settings_delete"), style = MaterialTheme.typography.bodySmall)
                }
                colItem {
                    Text(tk(Ns.AI, "AI Features include:"), style = MaterialTheme.typography.titleSmall)
                    Text("• " + tk(Ns.AI, "Automatic transcript summaries"), style = MaterialTheme.typography.bodySmall)
                    Text("• " + tk(Ns.AI, "Key highlights extraction"), style = MaterialTheme.typography.bodySmall)
                    Text("• " + tk(Ns.AI, "Interactive AI chat assistant"), style = MaterialTheme.typography.bodySmall)
                }
                colItem {
                    Text(
                        tk(Ns.AI, "By accepting, you agree to the processing of your transcript data as described above. For more information, please review our"),
                        style = MaterialTheme.typography.bodySmall,
                    )
                    TextButton(
                        onClick = { uri.openUri(consent.privacyPolicyUrl) },
                        modifier = Modifier.heightIn(min = 48.dp).testTag("ai-consent-privacy"),
                    ) { Text(tk(Ns.COMMON, "Privacy Policy")) }
                }
            }
        },
        dismissButton = {
            OutlinedButton(
                onClick = {
                    close()
                    send(AppIntent.Consent(ConsentIntent.DECLINE_REQUESTED))
                },
                modifier = Modifier.heightIn(min = 48.dp).testTag("ai-consent-decline"),
            ) { Text(tk(Ns.AI, "Decline")) }
        },
        confirmButton = {
            Button(
                onClick = {
                    close()
                    send(AppIntent.Consent(ConsentIntent.ENABLE_REQUESTED))
                },
                modifier = Modifier.heightIn(min = 48.dp).testTag("ai-consent-dialog-accept"),
            ) { Text(tk(Ns.AI, "Accept & Enable AI Features")) }
        },
    )
}

@Composable
private fun AnalysisPanel(view: AnalysisPanelVm, send: (AppIntent) -> Unit) {
    fun analysis(intent: AnalysisIntent) = send(AppIntent.Analysis(intent))
    val counter = remember { SectionCounter() }
    Column(Modifier.fillMaxSize().testTag("analysis-panel")) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column(Modifier.weight(1f)) {
                Text(tk(Ns.AI, "AI Analysis"), style = MaterialTheme.typography.titleSmall, modifier = Modifier.testTag("analysis-title"))
                view.detected?.let { detected -> TypeBadge(detected.kind) }
            }
            if (view.state == PanelState.ERROR && view.canRetry) {
                TextButton(onClick = { analysis(AnalysisIntent.RetryRequested) }, modifier = Modifier.heightIn(min = 48.dp).testTag("analysis-retry")) {
                    Text(tk(Ns.AI, "Retry"))
                }
            }
            if (view.hasData) {
                TextButton(onClick = { analysis(AnalysisIntent.CopyRequested) }, modifier = Modifier.heightIn(min = 48.dp).testTag("analysis-copy")) {
                    Text(tk(Ns.AI, "Copy to clipboard"))
                }
                ExportMenu(view.exportBusy, view.exportError, send)
            }
        }
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth().testTag("analysis-list"),
            state = rememberLazyListState(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(12.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            counter.n = 0
            if (view.state == PanelState.LOADING && !view.hasData) item { ProgressBlock(view) }
            if (view.state == PanelState.ERROR || view.state == PanelState.CANCELLED) item { FailureBlock(view) }
            if (view.state == PanelState.INTERRUPTED) item { InterruptedBlock(view, send) }
            if (view.hasData) {
                view.summary?.let { summary ->
                    item { SelectionContainer { Text(summary, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.testTag("analysis-summary")) } }
                }
                view.analysis?.let { analysisBlocks(it, counter, send) }
                if (view.state == PanelState.LOADING) item { ProgressBlock(view) }
            }
            if (view.noTranscript) {
                item { Text(tk(Ns.AI, "No transcript content available"), color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.testTag("analysis-no-transcript")) }
            } else if (view.state == PanelState.IDLE) {
                item {
                    Row(Modifier.testTag("analysis-preparing"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        CircularProgressIndicator(Modifier.padding(4.dp), strokeWidth = 2.dp)
                        Text(tk(Ns.AI, "Preparing summary..."), color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
        }
    }
}

@Composable
private fun TypeBadge(kind: AnalysisType) {
    val label = when (kind) {
        AnalysisType.GENERIC -> null
        AnalysisType.LECTURE -> "Lecture"
        AnalysisType.MEETING -> "Meeting"
        AnalysisType.SALES -> "Sales Call"
        AnalysisType.SUPPORT -> "Support"
        AnalysisType.INTERVIEW -> "Interview"
    }
    if (label != null) {
        Text(tk(Ns.AI, label), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.testTag("analysis-type"))
    }
}

@Composable
private fun ProgressBlock(view: AnalysisPanelVm) {
    val stage = view.progress?.stage?.text() ?: tk(Ns.AI, "Analyzing...")
    Column(Modifier.fillMaxWidth().testTag("analysis-progress"), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stage, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        LinearProgressIndicator(progress = { (view.progress?.percentage?.toInt() ?: 0) / 100f }, modifier = Modifier.fillMaxWidth())
    }
}

@Composable
private fun FailureBlock(view: AnalysisPanelVm) {
    Card(
        Modifier.fillMaxWidth().testTag("analysis-error"),
        border = BorderStroke(1.dp, Tone.DANGER.color()),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                tk(Ns.AI, if (view.state == PanelState.CANCELLED) "Summary generation cancelled" else "Failed to generate summary"),
                style = MaterialTheme.typography.bodyMedium,
            )
            view.errorDetail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            view.errorHint?.let { Text(it.text(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
}

@Composable
private fun InterruptedBlock(view: AnalysisPanelVm, send: (AppIntent) -> Unit) {
    Card(
        Modifier.fillMaxWidth().testTag("analysis-interrupted"),
        border = BorderStroke(1.dp, Tone.PROGRESS.color()),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
    ) {
        Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Column(Modifier.weight(1f)) {
                Text(tk(Ns.AI, "Analysis incomplete."), style = MaterialTheme.typography.bodyMedium)
                Text(
                    tk(Ns.AI, if (view.hasData) "Partial results shown." else "No data received."),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (view.canRetry) {
                Button(onClick = { send(AppIntent.Analysis(AnalysisIntent.RetryRequested)) }, modifier = Modifier.heightIn(min = 48.dp).testTag("analysis-retry")) {
                    Text(tk(Ns.AI, "Retry"))
                }
            }
        }
    }
}

@Composable
private fun ExportMenu(busy: DocumentFormat?, error: dev.transgate.core.Msg?, send: (AppIntent) -> Unit) {
    var open by remember { mutableStateOf(false) }
    fun analysis(intent: AnalysisIntent) = send(AppIntent.Analysis(intent))
    val options = listOf(
        Triple(DocumentFormat.DOCX, "Export as Word (.docx)", "Microsoft Word format"),
        Triple(DocumentFormat.PDF, "Export as PDF", "Portable Document Format"),
        Triple(DocumentFormat.TXT, "Export as TXT", "Plain text format"),
        Triple(DocumentFormat.MD, "Export as Markdown", "Markdown format"),
    )
    Box {
        TextButton(
            onClick = { open = true },
            enabled = busy == null,
            modifier = Modifier.heightIn(min = 48.dp).testTag("analysis-export"),
        ) {
            if (busy != null) CircularProgressIndicator(Modifier.padding(end = 8.dp), strokeWidth = 2.dp)
            Text(tk(Ns.AI, "Export"))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { (format, label, description) ->
                DropdownMenuItem(
                    modifier = Modifier.heightIn(min = 48.dp).testTag("analysis-export-${format.name.lowercase()}"),
                    text = {
                        Column {
                            Text(tk(Ns.AI, label))
                            Text(tk(Ns.AI, description), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    },
                    onClick = {
                        open = false
                        if (error != null) analysis(AnalysisIntent.ExportErrorDismissed)
                        analysis(AnalysisIntent.ExportRequested(format))
                    },
                )
            }
            Text(
                tk(Ns.AI, "Choose a format to download your analysis"),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            )
        }
    }
    if (error != null) {
        AlertDialog(
            onDismissRequest = { analysis(AnalysisIntent.ExportErrorDismissed) },
            modifier = Modifier.testTag("analysis-export-error"),
            text = { Text(error.text()) },
            confirmButton = {
                TextButton(
                    onClick = { analysis(AnalysisIntent.ExportErrorDismissed) },
                    modifier = Modifier.heightIn(min = 48.dp).testTag("analysis-export-error-dismiss"),
                ) { Text(tk(Ns.AI, "Dismiss")) }
            },
        )
    }
}

internal class SectionCounter { var n: Int = 0 }

/** A lazy item whose several children stack vertically (a bare `item` overlays them). */
internal fun LazyListScope.colItem(content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit) {
    item { Column(content = content) }
}
