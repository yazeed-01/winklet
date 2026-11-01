package app.winklet

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameMillis
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.sp
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.imageResource
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import android.media.AudioManager
import android.os.BatteryManager
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import java.time.LocalDate
import java.time.LocalTime
import kotlin.random.Random

/** The eye events, as on the desktop pill. Each is a short one-shot. */
enum class Mood(val label: String, val ms: Long) {
    None("", 0),
    LookLeft("Left", 2_000),
    LookRight("Right", 2_000),
    Wink("Wink", 1_000),
    Happy("Happy", 2_000),
    Surprised("Wow", 2_000),
    Car("Car", 3_000),
    Coffee("Coffee", 6_000),
    Love("Love", 2_000),
    Music("Music", 6_000),
    Tired("Tired", 4_000),
    Away("Zzz", 5_000),
}

/** Ask the eyes for an event from anywhere: water goal reached, a test from settings… */
object EyeEvents {
    private val _events = MutableSharedFlow<Mood>(extraBufferCapacity = 4)
    val events: SharedFlow<Mood> = _events
    fun play(m: Mood) {
        _events.tryEmit(m)
    }
}

class EyeState(mood: State<Mood>, blink: State<Boolean>, val startedAt: State<Long>) {
    val mood by mood
    val blink by blink
}

/**
 * Picks a random event every 7-17 s and blinks. Coffee comes first thing in the morning,
 * music playing on the phone gets the headphones, a late night or a low battery makes the
 * eyes tired, and after midnight they doze off now and then. It also blinks
 * every few seconds. Nothing runs while [awake] is false (screen off), so the island sits
 * still and costs nothing.
 */
@Composable
fun rememberEyeState(lively: Boolean, awake: Boolean): EyeState {
    val mood = remember { mutableStateOf(Mood.None) }
    val blink = remember { mutableStateOf(false) }
    val started = remember { mutableLongStateOf(0L) }
    val ctx = LocalContext.current

    suspend fun play(m: Mood) {
        mood.value = m
        started.longValue = System.currentTimeMillis()
        delay(m.ms)
        mood.value = Mood.None
    }

    LaunchedEffect(awake) {
        while (awake) {
            delay(Random.nextLong(2_500, 6_000))
            blink.value = true
            delay(120)
            blink.value = false
        }
    }
    LaunchedEffect(lively, awake) {
        mood.value = Mood.None
        while (lively && awake) {
            delay(Random.nextLong(7_000, 17_000))
            val today = LocalDate.now().toEpochDay()
            if (LocalTime.now().hour in 6 until 11 && Prefs.coffeeDay != today) {
                Prefs.coffeeDay = today
                play(Mood.Coffee)
                continue
            }
            // vibe to the music for a bit, like the desktop pill
            if (ctx.getSystemService(AudioManager::class.java).isMusicActive && Random.nextFloat() < 0.6f) {
                play(Mood.Music)
                continue
            }
            val hour = LocalTime.now().hour
            val battery = ctx.getSystemService(BatteryManager::class.java).getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
            if ((hour >= 23 || hour < 5 || battery in 1..15) && Random.nextFloat() < 0.35f) {
                play(if (hour in 0 until 5 && Random.nextBoolean()) Mood.Away else Mood.Tired)
                continue
            }
            val r = Random.nextFloat()
            play(
                when {
                    r < 0.18f -> Mood.LookLeft
                    r < 0.36f -> Mood.LookRight
                    r < 0.48f -> Mood.Wink
                    r < 0.62f -> Mood.Happy
                    r < 0.72f -> Mood.Surprised
                    r < 0.82f -> Mood.Car
                    r < 0.92f -> Mood.Coffee
                    else -> Mood.Love
                },
            )
        }
    }
    // a new event replaces the one playing instead of waiting behind it
    LaunchedEffect(Unit) {
        EyeEvents.events.collectLatest { play(it) }
    }
    return remember { EyeState(mood, blink, started) }
}

private val HeartPink = EyeColor("Love", Color(0xFFFFB3CF), Color(0xFFF472B6))

/**
 * Two glowing eyes, plus the event props (car, coffee cup, hearts). Sizes are for the 52 dp bubble; [scale] shrinks or grows them.
 */
