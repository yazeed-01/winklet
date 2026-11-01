package app.winklet

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import kotlin.math.PI
import kotlin.math.sin

/**
 * A cup or bottle drawn in a 40 × 56 box, the same shapes as the desktop. [level] = where
 * the drink's surface sits (viewbox y).
 */
private class CupKind(val name: String, val ml: Int, body: String, extra: String, val level: Float) {
    val body: Path = PathParser().parsePathString(body).toPath()
    val extra: Path? = extra.takeIf { it.isNotEmpty() }?.let { PathParser().parsePathString(it).toPath() }
}

private val Cups = listOf(
    CupKind("Shot", 40, "M13 37 H27 V47 Q27 52 22 52 H18 Q13 52 13 47 Z", "M27 40 Q32 40 32 44 Q32 48 27 48", 41f),
    CupKind("Tea glass", 150, "M13 28 H27 L25 52 H15 Z", "", 33f),
    CupKind("Cup", 250, "M10 23 H27 V47 Q27 52 22 52 H15 Q10 52 10 47 Z", "M27 29 Q34 29 34 36 Q34 43 27 43", 28f),
    CupKind("Big mug", 350, "M9 14 H28 V47 Q28 52 23 52 H14 Q9 52 9 47 Z", "M28 21 Q36 21 36 30 Q36 39 28 39", 20f),
    CupKind("Tall glass", 400, "M11 10 H29 L27 52 H13 Z", "M25 3 L22 22", 15f),
    CupKind("Bottle", 500, "M17 9 H23 V14 Q30 17 30 23 V50 Q30 52 28 52 H12 Q10 52 10 50 V23 Q10 17 17 14 Z", "M16 5 H24 V9 H16 Z", 22f),
    CupKind("Big bottle", 1000, "M17 7 H23 V11 Q33 14 33 21 V50 Q33 52 31 52 H9 Q7 52 7 50 V21 Q7 14 17 11 Z", "M16 2 H24 V7 H16 Z", 15f),
)

private val DayName = DateTimeFormatter.ofPattern("EEE")

/** What the Water tab is showing below the bottle. */
private sealed interface Sheet {
    data object Drinks : Sheet
    data class Pick(val id: String) : Sheet
    /** [id] null = a new drink. */
    data class Edit(val id: String?) : Sheet
}

@Composable
fun WaterPage() {
    val t = LocalTokens.current
    val data by Store.data.collectAsState()
    val goal by Prefs.waterGoal.state.collectAsState()
    var sheet by remember { mutableStateOf<Sheet>(Sheet.Drinks) }
    val have = data.waterToday()

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 8.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Bottle(data, goal, Modifier.size(78.dp, 150.dp))
            Column(Modifier.weight(1f)) {
                Big("$have ml", size = 30.sp)
                Label("of $goal ml today", size = 13.sp)
                Spacer(Modifier.height(8.dp))
                val behind = Reminders.pace() - have
                Label(
                    when {
                        have >= goal -> "Goal reached. Well done!"
                        behind > 0 -> "$behind ml behind pace"
                        else -> "On track · ${goal - have} ml to go"
                    },
                    size = 13.sp, color = if (have >= goal) t.green else if (behind > 250) t.orange else t.textDim,
                )
                NextReminder()
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Pill("+ cup", filled = true, color = t.blue) { log("water", 250, have, goal) }
                    if (data.sips.isNotEmpty()) Pill("Undo") { Store.undoSip() }
                }
            }
        }

        when (val s = sheet) {
            Sheet.Drinks -> DrinkTiles(data, onPick = { sheet = Sheet.Pick(it) }, onNew = { sheet = Sheet.Edit(null) })
            is Sheet.Pick -> SizePicker(
                data.drink(s.id), data.lastSize[s.id] ?: data.drink(s.id).ml,
                onAdd = { ml ->
                    log(s.id, ml, have, goal)
                    sheet = Sheet.Drinks
                },
                onEdit = { sheet = Sheet.Edit(s.id) },
                onBack = { sheet = Sheet.Drinks },
            )
            is Sheet.Edit -> DrinkEditor(
                s.id?.let { data.drink(it) },
                data.drinks,
                onDone = { sheet = Sheet.Drinks },
            )
        }

        Label("Last 7 days", size = 12.sp)
        val days = (6 downTo 0).map { LocalDate.now().minusDays(it.toLong()) }
        Bars(
            days.map { d -> (if (d == LocalDate.now()) have else data.days[d]?.waterMl ?: 0).toFloat() },
            color = t.blue, highlight = 6, goal = goal.toFloat(),
            labels = days.map { it.format(DayName) },
        )
    }
}

