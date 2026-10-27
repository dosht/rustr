package dev.transgate.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
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
import dev.transgate.core.Media
import dev.transgate.core.Ns

/** The audio controls over the mounted player (the web's `PlayerControls`): toggle, seek, elapsed / total. */
@Composable
internal fun AudioControls(media: Media.Audio, host: ShellHost) {
    val player = host.player
    val duration = player.duration
    var scrub by remember { mutableStateOf<Float?>(null) }
    val ready = duration != null && !media.loading && !media.unavailable
    Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp).testTag("player"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = { player.toggle() }, enabled = ready, modifier = Modifier.heightIn(min = 48.dp).testTag("player-toggle")) {
            if (player.playing) Text(tk(Ns.APP, "p5.player.pause")) else Text(tk(Ns.APP, "p5.player.play"))
        }
        if (media.loading) CircularProgressIndicator(Modifier.size(20.dp).testTag("player-loading"))
        val total = duration ?: 0.0
        Slider(
            value = (scrub ?: player.seconds.toFloat()).coerceIn(0f, total.toFloat()),
            onValueChange = { scrub = it },
            onValueChangeFinished = {
                scrub?.let { player.seek(it.toDouble()) }
                scrub = null
            },
            valueRange = 0f..(if (total > 0.0) total.toFloat() else 1f),
            enabled = ready,
            modifier = Modifier.weight(1f).heightIn(min = 48.dp).testTag("player-seek"),
        )
        Text(
            "${clock(scrub?.toDouble() ?: player.seconds)} / ${clock(total)}",
            style = MaterialTheme.typography.labelMedium,
            modifier = Modifier.testTag("player-time"),
        )
    }
}

/** `HH:MM:SS` digits, whole seconds rounded down (the web's `formatClock`). */
private fun clock(seconds: Double): String {
    val whole = seconds.toLong().coerceAtLeast(0)
    return String.format(java.util.Locale.ROOT, "%02d:%02d:%02d", whole / 3600, whole % 3600 / 60, whole % 60)
}
