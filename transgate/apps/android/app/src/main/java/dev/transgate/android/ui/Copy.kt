package dev.transgate.android.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import dev.transgate.core.ArgWire
import dev.transgate.core.FfiException
import dev.transgate.core.Msg
import dev.transgate.core.MsgWire
import dev.transgate.core.Ns
import dev.transgate.core.translate

/**
 * All copy goes through core's strict `translate()` over the web catalogs (D3): the shell
 * never holds a string of its own. A view-model [Msg] is translated as it arrives; a fixed
 * label names its catalog namespace and literal key, exactly as the web's `i18n.tk` does.
 * A message core cannot translate is a bug, shown as its key rather than hidden by a
 * fallback.
 */
class Copy(val locale: String) {
    private val cache = HashMap<MsgWire, String>()

    fun msg(message: Msg): String = cache.getOrPut(message) {
        try {
            translate(locale, message)
        } catch (_: FfiException) {
            "${message.ns.name.lowercase()}:${message.key}"
        }
    }

    fun tk(ns: Ns, key: String, vararg args: Pair<String, ArgWire>): String = msg(MsgWire(ns, key, mapOf(*args)))
}

val LocalCopy = compositionLocalOf<Copy> { error("no Copy provided") }

/** A fixed label from the catalogs: `tk(Ns.COMMON, "Sign Out")`. */
@Composable
fun tk(ns: Ns, key: String, vararg args: Pair<String, ArgWire>): String = LocalCopy.current.tk(ns, key, *args)

/** A view-model message. */
@Composable
fun Msg.text(): String = LocalCopy.current.msg(this)