/** "Next reminder 14:20", refreshed when a drink moves it. */
@Composable
private fun NextReminder() {
    val every by Prefs.waterEvery.state.collectAsState()
    // re-read after each drink, which reschedules
    val data by Store.data.collectAsState()
    val at = remember(data.sips.size, every) { Prefs.waterNextAt }
    if (every == 0 || at <= 0) return
    val time = java.time.Instant.ofEpochMilli(at).atZone(java.time.ZoneId.systemDefault())
    val day = if (time.toLocalDate() == LocalDate.now()) "" else "tomorrow "
    Label("Next reminder $day${time.format(DateTimeFormatter.ofPattern("HH:mm"))}", size = 11.sp, color = LocalTokens.current.textFaint)
}

/** Logs a drink; reaching the goal makes the eyes show hearts. */
private fun log(type: String, ml: Int, before: Int, goal: Int) {
    Store.addSip(type, ml)
    if (before < goal && before + ml >= goal) EyeEvents.play(Mood.Love)
}

/** The drinks in two columns so every name fits, then "New drink". */
@Composable
private fun DrinkTiles(data: Data, onPick: (String) -> Unit, onNew: () -> Unit) {
    val t = LocalTokens.current
    val today = data.sips.map { it.type }.toSet()
    Label("Tap a drink", size = 12.sp)
    val tiles: List<DrinkType?> = data.drinks + null
    tiles.chunked(2).forEach { row ->
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            row.forEach { d ->
                val shape = RoundedCornerShape(12.dp)
                if (d == null) {
                    Box(
                        Modifier
                            .weight(1f)
                            .clip(shape)
                            .border(1.dp, t.accent.copy(alpha = 0.6f), shape)
                            .clickable(onClick = onNew)
                            .padding(vertical = 11.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text("+  New drink", color = t.accent, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    }
                } else {
                    val c = Color(d.color)
                    val had = d.id in today
                    Row(
                        Modifier
                            .weight(1f)
                            .clip(shape)
                            .background(if (had) c.copy(alpha = 0.2f) else t.surface)
                            .border(1.dp, if (had) c.copy(alpha = 0.6f) else t.stroke, shape)
                            .clickable { onPick(d.id) }
                            .padding(horizontal = 10.dp, vertical = 11.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(Modifier.size(9.dp).background(c, CircleShape))
                        Spacer(Modifier.width(8.dp))
                        Text(
                            d.name, color = t.text, fontSize = 13.sp, maxLines = 2, lineHeight = 15.sp,
                            fontWeight = if (had) FontWeight.SemiBold else FontWeight.Normal,
                        )
                    }
                }
            }
            if (row.size == 1) Spacer(Modifier.weight(1f))
        }
    }
}

/** "How much?": drawn cups and bottles (the one from last time is marked) or any amount. */
@Composable
private fun SizePicker(d: DrinkType, last: Int, onAdd: (Int) -> Unit, onEdit: () -> Unit, onBack: () -> Unit) {
    val t = LocalTokens.current
    val tint = Color(d.color)
    var custom by remember(d.id) { mutableIntStateOf(last) }
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(16.dp))
            .background(t.surface)
            .border(1.dp, t.stroke, RoundedCornerShape(16.dp))
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(12.dp).background(tint, CircleShape))
            Spacer(Modifier.width(8.dp))
            Text("${d.name} · how much?", color = t.text, fontSize = 14.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
            Text("Edit", color = t.textDim, fontSize = 13.sp, modifier = Modifier.clickable(onClick = onEdit).padding(6.dp))
            Text("✕", color = t.textDim, fontSize = 15.sp, modifier = Modifier.clickable(onClick = onBack).padding(6.dp))
        }
        Cups.chunked(4).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                row.forEach { c -> CupTile(c, tint, c.ml == last, Modifier.weight(1f)) { onAdd(c.ml) } }
                repeat(4 - row.size) { Spacer(Modifier.weight(1f)) }
            }
        }
        // any other amount
        Row(verticalAlignment = Alignment.CenterVertically) {
            Label("Other amount", size = 12.sp, modifier = Modifier.weight(1f))
            Pill("−") { custom = (custom - if (custom <= 100) 10 else 50).coerceAtLeast(10) }
            Text("$custom ml", color = t.text, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(horizontal = 8.dp))
            Pill("+") { custom = (custom + if (custom < 100) 10 else 50).coerceAtMost(3000) }
            Spacer(Modifier.width(8.dp))
            Pill("Add", filled = true, color = tint) { onAdd(custom) }
        }
    }
}

