package app.winklet

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color

/**
 * One colour theme, as on the desktop. The island stays dark in every theme; a theme sets
 * its base, the text tint and the accent colours.
 */
data class Palette(
    val name: String,
    val island: Color,
    val surface: Color,
    val stroke: Color,
    val text: Color,
    val textDim: Color,
    val textFaint: Color,
    val accent: Color,
    val accent2: Color,
    val gradA: Color,
    val gradB: Color,
    val gradC: Color,
)

private fun c(argbHex: Long) = Color(argbHex)
/** "#rrggbbaa" order as in theme.slint, written as 0xRRGGBB plus alpha. */
private fun ca(rgb: Long, a: Int) = Color(((a.toLong() shl 24) or rgb))

val Palettes = listOf(
    Palette("Classic", c(0xFF000000), ca(0xffffff, 0x0d), ca(0xffffff, 0x14), c(0xFFF5F5F7), c(0xFFA1A1AA), c(0xFF6B6B76), c(0xFF8B7BFF), c(0xFF4F8DFF), c(0xFFA78BFA), c(0xFF6366F1), c(0xFF3B82F6)),
    Palette("Racing Green", c(0xFF06110C), ca(0xffffff, 0x0d), ca(0xffffff, 0x14), c(0xFFF3EFE6), c(0xFFA7ADA6), c(0xFF6A736C), c(0xFFD93A35), c(0xFF2F7A57), c(0xFFEF5A4C), c(0xFFC22A26), c(0xFFA31C1B)),
    Palette("Burgundy Sage", c(0xFF140706), ca(0xe8d2b5, 0x0f), ca(0xe8d2b5, 0x1a), c(0xFFF1E3CF), c(0xFFA5A79B), c(0xFF77756A), c(0xFFC2423A), c(0xFFA5A79B), c(0xFFE8D2B5), c(0xFFB8433A), c(0xFF590907)),
    Palette("Acid Lime", c(0xFF030407), ca(0xffffff, 0x0d), ca(0xffffff, 0x14), c(0xFFF4F6FB), c(0xFFA2A8B8), c(0xFF646A7A), c(0xFFD4FF24), c(0xFF3D7BFF), c(0xFFD4FF24), c(0xFF4F8DFF), c(0xFF1558C9)),
    Palette("Orchid Sunset", c(0xFF16051D), ca(0xe7b0de, 0x12), ca(0xe7b0de, 0x1c), c(0xFFFBEEFA), c(0xFFC4A6C4), c(0xFF806582), c(0xFFFF6A1F), c(0xFFE7B0DE), c(0xFFE7B0DE), c(0xFFFF8A4A), c(0xFFFF5A0A)),
    Palette("Chrome Laser", c(0xFF080808), ca(0xcdcbca, 0x10), ca(0xcdcbca, 0x1a), c(0xFFF2F1F0), c(0xFFA8A6A5), c(0xFF6B6968), c(0xFFEE2C26), c(0xFFCDCBCA), c(0xFFCDCBCA), c(0xFFEE2C26), c(0xFFD41514)),
    Palette("Coral Steel", c(0xFF17191C), ca(0x929da7, 0x14), ca(0x929da7, 0x1f), c(0xFFF1F3F5), c(0xFFA4ADB6), c(0xFF6C747C), c(0xFFFF4B32), c(0xFF929DA7), c(0xFFFF8A6B), c(0xFFFF4B32), c(0xFFE0341F)),
    Palette("Aurora", c(0xFF060A17), ca(0x6aa8ff, 0x12), ca(0x6aa8ff, 0x1f), c(0xFFEEF3FF), c(0xFF9FABC6), c(0xFF626E8A), c(0xFF4F8DFF), c(0xFFF08A4B), c(0xFF7FB2FF), c(0xFF4F8DFF), c(0xFF2E5BFF)),
    Palette("Graphite", c(0xFF0C0C0E), ca(0xffffff, 0x0f), ca(0xffffff, 0x17), c(0xFFF5F5F7), c(0xFFA3A3AB), c(0xFF67676F), c(0xFF3B7BFF), c(0xFF8A8A94), c(0xFF5B93FF), c(0xFF3B7BFF), c(0xFF2563EB)),
)

/** Colours one surface is drawn with: the theme, in dark or the warm paper light mode. */
data class Tokens(
    val palette: Palette,
    val light: Boolean,
) {
    val ink = if (light) Color(0xFF3B3226) else Color.White
    val island = if (light) Color(0xFFE3DDD1) else palette.island
    val surface = if (light) ca(0x3b3226, 0x0f) else palette.surface
    val stroke = if (light) ca(0x3b3226, 0x1a) else palette.stroke
    val text = if (light) Color(0xFF2E2923) else palette.text
    val textDim = if (light) Color(0xFF615A50) else palette.textDim
    val textFaint = if (light) Color(0xFF8C8478) else palette.textFaint
    val accent = if (light) darker(palette.accent) else palette.accent
    val accent2 = if (light) darker(palette.accent2) else palette.accent2
    val grad = Brush.linearGradient(listOf(palette.gradA, palette.gradB, palette.gradC))
    val green = if (light) Color(0xFF1F9466) else Color(0xFF34D399)
    val orange = if (light) Color(0xFFCC6A1D) else Color(0xFFFB923C)
    val red = if (light) Color(0xFFCF4545) else Color(0xFFF87171)
    val blue = if (light) Color(0xFF1F86BD) else Color(0xFF38BDF8)
    val pink = if (light) Color(0xFFC8447F) else Color(0xFFF472B6)

    fun wash(a: Float) = ink.copy(alpha = a)

    private fun darker(c: Color) = Color(c.red * 0.7f, c.green * 0.7f, c.blue * 0.7f, c.alpha)
}

val LocalTokens = staticCompositionLocalOf { Tokens(Palettes[0], false) }

/** Provides the current theme's tokens; [panel] = the open panel, which may go light. */
@Composable
fun WinkletTheme(panel: Boolean, content: @Composable () -> Unit) {
    val theme by Prefs.theme.state.collectAsState()
    val light by Prefs.lightMode.state.collectAsState()
    val tokens = Tokens(Palettes[theme.coerceIn(0, Palettes.lastIndex)], light && panel)
    CompositionLocalProvider(LocalTokens provides tokens, content = content)
}
