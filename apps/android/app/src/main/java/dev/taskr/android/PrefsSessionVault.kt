package dev.taskr.android

import android.content.Context
import dev.taskr.core.SessionVault

/**
 * The session port, fulfilled by app-private preferences. Other apps cannot
 * read them; a production build should wrap the value with a Keystore key.
 */
class PrefsSessionVault(context: Context) : SessionVault {
    private val prefs = context.getSharedPreferences("taskr.session", Context.MODE_PRIVATE)

    override fun load(): String? = prefs.getString(KEY, null)

    override fun save(value: String) {
        prefs.edit().putString(KEY, value).apply()
    }

    override fun clear() {
        prefs.edit().remove(KEY).apply()
    }

    private companion object {
        const val KEY = "session"
    }
}
