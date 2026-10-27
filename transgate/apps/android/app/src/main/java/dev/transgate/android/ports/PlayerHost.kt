package dev.transgate.android.ports

import android.media.AudioAttributes
import android.media.MediaPlayer
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableDoubleStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * The one mounted audio player (the web's `MediaPlayer` platform): the shell plays what the
 * transcript's `Media::Audio` URL names and reports `playing` back to core; whether a
 * transcript has audio, and what an unavailable one says, is core's. Calls come from the
 * driver's blocking threads (pause/resume/seek ports) and from the UI, so everything is
 * posted to the main looper, where `MediaPlayer` lives. D11: a YouTube transcript has no
 * embedded player; seeking it opens the video at that time in the YouTube app or browser.
 */
class PlayerHost(private val onPlaying: (Boolean) -> Unit, private val openUrl: (String) -> Unit) {
    private val main = Handler(Looper.getMainLooper())
    private var player: MediaPlayer? = null
    private var url: String? = null
    private var prepared = false

    @Volatile private var youtubeId: String? = null

    @Volatile private var playingNow = false

    /** Observable by the UI: seconds into the audio and the whole length (null until known). */
    var seconds by mutableDoubleStateOf(0.0)
        private set
    var duration by mutableStateOf<Double?>(null)
        private set
    var playing by mutableStateOf(false)
        private set
    var failed by mutableStateOf(false)
        private set

    private val ticker = object : Runnable {
        override fun run() {
            player?.takeIf { prepared }?.let { seconds = it.currentPosition / 1000.0 }
            if (playingNow) main.postDelayed(this, 250)
        }
    }

    /** Mounts the YouTube video whose time links open outside the app; `null` clears it. */
    fun mountYouTube(videoId: String?) {
        youtubeId = videoId
    }

    /** Mounts `source` when it changes; `null` unmounts. */
    fun mount(source: String?) {
        main.post {
            if (source == url) return@post
            release()
            url = source
            failed = false
            if (source == null) return@post
            val fresh = MediaPlayer()
            player = fresh
            fresh.setAudioAttributes(AudioAttributes.Builder().setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).setUsage(AudioAttributes.USAGE_MEDIA).build())
            fresh.setOnPreparedListener {
                prepared = true
                duration = it.duration.takeIf { ms -> ms > 0 }?.let { ms -> ms / 1000.0 }
            }
            fresh.setOnCompletionListener { applyPlaying(false) }
            fresh.setOnErrorListener { _, _, _ ->
                failed = true
                applyPlaying(false)
                true
            }
            try {
                fresh.setDataSource(source)
                fresh.prepareAsync()
            } catch (_: Exception) {
                failed = true
            }
        }
    }

    fun toggle() {
        main.post { if (playingNow) pauseNow() else resumeNow() }
    }

    fun seek(to: Double) {
        val video = youtubeId
        if (video != null) {
            openUrl("https://www.youtube.com/watch?v=$video&t=${to.toInt()}s")
            return
        }
        main.post {
            player?.takeIf { prepared }?.seekTo((to * 1000).toInt())
            seconds = to
        }
    }

    /** Port: pauses for a modal action; whether it was playing before. */
    fun pauseForPort(): Boolean {
        val was = playingNow
        if (was) main.post { pauseNow() }
        return was
    }

    fun resumeForPort() {
        main.post { resumeNow() }
    }

    fun unmount() {
        main.post {
            release()
            url = null
        }
    }

    private fun resumeNow() {
        val active = player?.takeIf { prepared } ?: return
        active.start()
        applyPlaying(true)
    }

    private fun pauseNow() {
        player?.takeIf { prepared }?.pause()
        applyPlaying(false)
    }

    private fun applyPlaying(value: Boolean) {
        if (playingNow == value) return
        playingNow = value
        playing = value
        if (value) main.post(ticker) else main.removeCallbacks(ticker)
        onPlaying(value)
    }

    private fun release() {
        main.removeCallbacks(ticker)
        player?.release()
        player = null
        prepared = false
        duration = null
        seconds = 0.0
        if (playingNow) applyPlaying(false)
    }
}
