package dev.transgate.android.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import dev.transgate.core.Tone

val LocalSemantic = compositionLocalOf<SemanticColors> { error("no SemanticColors provided") }

/** The token color of core's semantic tone; views never hold a color literal. */
@Composable
fun Tone.color(): Color = LocalSemantic.current.tone(this)

/**
 * The generated Paper palettes (`design-tokens.json`, D12): the Material scheme for stock
 * components and the sixteen semantic roles for everything core names by tone. The locale
 * is the person's UI language from the view model.
 */
@Composable
fun TransgateTheme(locale: String, content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val copy = remember(locale) { Copy(locale) }
    CompositionLocalProvider(
        LocalCopy provides copy,
        LocalSemantic provides remember(dark) { DesignTokens.semantic(dark) },
    ) {
        MaterialTheme(colorScheme = DesignTokens.colors(dark), content = content)
    }
}
