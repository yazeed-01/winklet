package app.winklet

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.unit.IntOffset
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.updateTransition
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.lerp
import androidx.compose.ui.unit.min
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

const val BALL_DP = 52
const val PANEL_W_DP = 340
const val PANEL_H_DP = 500

data class Battery(val level: Int = 0, val charging: Boolean = false)

enum class Tab(val label: String) { Home("Home"), Todo("To-do"), Water("Water"), Health("Health"), Device("Device") }

/** What the overlay shows. The service owns it and the UI reads it. */
class IslandModel {
    var expanded by mutableStateOf(false)
    var tab by mutableStateOf(Tab.Home)
    var awake by mutableStateOf(true)
    var battery by mutableStateOf(Battery())
    var stats by mutableStateOf<SysStats?>(null)
    val cpuHistory = mutableStateListOf<Float>()
    var peek by mutableStateOf<Peek?>(null)
    var stepsReady by mutableStateOf(true)
    /** Which edge the island rests on; the panel grows out of that corner. */
    var onRight by mutableStateOf(true)
    /** How far below the open window's top the bubble sits, in px (the panel may shift up to fit). */
    var anchorY by mutableIntStateOf(0)
}

/** The whole overlay: the resting bubble (or a peek), or the panel. */
@Composable
fun Island(m: IslandModel, onOpen: (Boolean) -> Unit, onTab: (Tab) -> Unit, onPeekTap: () -> Unit) {
    WinkletTheme(panel = m.expanded) {
        val t = LocalTokens.current
        val eyeIndex by Prefs.eyeColor.state.collectAsState()
        val lively by Prefs.lively.state.collectAsState()
        // the eyes always use the dark theme colours: they glow on the dark ball
        val color = EyeTint.get(eyeIndex, t.palette)
        val eyes = rememberEyeState(lively, m.awake && !m.expanded && !Prefs.bubbleHidden.value)
        val peek = m.peek
        val tucked by Prefs.bubbleHidden.state.collectAsState()
        val deckOpen by DeckActivity.showing.collectAsState()
        val hidden = tucked || deckOpen

        // 0 = the bubble, 1 = the full panel: it grows out of the bubble and shrinks back into it
        val open = updateTransition(m.expanded, label = "open")
        val p by open.animateFloat(
            transitionSpec = { if (targetState) spring(dampingRatio = 0.8f, stiffness = 420f) else tween(240, easing = FastOutSlowInEasing) },
            label = "p",
        ) { if (it) 1f else 0f }

        when {
            // tucked away into the notification: draw nothing, animate nothing
            hidden -> {}
            // the panel keeps its light/dark look while it shrinks back
            open.currentState || open.targetState -> WinkletTheme(panel = true) {
                Morph(m, p, color, eyes, onTab, onClose = { onOpen(false) })
            }
            peek != null -> PeekCard(peek, color, eyes, onPeekTap)
            else -> Ball(color, eyes, m.awake, onTap = { onOpen(true) }, onLongPress = { Prefs.bubbleHidden.set(true) })
        }
    }
}

