package dev.transgate.android

import dev.transgate.android.ports.PickedFiles
import dev.transgate.android.ports.PlayerHost

/**
 * What a screen may ask of the platform beyond forwarding intents: the mounted player, the
 * file handles the picker produced, and leaving for a browser page. Screens hold no other
 * platform object, so a core view model plus this is everything they render from.
 */
class ShellHost(
    val player: PlayerHost,
    val files: PickedFiles,
    private val openUrl: (String) -> Unit,
    private val webUrl: String,
) {
    /** A page of the web app in a Custom Tab, signed in by the shared browser session (D6 handoffs). */
    fun openWeb(path: String) = openUrl(webUrl.trimEnd('/') + path)

    /** Any absolute URL (a privacy policy link, a video). */
    fun open(url: String) = openUrl(url)
}