@Composable
fun Eyes(
    color: EyeColor,
    state: EyeState,
    modifier: Modifier = Modifier,
    scale: Float = 1f,
) {
    val mood = state.mood
    val anim = tween<Dp>(120)
    val w by animateDpAsState(if (mood == Mood.Surprised) 9.dp else 7.dp, anim, label = "w")
    val h by animateDpAsState(
        when {
            state.blink -> 2.dp
            mood == Mood.Surprised -> 15.dp
            mood == Mood.Away -> 4.dp
            mood == Mood.Tired -> 6.dp
            else -> 13.dp
        },
        anim, label = "h",
    )
    // tired and dozing eyes sit a little lower
    val droop by animateDpAsState(if (mood == Mood.Tired || mood == Mood.Away) 3.dp else 0.dp, tween(400), label = "droop")
    val text = rememberTextMeasurer()
    val car = ImageBitmap.imageResource(R.drawable.car_jdm)
    val baseLook by animateDpAsState(
        when (mood) {
            Mood.LookLeft -> (-3).dp
            Mood.LookRight -> 3.dp
            Mood.Coffee -> (-2).dp
            else -> 0.dp
        },
        anim, label = "look",
    )

    // per-event motion: only composed while that event plays
    val t = if (mood in Animated) frameClock(state.startedAt.value) else null

    Canvas(modifier) {
        val s = scale
        val ms = t?.value ?: 0L
        // music: the eyes bob to the beat
        val beat = mood == Mood.Music && (ms / 400) % 2 == 0L
        val cy = size.height / 2 + droop.toPx() * s - (if (beat) 2.dp.toPx() * s else 0f)
        val gap = 7.dp.toPx() * s

        // the tiny car: accelerate in, cruise past the eyes, speed off (eyes follow it)
        var carX = Float.NaN
        if (mood == Mood.Car) {
            val u = (ms / 2_600f).coerceIn(0f, 1f)
            val e = if (u < 0.5f) 4 * u * u * u else 1 - Math.pow((-2.0 * u + 2), 3.0).toFloat() / 2
            carX = -18.dp.toPx() * s + (size.width + 36.dp.toPx() * s) * e
        }
        val look = if (!carX.isNaN()) ((carX - size.width / 2) / 6).coerceIn(-3.dp.toPx(), 3.dp.toPx()) else baseLook.toPx() * s
        val cx = size.width / 2 + look

        // coffee: the cup comes up to sip every 1.6 s, eyes close happily while sipping
        val sipping = mood == Mood.Coffee && (ms % 1_600) in 500..1_200

        for (i in 0..1) {
            val ex = if (i == 0) cx - gap else cx + gap
            when {
                mood == Mood.Love -> {
                    val beat = 1f + 0.12f * kotlin.math.sin(ms / 1000f * 2 * Math.PI.toFloat() * 1.5f).coerceAtLeast(0f)
                    heart(ex, cy, 11.dp.toPx() * s * beat, HeartPink)
                }
                mood == Mood.Happy || sipping -> happyArc(ex, cy, 7.dp.toPx() * s, color)
                else -> {
                    val closed = mood == Mood.Wink && i == 1
                    glowPill(ex, cy, w.toPx() * s, if (closed) 2.dp.toPx() * s else h.toPx() * s, color)
                }
            }
        }

        if (!carX.isNaN()) {
            // the desktop's orange car, passing under the eyes
            val cw = 32.dp.toPx() * s
            val ch = cw * 100 / 220
            drawImage(
                car,
                dstOffset = IntOffset((carX - cw / 2).toInt(), (size.height / 2 + 13.dp.toPx() * s - ch / 2).toInt()),
                dstSize = IntSize(cw.toInt(), ch.toInt()),
            )
        }
        if (mood == Mood.Surprised) {
            // "!" by the right eye
            val mark = text.measure("!", TextStyle(color = Color(0xFFFB923C), fontSize = (12 * s).sp, fontWeight = FontWeight.ExtraBold))
            drawText(mark, topLeft = Offset(size.width / 2 + 13.dp.toPx() * s, cy - 14.dp.toPx() * s))
        }
        if (mood == Mood.Music) headphones(size.width / 2, cy, s, ms, text)
        if (mood == Mood.Away) zzz(size.width / 2 + 11.dp.toPx() * s, cy, s, ms, text)
        if (mood == Mood.Coffee) cup(size.width / 2 + 13.dp.toPx() * s, cy + (if (sipping) 6.dp else 11.dp).toPx() * s, s, ms)
    }
}

/** Events that move on every frame while they play. */
private val Animated = setOf(Mood.Car, Mood.Coffee, Mood.Love, Mood.Music, Mood.Away)

/** Headphones over the eyes and a note bobbing beside them, as on the desktop. */
private fun DrawScope.headphones(x: Float, cy: Float, s: Float, ms: Long, text: TextMeasurer) {
    val d = density * s
    val beat = (ms / 400) % 2 == 0L
    val band = Path().apply {
        moveTo(x - 17 * d, cy + 4 * d)
        cubicTo(x - 17 * d, cy - 16 * d, x + 17 * d, cy - 16 * d, x + 17 * d, cy + 4 * d)
    }
    drawPath(band, Color(0xFFD4D4FF), style = Stroke(2.2f * d, cap = StrokeCap.Round))
    val cup = Brush.linearGradient(listOf(Color(0xFFA78BFA), Color(0xFF6366F1), Color(0xFF3B82F6)))
    for (side in listOf(-1, 1)) {
        drawRoundRect(cup, Offset(x + side * 17 * d - 4 * d, cy - 2 * d), Size(8 * d, 12 * d), CornerRadius(3 * d))
    }
    val note = text.measure("♪", TextStyle(color = Color(0xFF8B7BFF), fontSize = (11 * s).sp))
    drawText(note, topLeft = Offset(x + 12 * d, cy - (if (beat) 22 else 16) * d), alpha = if (beat) 0.35f else 1f)
}

