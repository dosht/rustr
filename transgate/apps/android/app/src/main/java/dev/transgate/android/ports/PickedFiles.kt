package dev.transgate.android.ports

import android.content.Context
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.provider.OpenableColumns
import dev.transgate.core.PickedFile
import dev.transgate.core.ProgressSink
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/**
 * Files the person picked, held by handle until core releases them (UPL-001/013): the shell
 * keeps the content URI, core only ever sees the handle, name, size and type. Measuring and
 * the one signed PUT run here because only the platform can read the file.
 */
class PickedFiles(private val context: Context) {
    private class Held(val uri: Uri, val mime: String)

    private val held = ConcurrentHashMap<Long, Held>()
    private val next = AtomicLong(1)

    fun hold(uris: List<Uri>): List<PickedFile> = uris.map { uri ->
        var name = uri.lastPathSegment ?: "file"
        var size = 0L
        context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME).takeIf { it >= 0 }?.let { name = cursor.getString(it) }
                cursor.getColumnIndex(OpenableColumns.SIZE).takeIf { it >= 0 }?.let { size = cursor.getLong(it) }
            }
        }
        val mime = context.contentResolver.getType(uri) ?: ""
        val handle = next.getAndIncrement()
        held[handle] = Held(uri, mime)
        PickedFile(handle = handle.toULong(), name = name, size = size, mime = mime)
    }

    /** Seconds from the container's metadata, or null when the file cannot be measured. */
    fun measure(handle: ULong): Double? {
        val file = held[handle.toLong()] ?: return null
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(context, file.uri)
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull()?.let { it / 1000.0 }
        } catch (_: Exception) {
            null
        } finally {
            retriever.release()
        }
    }

    /** One PUT of the held file to the signed URL; true only for HTTP 200 (UPL-013/014). */
    fun put(handle: ULong, url: String, progress: ProgressSink): Boolean {
        val file = held[handle.toLong()] ?: return false
        return try {
            val total = context.contentResolver.openAssetFileDescriptor(file.uri, "r")?.use { it.length } ?: -1L
            val connection = URL(url).openConnection() as HttpURLConnection
            try {
                connection.requestMethod = "PUT"
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", file.mime.ifEmpty { "application/octet-stream" })
                if (total >= 0) connection.setFixedLengthStreamingMode(total) else connection.setChunkedStreamingMode(0)
                progress.report(0u)
                context.contentResolver.openInputStream(file.uri)?.use { input ->
                    connection.outputStream.use { output ->
                        val buffer = ByteArray(64 * 1024)
                        var sent = 0L
                        var last = -1
                        while (true) {
                            val read = input.read(buffer)
                            if (read < 0) break
                            output.write(buffer, 0, read)
                            sent += read
                            val percent = if (total > 0) (sent * 100 / total).toInt() else 0
                            if (percent != last) {
                                last = percent
                                progress.report(percent.toUByte())
                            }
                        }
                    }
                } ?: return false
                connection.responseCode == 200
            } finally {
                connection.disconnect()
            }
        } catch (_: Exception) {
            false
        }
    }

    fun release(handle: ULong) {
        held.remove(handle.toLong())
    }
}
