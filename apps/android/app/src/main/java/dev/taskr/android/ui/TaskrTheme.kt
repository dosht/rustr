package dev.taskr.android.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

@Composable
fun TaskrTheme(
    dark: Boolean = isSystemInDarkTheme(),
    palette: String = DesignTokens.defaultPalette,
    content: @Composable () -> Unit,
) {
    MaterialTheme(colorScheme = DesignTokens.colors(dark, palette)) {
        Surface(modifier = Modifier.fillMaxSize()) { content() }
    }
}
