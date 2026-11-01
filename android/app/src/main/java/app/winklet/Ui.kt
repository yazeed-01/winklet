package app.winklet

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** A rounded tile on the panel. */
@Composable
fun Card(modifier: Modifier = Modifier, onClick: (() -> Unit)? = null, content: @Composable ColumnScope.() -> Unit) {
    val t = LocalTokens.current
    val shape = RoundedCornerShape(16.dp)
    Column(
        modifier
            .clip(shape)
            .background(t.surface)
            .border(1.dp, t.stroke, shape)
            .let { if (onClick != null) it.clickable(onClick = onClick) else it }
            .padding(12.dp),
        content = content,
    )
}

@Composable
fun Label(text: String, modifier: Modifier = Modifier, size: TextUnit = 12.sp, color: Color? = null, weight: FontWeight? = null, align: TextAlign? = null, maxLines: Int = 4) {
    val t = LocalTokens.current
    Text(text, modifier, color = color ?: t.textDim, fontSize = size, fontWeight = weight, textAlign = align, maxLines = maxLines)
}

@Composable
fun Big(text: String, size: TextUnit = 22.sp, color: Color? = null, modifier: Modifier = Modifier) {
    val t = LocalTokens.current
    Text(text, modifier, color = color ?: t.text, fontSize = size, fontWeight = FontWeight.SemiBold, maxLines = 1)
}

/** A small pill button. [filled] = accent background; [compact] = tight padding for rows of many. */
@Composable
fun Pill(text: String, filled: Boolean = false, modifier: Modifier = Modifier, color: Color? = null, compact: Boolean = false, onClick: () -> Unit) {
    val t = LocalTokens.current
    val shape = RoundedCornerShape(50)
    val bg = color ?: t.accent
    Box(
        modifier
            .clip(shape)
            .background(if (filled) bg else t.surface)
            .border(1.dp, if (filled) bg else t.stroke, shape)
            .clickable(onClick = onClick)
            .padding(horizontal = if (compact) 2.dp else 10.dp, vertical = 6.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = if (filled) onColor(bg) else t.text, fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

/** Black on bright accents (Acid Lime), white on the rest. */
fun onColor(c: Color): Color = if (0.299f * c.red + 0.587f * c.green + 0.114f * c.blue > 0.6f) Color(0xFF111111) else Color.White

/** A progress ring with a value in the middle. */
@Composable
fun Ring(
    fraction: Float,
    value: String,
    label: String,
    color: Color,
    size: Dp = 64.dp,
    stroke: Dp = 6.dp,
) {
    val t = LocalTokens.current
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Box(Modifier.size(size), contentAlignment = Alignment.Center) {
            Canvas(Modifier.size(size)) {
                val sw = stroke.toPx()
                val arc = Size(this.size.width - sw, this.size.height - sw)
                val tl = Offset(sw / 2, sw / 2)
                drawArc(t.wash(0.08f), 0f, 360f, false, tl, arc, style = Stroke(sw))
                drawArc(color, -90f, 360f * fraction.coerceIn(0f, 1f), false, tl, arc, style = Stroke(sw, cap = StrokeCap.Round))
            }
            Text(value, color = t.text, fontSize = (size.value / 4.6f).sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        }
        Label(label, Modifier.padding(top = 4.dp), size = 11.sp)
    }
}

/** Simple bars; [highlight] marks one (today, this hour). */
@Composable
fun Bars(
    values: List<Float>,
    modifier: Modifier = Modifier,
    color: Color? = null,
    highlight: Int = -1,
    goal: Float? = null,
    labels: List<String>? = null,
) {
    val t = LocalTokens.current
    val c = color ?: t.accent
    Column(modifier) {
        Canvas(Modifier.fillMaxWidth().height(56.dp)) {
            val max = maxOf(values.maxOrNull() ?: 0f, goal ?: 0f, 1f)
            val n = values.size.coerceAtLeast(1)
            val slot = size.width / n
            val bw = (slot * 0.62f).coerceAtMost(18.dp.toPx())
            values.forEachIndexed { i, v ->
                val h = (v / max * size.height).coerceAtLeast(2.dp.toPx())
                drawRoundRect(
                    if (i == highlight || highlight < 0) c else c.copy(alpha = 0.45f),
                    Offset(i * slot + (slot - bw) / 2, size.height - h), Size(bw, h), CornerRadius(bw / 3),
                )
            }
            goal?.let {
                val y = size.height - it / max * size.height
                drawLine(t.wash(0.25f), Offset(0f, y), Offset(size.width, y), 1.dp.toPx())
            }
        }
        if (labels != null) {
            Row(Modifier.fillMaxWidth().padding(top = 4.dp)) {
                labels.forEach { Label(it, Modifier.weight(1f), size = 10.sp, align = TextAlign.Center) }
            }
        }
    }
}

/** A thin line chart of recent values (0..100). */
@Composable
fun Spark(values: List<Float>, color: Color, modifier: Modifier = Modifier) {
    Canvas(modifier.fillMaxWidth().height(40.dp)) {
        if (values.size < 2) return@Canvas
        val step = size.width / (values.size - 1)
        for (i in 1 until values.size) {
            drawLine(
                color,
                Offset((i - 1) * step, size.height * (1 - values[i - 1] / 100f)),
                Offset(i * step, size.height * (1 - values[i] / 100f)),
                2.dp.toPx(), StrokeCap.Round,
            )
        }
    }
}

/** label ............ value */
@Composable
fun StatRow(label: String, value: String, valueColor: Color? = null) {
    val t = LocalTokens.current
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), horizontalArrangement = Arrangement.SpaceBetween) {
        Label(label, size = 13.sp)
        Text(value, color = valueColor ?: t.text, fontSize = 13.sp, fontWeight = FontWeight.Medium)
    }
}

/** Round tick box for tasks. */
@Composable
fun Tick(on: Boolean, onClick: () -> Unit) {
    val t = LocalTokens.current
    Box(
        Modifier
            .size(22.dp)
            .clip(RoundedCornerShape(50))
            .background(if (on) t.accent else Color.Transparent)
            .border(1.5.dp, if (on) t.accent else t.textFaint, RoundedCornerShape(50))
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        if (on) Text("✓", color = onColor(t.accent), fontSize = 13.sp, fontWeight = FontWeight.Bold)
    }
}

fun bytes(b: Long): String = when {
    b >= 1L shl 30 -> "%.1f GB".format(b / (1L shl 30).toFloat())
    b >= 1L shl 20 -> "%.0f MB".format(b / (1L shl 20).toFloat())
    b >= 1L shl 10 -> "%.0f KB".format(b / (1L shl 10).toFloat())
    else -> "$b B"
}

fun duration(sec: Int): String {
    val h = sec / 3600
    val m = sec % 3600 / 60
    return if (h > 0) "${h}h ${m}m" else "${m}m"
}