@Composable
private fun CupTile(kind: CupKind, tint: Color, usual: Boolean, modifier: Modifier, onClick: () -> Unit) {
    val t = LocalTokens.current
    val shape = RoundedCornerShape(12.dp)
    Column(
        modifier
            .clip(shape)
            .background(if (usual) tint.copy(alpha = 0.16f) else t.wash(0.05f))
            .border(if (usual) 1.5.dp else 0.dp, if (usual) tint.copy(alpha = 0.7f) else Color.Transparent, shape)
            .clickable(onClick = onClick)
            .padding(vertical = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        val outline = if (t.light) t.ink.copy(alpha = 0.6f) else Color.White.copy(alpha = 0.7f)
        Canvas(Modifier.size(40.dp, 56.dp)) {
            val k = size.width / 40f
            scale(k, Offset.Zero) {
                // the drink inside: the body cut off above the surface
                clipRect(top = kind.level) {
                    drawPath(kind.body, Brush.verticalGradient(listOf(lerp(tint, Color.White, 0.25f), tint), startY = kind.level, endY = 52f))
                }
                drawPath(kind.body, outline, style = Stroke(1.4f))
                kind.extra?.let { drawPath(it, outline, style = Stroke(1.4f)) }
            }
        }
        Spacer(Modifier.height(4.dp))
        Text(if (kind.ml >= 1000) "${kind.ml / 1000} L" else "${kind.ml} ml", color = t.text, fontSize = 12.sp, fontWeight = FontWeight.Bold)
        Text(kind.name, color = t.textFaint, fontSize = 10.sp, maxLines = 1)
    }
}

/** New drink or edit one: a name and one of 12 colours. The size is picked when you log it. */
@Composable
private fun DrinkEditor(d: DrinkType?, all: List<DrinkType>, onDone: () -> Unit) {
    val t = LocalTokens.current
    var name by remember(d?.id) { mutableStateOf(d?.name ?: "") }
    // a new drink starts on a colour nobody uses yet
    var color by remember(d?.id) {
        mutableLongStateOf(d?.color ?: DrinkPalette.firstOrNull { c -> all.none { it.color == c } } ?: DrinkPalette[0])
    }
    var confirmRemove by remember(d?.id) { mutableStateOf(false) }

    fun save() {
        if (name.isNotBlank()) {
            Store.saveDrink(d?.id, name, color)
            onDone()
        }
    }

    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(16.dp))
            .background(t.surface)
            .border(1.dp, t.stroke, RoundedCornerShape(16.dp))
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(12.dp).background(Color(color), CircleShape))
            Spacer(Modifier.width(8.dp))
            Text(if (d == null) "New drink" else "Edit drink", color = t.text, fontSize = 14.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
            Text("✕", color = t.textDim, fontSize = 15.sp, modifier = Modifier.clickable(onClick = onDone).padding(6.dp))
        }
        Box(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(12.dp))
                .background(t.wash(0.05f))
                .border(1.dp, t.stroke, RoundedCornerShape(12.dp))
                .padding(horizontal = 12.dp, vertical = 10.dp),
        ) {
            if (name.isEmpty()) Label("Name (e.g. Cortado, Mocha, Lemonade)", size = 14.sp, color = t.textFaint)
            BasicTextField(
                name, { name = it.take(24) },
                Modifier.fillMaxWidth(),
                singleLine = true,
                textStyle = TextStyle(color = t.text, fontSize = 14.sp),
                cursorBrush = SolidColor(t.accent),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { save() }),
            )
        }
        DrinkPalette.chunked(6).forEach { row ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                row.forEach { c ->
                    Box(
                        Modifier
                            .size(30.dp)
                            .clip(CircleShape)
                            .background(Color(c))
                            .border(if (c == color) 2.5.dp else 0.dp, if (c == color) t.text else Color.Transparent, CircleShape)
                            .clickable { color = c },
                    )
                }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            if (d != null && d.id != "water") {
                Pill(if (confirmRemove) "Tap again to remove" else "Remove", color = t.red, filled = confirmRemove) {
                    if (confirmRemove) {
                        Store.removeDrink(d.id)
                        onDone()
                    } else {
                        confirmRemove = true
                    }
                }
            }
            Spacer(Modifier.weight(1f))
            Pill("Save", filled = true, color = Color(color)) { save() }
        }
    }
}

