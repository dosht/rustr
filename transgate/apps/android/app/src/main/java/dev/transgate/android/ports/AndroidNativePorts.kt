package dev.transgate.android.ports

import android.content.Context
import dev.transgate.core.CalendarOffsets
import dev.transgate.core.NativePorts
import dev.transgate.core.ProgressSink
import dev.transgate.core.ShareResult
import java.util.Calendar
import java.util.TimeZone

/**
 * Everything core asks of the platform (the web's `Platform`), fulfilled with Android:
 * Keystore storage (D8), Custom Tabs (D7), content-URI files and the signed PUT, the one
 * mounted player, clipboard/save/preview/share, and the bundled assets (D10). It decides
 * nothing: a failure is reported as its failure value and core chooses what the person sees.
 *
 * `reload` is the platform's "load this app URL afresh" (what a browser does on a locale
 * switch): core's `Leave` with an app path restarts the presenter at that path, anything
 * with a scheme leaves the app.
 */
class AndroidNativePorts(
    private val context: Context,
    private val vault: KeystoreVault,
    val files: PickedFiles,
    val player: PlayerHost,
    private val outputs: Outputs,
    private val reload: (String) -> Unit,
) : NativePorts {
    override fun storageGet(key: String): String? = vault.get(key)

    override fun storageSet(key: String, value: String) = vault.set(key, value)

    override fun storageRemove(key: String) = vault.remove(key)

    /** The route lives in core's view model; there is no address bar to keep in step. */
    override fun navigate(path: String, replace: Boolean) {}

    override fun leave(url: String) {
        if (url.startsWith("/")) reload(url) else outputs.leave(url)
    }

    override fun calendar(): CalendarOffsets {
        val zone = TimeZone.getDefault()
        val now = Calendar.getInstance(zone)
        val then = (now.clone() as Calendar).apply { add(Calendar.MONTH, -1) }
        return CalendarOffsets(
            nowMinutes = zone.getOffset(now.timeInMillis) / 60_000,
            thenMinutes = zone.getOffset(then.timeInMillis) / 60_000,
        )
    }

    override fun measure(file: ULong): Double? = files.measure(file)

    override fun upload(file: ULong, url: String, progress: ProgressSink): Boolean = files.put(file, url, progress)

    override fun release(file: ULong) = files.release(file)

    override fun pausePlayer(): Boolean = player.pauseForPort()

    override fun resumePlayer() = player.resumeForPort()

    override fun seekPlayer(seconds: Double) = player.seek(seconds)

    override fun copy(text: String, html: String?): Boolean = outputs.copy(text, html)

    override fun save(bytes: ByteArray, filename: String, mime: String): Boolean = outputs.save(bytes, filename, mime)

    override fun preview(bytes: ByteArray, filename: String, mime: String): Boolean = outputs.preview(bytes, filename, mime)

    override fun share(bytes: ByteArray, filename: String, mime: String): ShareResult = outputs.share(bytes, filename, mime)

    override fun asset(name: String): ByteArray? = try {
        context.assets.open(name).use { it.readBytes() }
    } catch (_: Exception) {
        null
    }
}
