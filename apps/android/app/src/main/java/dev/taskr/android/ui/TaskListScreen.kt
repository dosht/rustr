package dev.taskr.android.ui

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
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import dev.taskr.core.ColumnAlign
import dev.taskr.core.ColumnView
import dev.taskr.core.ScopeTabView
import dev.taskr.core.TaskIntent
import dev.taskr.core.TaskListView
import dev.taskr.core.TaskRowView
import dev.taskr.core.TaskScope

/** Pure view: renders a TaskListView, reports intents. Only the add draft is local. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TaskListScreen(view: TaskListView?, onIntent: (TaskIntent) -> Unit) {
    Scaffold(topBar = { TopAppBar(title = { Text("taskr") }) }) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            if (view == null) {
                CircularProgressIndicator(Modifier.align(Alignment.Center))
            } else {
                TaskListContent(view, onIntent)
            }
        }
    }
}

@Composable
private fun TaskListContent(view: TaskListView, onIntent: (TaskIntent) -> Unit) {
    var draft by rememberSaveable { mutableStateOf("") }
    val submit = {
        onIntent(TaskIntent.AddSubmitted(draft))
        draft = ""
    }

    Column(Modifier.fillMaxSize()) {
        val selected = view.tabs.indexOfFirst { it.selected }.coerceAtLeast(0)
        PrimaryTabRow(selectedTabIndex = selected) {
            view.tabs.forEach { tab ->
                Tab(
                    selected = tab.selected,
                    onClick = { onIntent(TaskIntent.ScopeChanged(tab.scope)) },
                    text = { Text(tab.label) },
                )
            }
        }

        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                label = { Text("New task") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { submit() }),
                modifier = Modifier.weight(1f),
            )
            Button(onClick = submit) { Text("Add") }
        }

        OutlinedTextField(
            value = view.filter,
            onValueChange = { onIntent(TaskIntent.FilterChanged(it)) },
            label = { Text("Filter") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        )

        val message = view.error ?: view.notice
        if (message != null) {
            MessageBanner(
                text = message,
                isError = view.error != null,
                onDismiss = { onIntent(TaskIntent.MessageDismissed) },
            )
        }

        val empty = view.emptyMessage
        if (empty != null) {
            Text(
                empty,
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(32.dp).align(Alignment.CenterHorizontally),
            )
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(view.rows, key = { it.id.toLong() }) { row ->
                    TaskRow(row, view.columns, onToggle = { onIntent(TaskIntent.ToggleRequested(row.id)) })
                    HorizontalDivider()
                }
            }
        }
    }
}

@Composable
private fun TaskRow(row: TaskRowView, columns: List<ColumnView>, onToggle: () -> Unit) {
    val trailing = columns.indices.filter { columns[it].align == ColumnAlign.RIGHT && !columns[it].wide }
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onToggle).padding(horizontal = 8.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(checked = row.done, onCheckedChange = { onToggle() })
        Text(
            row.title,
            style = MaterialTheme.typography.bodyLarge,
            textDecoration = if (row.done) TextDecoration.LineThrough else null,
            color = if (row.done) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.weight(1f),
        )
        trailing.forEach { i ->
            Text(
                row.cells[i],
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 8.dp),
            )
        }
    }
}

@Composable
private fun MessageBanner(text: String, isError: Boolean, onDismiss: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Surface(
        color = if (isError) colors.errorContainer else colors.secondaryContainer,
        contentColor = if (isError) colors.onErrorContainer else colors.onSecondaryContainer,
        shape = MaterialTheme.shapes.small,
        modifier = Modifier.fillMaxWidth().padding(16.dp, 8.dp),
    ) {
        Row(Modifier.padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(text, modifier = Modifier.weight(1f))
            TextButton(onClick = onDismiss) { Text("Dismiss") }
        }
    }
}

// Previews build views in Kotlin because Layoutlib cannot load the native core.
// On a device, `previewScreens()` from the core returns the same states.
private fun previewView(rows: List<TaskRowView>, error: String? = null) = TaskListView(
    tabs = listOf(
        ScopeTabView(TaskScope.OPEN, "Open (${rows.count { !it.done }})", true),
        ScopeTabView(TaskScope.DONE, "Done (${rows.count { it.done }})", false),
        ScopeTabView(TaskScope.ALL, "All (${rows.size})", false),
    ),
    filter = "",
    columns = listOf(
        ColumnView("ID", ColumnAlign.RIGHT, wide = true),
        ColumnView("TITLE", ColumnAlign.LEFT, wide = false),
        ColumnView("AGE", ColumnAlign.RIGHT, wide = false),
    ),
    rows = rows,
    emptyMessage = if (rows.isEmpty()) "Nothing here yet. Add your first task." else null,
    isLoading = false,
    error = error,
    notice = null,
)

private val sampleRows = listOf(
    TaskRowView(1u, "Write the architecture doc", false, listOf("1", "Write the architecture doc", "3d")),
    TaskRowView(2u, "Try the TUI with vim keys", false, listOf("2", "Try the TUI with vim keys", "1h")),
    TaskRowView(4u, "Generate Swift bindings", false, listOf("4", "Generate Swift bindings", "5m")),
)

@Preview(showBackground = true)
@Composable
private fun SamplePreview() = MaterialTheme { TaskListScreen(previewView(sampleRows)) {} }

@Preview(showBackground = true)
@Composable
private fun EmptyPreview() = MaterialTheme { TaskListScreen(previewView(emptyList())) {} }

@Preview(showBackground = true)
@Composable
private fun ErrorPreview() = MaterialTheme {
    TaskListScreen(previewView(sampleRows, error = "There is already an open task with that title.")) {}
}