/** The bottle fills in layers of today's drinks, with a moving wave in the last one's colour. */
@Composable
private fun Bottle(data: Data, goal: Int, modifier: Modifier) {
    val t = LocalTokens.current
    val sips = data.sips
    val phase by rememberInfiniteTransition(label = "wave").animateFloat(
        0f, 1f, infiniteRepeatable(tween(2_400, easing = LinearEasing), RepeatMode.Restart), label = "wave",
    )
    Canvas(modifier) {
        val neckW = size.width * 0.38f
        val neckH = size.height * 0.12f
        val shoulder = size.height * 0.08f
        val r = 14.dp.toPx()
        val body = Path().apply {
            moveTo((size.width - neckW) / 2, 0f)
            lineTo((size.width + neckW) / 2, 0f)
            lineTo((size.width + neckW) / 2, neckH)
            lineTo(size.width, neckH + shoulder)
            lineTo(size.width, size.height - r)
            quadraticTo(size.width, size.height, size.width - r, size.height)
            lineTo(r, size.height)
            quadraticTo(0f, size.height, 0f, size.height - r)
            lineTo(0f, neckH + shoulder)
            lineTo((size.width - neckW) / 2, neckH)
            close()
        }
        drawPath(body, t.wash(0.06f))
        clipPath(body) {
            val fillTop = neckH + shoulder
            val usable = size.height - fillTop
            var y = size.height
            sips.forEachIndexed { i, s ->
                val h = s.ml / goal.toFloat() * usable
                val top = (y - h).coerceAtLeast(fillTop)
                val c = Color(data.drink(s.type).color)
                if (i == sips.lastIndex) {
                    // the top layer gets a wave
                    val wave = Path().apply {
                        moveTo(0f, y)
                        lineTo(0f, top)
                        val steps = 24
                        for (k in 0..steps) {
                            val x = size.width * k / steps
                            lineTo(x, top + 3.dp.toPx() * sin((k / steps.toFloat() + phase) * 2 * PI).toFloat())
                        }
                        lineTo(size.width, y)
                        close()
                    }
                    drawPath(wave, c.copy(alpha = 0.9f))
                } else {
                    drawRect(c.copy(alpha = 0.9f), Offset(0f, top), Size(size.width, y - top))
                }
                y = top
            }
        }
        drawPath(body, t.wash(0.25f), style = Stroke(1.5.dp.toPx()))
    }
}
