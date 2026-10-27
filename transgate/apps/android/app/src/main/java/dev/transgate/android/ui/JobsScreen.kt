package dev.transgate.android.ui

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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import dev.transgate.android.ShellHost
import dev.transgate.core.AppIntent
import dev.transgate.core.FeedbackIntent
import dev.transgate.core.HomeView
import dev.transgate.core.JobListVm
import dev.transgate.core.JobRow
import dev.transgate.core.JobsIntent
import dev.transgate.core.Ns
import dev.transgate.core.Route
import dev.transgate.core.SignedInVm

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun JobsScreen(vm: SignedInVm, send: (AppIntent) -> Unit, host: ShellHost) {
    val jobs = vm.jobs
    val job: (JobsIntent) -> Unit = { send(AppIntent.Jobs(it)) }
    val newTranscript = { send(AppIntent.Navigate(vm.newTranscriptRoute)) }

    Box(Modifier.fillMaxSize()) {
        when {
            jobs.initialLoading && jobs.rows.isEmpty() -> JobsCentered("jobs-loading") {
                CircularProgressIndicator()
                Text(tk(Ns.APP, "Loading jobs..."))
            }
            jobs.error != null -> JobsCentered("jobs-error") {
                Text(jobs.error!!.text(), style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center)
                jobs.errorDetail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center) }
                Button(onClick = { job(JobsIntent.Refresh) }, modifier = Modifier.heightIn(min = 48.dp).testTag("jobs-retry")) {
                    Text(tk(Ns.APP, "Try Again"))
                }
            }
            jobs.rows.isEmpty() -> JobsCentered("jobs-empty") {
                Text(tk(Ns.APP, "No transcripts found"), style = MaterialTheme.typography.titleMedium)
                Text(tk(Ns.APP, "Start by creating your first transcript"), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Button(onClick = newTranscript, modifier = Modifier.heightIn(min = 48.dp).testTag("jobs-new")) {
                    Icon(Icons.Default.Add, contentDescription = null)
                    Text(tk(Ns.APP, "New Transcription"), Modifier.padding(start = 8.dp))
                }
            }
            else -> PullToRefreshBox(isRefreshing = jobs.initialLoading, onRefresh = { job(JobsIntent.Refresh) }, modifier = Modifier.fillMaxSize()) {
                JobsList(jobs, send, job)
            }
        }
    }

    jobs.confirmDelete?.let {
        AlertDialog(
            onDismissRequest = { job(JobsIntent.DeleteCanceled) },
            title = { Text(tk(Ns.COMMON, "Are you sure you want to delete this file?")) },
            text = { Text(tk(Ns.COMMON, "Note that you can't access this file again.")) },
            confirmButton = {
                TextButton(onClick = { job(JobsIntent.DeleteConfirmed) }, modifier = Modifier.heightIn(min = 48.dp).testTag("jobs-delete-confirm")) {
                    Text(tk(Ns.COMMON, "Yes"), color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { job(JobsIntent.DeleteCanceled) }, modifier = Modifier.heightIn(min = 48.dp).testTag("jobs-delete-cancel")) {
                    Text(tk(Ns.COMMON, "No"))
                }
            },
        )
    }
}

@Composable
private fun JobsCentered(tag: String, content: @Composable () -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(16.dp).testTag(tag),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) { content() }
}

@Composable
private fun JobsList(jobs: JobListVm, send: (AppIntent) -> Unit, job: (JobsIntent) -> Unit) {
    LazyColumn(
        Modifier.fillMaxSize().testTag("jobs-list"),
        contentPadding = androidx.compose.foundation.layout.PaddingValues(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(jobs.rows, key = { it.id }) { row -> JobCard(jobs, row, send, job) }
        item(key = "jobs-tail") {
            // The sentinel: composing the tail reports visibility; core decides whether to page.
            LaunchedEffect(jobs.canLoadMore, jobs.rows.size) {
                if (jobs.canLoadMore) job(JobsIntent.LoadMore)
            }
            Column(Modifier.fillMaxWidth().padding(vertical = 8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                if (jobs.loadingMore) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                        Text(tk(Ns.APP, "Loading more..."), style = MaterialTheme.typography.bodySmall)
                    }
                }
                jobs.endOfList?.let {
                    Text(it.text(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.testTag("jobs-end"))
                }
            }
        }
    }
}

@Composable
private fun JobCard(jobs: JobListVm, row: JobRow, send: (AppIntent) -> Unit, job: (JobsIntent) -> Unit) {
    Card(
        Modifier.fillMaxWidth().clickable { send(AppIntent.Navigate(Route.Home(HomeView.Job(row.id)))) }.testTag("job-row-${row.id}"),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    row.displayName ?: jobs.untitled.text(),
                    Modifier.weight(1f),
                    maxLines = 1,
                    style = MaterialTheme.typography.bodyLarge,
                )
                IconButton(onClick = { job(JobsIntent.DeleteRequested(row.id)) }, modifier = Modifier.size(48.dp).testTag("job-delete-${row.id}")) {
                    Icon(Icons.Default.Delete, contentDescription = tk(Ns.APP, "Delete"))
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                if (row.isTranslation) Text(tk(Ns.APP, "p5.history.translation_job"), style = MaterialTheme.typography.labelSmall)
                Text(row.duration ?: jobs.notAvailable.text(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                row.speakers?.let { Text(it.text(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                Text(row.dateText, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (row.spinner) CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = row.tone.color())
                Text(row.statusLabel.text(), style = MaterialTheme.typography.labelMedium, color = row.tone.color(), modifier = Modifier.testTag("job-status-${row.id}"))
            }
            if (row.canDownload || row.canFeedback) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (row.canDownload) {
                        TextButton(
                            onClick = { job(JobsIntent.DownloadRequested(row.id)) },
                            enabled = !row.downloading,
                            modifier = Modifier.heightIn(min = 48.dp).testTag("job-download-${row.id}"),
                        ) { Text(tk(Ns.APP, "Download")) }
                    }
                    if (row.canFeedback) {
                        TextButton(
                            onClick = { send(AppIntent.Feedback(FeedbackIntent.Opened(row.id))) },
                            modifier = Modifier.heightIn(min = 48.dp).testTag("job-feedback-${row.id}"),
                        ) { Text(tk(Ns.APP, "Feedback")) }
                    }
                }
            }
        }
    }
}