/**
 * Bubble style: a floating ball with just the eyes. It bobs gently while the screen is on.
 * A long press tucks it into the notification.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun Ball(color: EyeColor, eyes: EyeState, awake: Boolean, onTap: () -> Unit, onLongPress: () -> Unit) {
    val t = LocalTokens.current
    val bob = if (awake) bobOffset() else null
    Box(
        Modifier
            // room for the bob so the window never clips the ball
            .padding(3.dp)
            .graphicsLayer { translationY = (bob?.value ?: 0f) * density }
            .size(BALL_DP.dp)
            .clip(CircleShape)
            .background(t.island)
            .border(1.dp, t.stroke, CircleShape)
            .combinedClickable(remember { MutableInteractionSource() }, null, onClick = onTap, onLongClick = onLongPress),
    ) {
        Eyes(color, eyes, Modifier.size(BALL_DP.dp))
    }
}

/** A short message dropping out of the bubble: reminders and water nudges. */
@Composable
private fun PeekCard(p: Peek, color: EyeColor, eyes: EyeState, onTap: () -> Unit) {
    val t = LocalTokens.current
    Row(
        Modifier
            .padding(vertical = 3.dp)
            .widthIn(max = 300.dp)
            .clip(RoundedCornerShape(28.dp))
            .background(t.island)
            .border(1.dp, t.stroke, RoundedCornerShape(28.dp))
            .clickable(remember { MutableInteractionSource() }, null, onClick = onTap)
            .padding(end = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Eyes(color, eyes, Modifier.size(BALL_DP.dp))
        Column(Modifier.padding(vertical = 8.dp).weight(1f, fill = false)) {
            Text(p.title, color = t.text, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (p.body.isNotEmpty()) Text(p.body, color = t.textDim, fontSize = 12.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        if (p.kind == Peek.Kind.Water) {
            Spacer(Modifier.width(10.dp))
            Pill("+cup", filled = true, color = t.blue) {
                Store.addSip("water", 250)
                Peeks.clear()
            }
        }
    }
}

/**
 * The open animation: a rounded box grows from the bubble's spot to the full panel (and back),
 * the eyes fade out as the panel's content fades in. The window is panel-sized the whole time,
 * so only drawing changes from frame to frame.
 */
@Composable
private fun Morph(m: IslandModel, p: Float, color: EyeColor, eyes: EyeState, onTab: (Tab) -> Unit, onClose: () -> Unit) {
    val t = LocalTokens.current
    val corner = if (m.onRight) Alignment.TopEnd else Alignment.TopStart
    val anchor = with(LocalDensity.current) { m.anchorY.toDp() }
    // a touch of overshoot from the spring, but never past the window
    val k = p.coerceIn(0f, 1.02f)
    // phones may give an overlay window less than the full screen width: fit what we get
    BoxWithConstraints(Modifier.padding(3.dp)) {
    val pw = min(PANEL_W_DP.dp, maxWidth - 4.dp)
    val ph = min(PANEL_H_DP.dp, maxHeight - 4.dp)
    val w = lerp(BALL_DP.dp, pw, k)
    val h = lerp(BALL_DP.dp, ph, k)
    val shape = RoundedCornerShape(lerp((BALL_DP / 2).dp, 28.dp, k.coerceAtMost(1f)))
    Box(Modifier.size(pw + 4.dp, ph + 4.dp)) {
        Box(
            Modifier
                .align(corner)
                .offset(y = lerp(anchor, 0.dp, k.coerceAtMost(1f)))
                .size(w, h)
                .clip(shape)
                .background(t.island)
                .border(1.dp, t.stroke, shape),
        ) {
            Panel(
                m, color, eyes, onTab, onClose,
                Modifier
                    .wrapContentSize(corner, unbounded = true)
                    .size(pw, ph)
                    .graphicsLayer { alpha = ((p - 0.35f) / 0.65f).coerceIn(0f, 1f) },
            )
            // the bubble's eyes, fading as it opens
            if (p < 0.4f) {
                Eyes(
                    color, eyes,
                    Modifier.align(corner).size(BALL_DP.dp).graphicsLayer { alpha = 1f - p / 0.4f },
                )
            }
        }
    }
    }
}

@Composable
private fun Panel(m: IslandModel, color: EyeColor, eyes: EyeState, onTab: (Tab) -> Unit, onClose: () -> Unit, modifier: Modifier) {
    val t = LocalTokens.current
    val ctx = androidx.compose.ui.platform.LocalContext.current
    Column(modifier.padding(horizontal = 14.dp, vertical = 12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            // the eyes close the panel, like tapping the bubble again
            Box(
                Modifier
                    .size(40.dp)
                    .clip(CircleShape)
                    .background(Palettes[Prefs.theme.value.coerceIn(0, Palettes.lastIndex)].island)
                    .clickable(onClick = onClose),
            ) {
                Eyes(color, eyes, Modifier.size(40.dp), scale = 0.72f)
            }
            Spacer(Modifier.width(8.dp))
            TabBar(m.tab, onTab, Modifier.weight(1f))
        }
        Spacer(Modifier.height(12.dp))
        // pages slide in from the side you're moving to, with a small spring and zoom
        AnimatedContent(
            m.tab,
            Modifier.fillMaxWidth().weight(1f),
            transitionSpec = {
                val dir = if (targetState.ordinal > initialState.ordinal) 1 else -1
                val spring = spring<IntOffset>(dampingRatio = 0.82f, stiffness = 520f)
                (slideInHorizontally(spring) { it / 3 * dir } + fadeIn(tween(220)) + scaleIn(tween(260), initialScale = 0.96f))
                    .togetherWith(slideOutHorizontally(spring) { -it / 4 * dir } + fadeOut(tween(160)))
                    .using(SizeTransform(clip = true))
            },
            label = "page",
        ) { tab ->
            Box(Modifier.fillMaxSize()) {
                when (tab) {
                    Tab.Home -> HomePage(m, onTab, onDeck = {
                    onClose()
                    // not paired yet: the app's settings, where "Connect PC deck" scans the code
                    if (PcDeck.link.value != null) {
                        DeckActivity.open(ctx)
                    } else {
                        ctx.startActivity(android.content.Intent(ctx, MainActivity::class.java).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
                    }
                })
                    Tab.Todo -> TodoPage()
                    Tab.Water -> WaterPage()
                    Tab.Health -> HealthPage(m.stepsReady)
                    Tab.Device -> DevicePage(m)
                }
            }
        }
    }
}

/** The tabs, with a highlight pill that slides (with a little spring) to the one you pick. */
@Composable
private fun TabBar(current: Tab, onTab: (Tab) -> Unit, modifier: Modifier) {
    val t = LocalTokens.current
    BoxWithConstraints(
        modifier
            .clip(RoundedCornerShape(50))
            .background(t.surface)
            .padding(3.dp),
    ) {
        val w = maxWidth / Tab.entries.size
        val x by animateDpAsState(w * current.ordinal, spring(dampingRatio = 0.75f, stiffness = 500f), label = "pill")
        Box(
            Modifier
                .offset(x = x)
                .width(w)
                .height(30.dp)
                .clip(RoundedCornerShape(50))
                .background(t.accent),
        )
        Row(Modifier.height(30.dp)) {
            Tab.entries.forEach { tab ->
                val on = tab == current
                val ink by animateColorAsState(if (on) onColor(t.accent) else t.textDim, tween(200), label = "ink")
                Box(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .clip(RoundedCornerShape(50))
                        .clickable { onTab(tab) },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(tab.label, fontSize = 11.sp, maxLines = 1, color = ink, fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal)
                }
            }
        }
    }
}
