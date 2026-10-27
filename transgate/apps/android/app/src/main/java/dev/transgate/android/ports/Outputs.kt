package dev.transgate.android.ports

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import androidx.browser.customtabs.CustomTabsIntent
import androidx.core.content.FileProvider
import dev.transgate.core.ShareResult
import java.io.File

/**
 * The output ports: clipboard, save, preview, share and leaving the app. Each is a plain
 * platform call; what is copied, named or exported is decided by core. Launches start from
 * the application context, so they carry NEW_TASK and never depend on a live Activity.
 */
class Outputs(private val context: Context) {
    fun copy(text: String, html: String?): Boolean = try {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(if (html != null) ClipData.newHtmlText("transgate", text, html) else ClipData.newPlainText("transgate", text))
        true
    } catch (_: Exception) {
        false
    }

    /** Writes into the shared Downloads collection (API 29+ needs no storage permission). */
    fun save(bytes: ByteArray, filename: String, mime: String): Boolean = try {
        val values = android.content.ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, filename)
            put(MediaStore.Downloads.MIME_TYPE, mime)
            put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
        }
        val uri = context.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
        uri != null && context.contentResolver.openOutputStream(uri)?.use { it.write(bytes); true } == true
    } catch (_: Exception) {
        false
    }

    fun preview(bytes: ByteArray, filename: String, mime: String): Boolean = try {
        val uri = stage(bytes, filename)
        context.startActivity(
            Intent(Intent.ACTION_VIEW).setDataAndType(uri, mime).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        true
    } catch (_: Exception) {
        false
    }

    /** The system chooser does not report which target was used; a launched sheet counts as shared. */
    fun share(bytes: ByteArray, filename: String, mime: String): ShareResult = try {
        val uri = stage(bytes, filename)
        val send = Intent(Intent.ACTION_SEND).setType(mime).putExtra(Intent.EXTRA_STREAM, uri).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        context.startActivity(Intent.createChooser(send, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        ShareResult.SHARED
    } catch (_: Exception) {
        ShareResult.UNSUPPORTED
    }

    /** D7/D11: a Custom Tab for authorization pages, the browser or app for any other URL. */
    fun leave(url: String) {
        val tab = CustomTabsIntent.Builder().build()
        tab.intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            tab.launchUrl(context, Uri.parse(url))
        } catch (_: Exception) {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }

    private fun stage(bytes: ByteArray, filename: String): Uri {
        val dir = File(context.cacheDir, "outgoing").apply { mkdirs() }
        val file = File(dir, filename.replace('/', '_')).apply { writeBytes(bytes) }
        return FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    }
}
