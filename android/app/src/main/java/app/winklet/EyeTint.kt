package app.winklet

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp

/** One colour for the eyes: what they're filled with and the glow around them. */
data class EyeColor(val name: String, val fill: Color, val glow: Color)

/** The same palette as the desktop's Settings > Look. Ice is the original look; Theme follows the accent. */
object EyeTint {
    val colors = listOf(
        EyeColor("Ice", Color(0xFFFFFFFF), Color(0xFF7CC8FF)),
        EyeColor("Aqua", Color(0xFFB8FBFF), Color(0xFF22D3EE)),
        EyeColor("Mint", Color(0xFFB4FFD8), Color(0xFF10B981)),
        EyeColor("Lime", Color(0xFFE6FF8A), Color(0xFF84CC16)),
        EyeColor("Sun", Color(0xFFFFE08A), Color(0xFFF59E0B)),
        EyeColor("Peach", Color(0xFFFFC9A8), Color(0xFFFF7A45)),
        EyeColor("Rose", Color(0xFFFFB3CF), Color(0xFFF43F5E)),
        EyeColor("Lilac", Color(0xFFDDD0FF), Color(0xFF8B5CF6)),
        EyeColor("Sky", Color(0xFFC2DCFF), Color(0xFF3B82F6)),
        EyeColor("Ember", Color(0xFFFFB0A3), Color(0xFFEF4444)),
    )

    /** Ten fixed colours plus Theme (index 10). */
    const val COUNT = 11

    fun name(i: Int) = if (i >= colors.size) "Theme" else colors[i].name

    fun get(i: Int, palette: Palette): EyeColor =
        if (i >= colors.size) {
            EyeColor("Theme", lerp(palette.accent, Color.White, 0.45f), palette.accent)
        } else {
            colors[i.coerceAtLeast(0)]
        }
}
