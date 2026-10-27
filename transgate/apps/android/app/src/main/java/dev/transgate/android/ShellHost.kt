package dev.transgate.android

import dev.transgate.android.ports.PickedFiles
import dev.transgate.android.ports.PlayerHost
import dev.transgate.core.Route
import dev.transgate.core.webUrl

/**
 * What a screen may ask of the platform beyond forwarding intents: the mounted player, the
 * file handles the picker produced, and leaving for a browser page. Screens hold no other
 * platform object, so a core view model plus this is everything they render from.
 */
class ShellHost(
    val player: PlayerHost,
    val files: PickedFiles,
    private val openUrl: (String) -> Unit,
    private val webOrigin: String,
) {
    /**
     * A page of the web app in a Custom Tab (D6 handoffs: invoices, API keys). The address is
     * core's `web_url` over the https web origin and the person's locale; the shell holds no path.
     */
    fun openWeb(route: Route, locale: String) = openUrl(webUrl(webOrigin, route, locale))

    /** Any absolute URL (a privacy policy link, a video). */
    fun open(url: String) = openUrl(url)
}