/** Three z's drifting up, small to big. */
private fun DrawScope.zzz(x: Float, cy: Float, s: Float, ms: Long, text: TextMeasurer) {
    val d = density * s
    for (i in 0..2) {
        val phase = ((ms + i * 350) % 1_400) / 1_400f
        val z = text.measure("z", TextStyle(color = Color(0xFFC7D2FE), fontSize = ((6 + i * 1.5f) * s).sp, fontWeight = FontWeight.Bold))
        drawText(z, topLeft = Offset(x + i * 4 * d, cy - (4 + i * 4) * d - phase * 3 * d), alpha = 0.3f + i * 0.25f)
    }
}

/** Milliseconds since [start], ticking every frame while composed. */
@Composable
private fun frameClock(start: Long): State<Long> {
    val t = remember { mutableLongStateOf(0L) }
    LaunchedEffect(start) {
        while (true) withFrameMillis { t.longValue = System.currentTimeMillis() - start }
    }
    return t
}

/** -2..2 dp gentle float for the bubble, read only in the draw phase. */
@Composable
fun bobOffset(): State<Float> =
    rememberInfiniteTransition(label = "bob").animateFloat(
        -2f, 2f,
        infiniteRepeatable(tween(1_600, easing = LinearEasing), RepeatMode.Reverse),
        label = "bob",
    )

private fun DrawScope.glowPill(cx: Float, cy: Float, w: Float, h: Float, color: EyeColor) {
    // a soft glow: two wider, faint layers under the eye
    for ((grow, alpha) in listOf(5.dp.toPx() to 0.16f, 2.5.dp.toPx() to 0.32f)) {
        val gw = w + grow * 2
        val gh = h + grow * 2
        drawRoundRect(color.glow.copy(alpha = alpha), Offset(cx - gw / 2, cy - gh / 2), Size(gw, gh), CornerRadius(minOf(gw, gh) / 2))
    }
    drawRoundRect(color.fill, Offset(cx - w / 2, cy - h / 2), Size(w, h), CornerRadius(minOf(w, h) / 2))
}

/** ^ shaped eye for happy and sipping. */
private fun DrawScope.happyArc(cx: Float, cy: Float, w: Float, color: EyeColor) {
    val stroke = 2.5.dp.toPx()
    val top = Offset(cx - w / 2, cy - w / 4)
    drawArc(color.glow.copy(alpha = 0.35f), 200f, 140f, false, top - Offset(1.5f, 1.5f), Size(w + 3f, w + 3f),
        style = Stroke(stroke + 3.dp.toPx(), cap = StrokeCap.Round))
    drawArc(color.fill, 200f, 140f, false, top, Size(w, w), style = Stroke(stroke, cap = StrokeCap.Round))
}

private fun DrawScope.heart(cx: Float, cy: Float, size: Float, color: EyeColor) {
    val p = Path().apply {
        val s = size / 2
        moveTo(cx, cy + s * 0.85f)
        cubicTo(cx - s * 1.3f, cy - s * 0.1f, cx - s * 0.6f, cy - s * 1.1f, cx, cy - s * 0.4f)
        cubicTo(cx + s * 0.6f, cy - s * 1.1f, cx + s * 1.3f, cy - s * 0.1f, cx, cy + s * 0.85f)
        close()
    }
    scale(1.35f, Offset(cx, cy)) { drawPath(p, color.glow.copy(alpha = 0.3f)) }
    drawPath(p, color.fill)
}

private fun DrawScope.cup(x: Float, y: Float, s: Float, ms: Long) {
    val d = density * s
    val cupColor = Color(0xFFF5F0E6)
    drawRoundRect(cupColor, Offset(x - 3.5f * d, y - 3 * d), Size(7 * d, 6 * d), CornerRadius(1.2f * d))
    drawArc(cupColor, -90f, 180f, false, Offset(x + 2 * d, y - 1.8f * d), Size(3.4f * d, 3.4f * d), style = Stroke(1.1f * d))
    drawRect(Color(0xFF8B5A3C), Offset(x - 2.6f * d, y - 2.4f * d), Size(5.2f * d, 1.1f * d))
    // two wisps of steam drifting up
    for (k in 0..1) {
        val phase = ((ms + k * 450) % 900) / 900f
        drawCircle(Color.White.copy(alpha = 0.45f * (1 - phase)), 0.8f * d,
            Offset(x - 1.2f * d + k * 2.4f * d, y - 4 * d - phase * 4 * d))
    }
}
