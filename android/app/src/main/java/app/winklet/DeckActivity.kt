package app.winklet

import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** The same icon font as the PC, so glyph keys look the same. */
private val IconFont = FontFamily(Font(R.font.winklet_icons))

/**
 * The PC's Deck, full screen: the same keys, colours, icons and pages, pressed over the
 * Wi-Fi. The screen stays on while it's open. Landscape shows the PC's 8 × 3 grid; portrait
 * shows the same keys 4 across.
 */
class DeckActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        PcDeck.init(this)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        // full screen: a swipe from the edge brings the bars back for a moment
        WindowCompat.setDecorFitsSystemWindows(window, false)
        WindowCompat.getInsetsController(window, window.decorView).apply {
            hide(WindowInsetsCompat.Type.systemBars())
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
        setContent {
            WinkletTheme(panel = false) { DeckScreen(onClose = ::finish) }
        }
    }

    override fun onStart() {
        super.onStart()
        _showing.value = true
    }

    override fun onStop() {
        _showing.value = false
        super.onStop()
    }

    companion object {
        private val _showing = kotlinx.coroutines.flow.MutableStateFlow(false)
        /** The deck is on screen: the bubble steps aside so it doesn't cover keys. */
        val showing: kotlinx.coroutines.flow.StateFlow<Boolean> = _showing

        fun open(ctx: Context) {
            ctx.startActivity(Intent(ctx, DeckActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }
}

private enum class Link { Connecting, Online, Offline }

@Composable
private fun DeckScreen(onClose: () -> Unit) {
    val t = LocalTokens.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var deck by remember { mutableStateOf<RemoteDeck?>(null) }
    var link by remember { mutableStateOf(Link.Connecting) }
    val icons = remember { mutableStateMapOf<String, ImageBitmap>() }

    // check the PC every 2.5 s while the deck is on screen: keys edited on the PC show up here
    LaunchedEffect(Unit) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                val got = runCatching { PcDeck.load() }.getOrNull()
                if (got == null) {
                    link = Link.Offline
                } else {
                    link = Link.Online
                    if (got.version != deck?.version) {
                        deck = got
                        icons.clear()
                        got.keys.filter { it.hasIcon }.forEach { k -> PcDeck.icon(k.id)?.let { icons[k.id] = it } }
                    }
                }
                delay(if (link == Link.Online) 2_500 else 1_500)
            }
        }
    }

    val landscape = LocalConfiguration.current.orientation == Configuration.ORIENTATION_LANDSCAPE
    val d = deck
    val pager = rememberPagerState { d?.pages ?: 1 }
    val scope = rememberCoroutineScope()

    Column(
        Modifier
            .fillMaxSize()
            .background(Color(0xFF07070A))
            .safeDrawingPadding()
            .padding(horizontal = 14.dp, vertical = 10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("‹", color = t.textDim, fontSize = 30.sp, modifier = Modifier.clickable(onClick = onClose).padding(end = 12.dp))
            Text("PC Deck", color = t.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.width(10.dp))
            val (dot, label) = when (link) {
                Link.Connecting -> t.orange to "Connecting…"
                Link.Online -> t.green to "Connected"
                Link.Offline -> t.red to "Can't reach the PC: same Wi-Fi? Winklet running?"
            }
            Box(Modifier.size(8.dp).background(dot, CircleShape))
            Spacer(Modifier.width(6.dp))
            Text(label, color = t.textDim, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            // pages
            if ((d?.pages ?: 1) > 1) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    repeat(d!!.pages) { p ->
                        val on = pager.currentPage == p
                        Box(
                            Modifier
                                .size(30.dp)
                                .clip(CircleShape)
                                .background(if (on) t.accent.copy(alpha = 0.3f) else t.surface)
                                .border(if (on) 1.dp else 0.dp, if (on) t.accent else Color.Transparent, CircleShape)
                                .clickable { scope.launch { pager.animateScrollToPage(p) } },
                            contentAlignment = Alignment.Center,
                        ) {
                            Text("${p + 1}", color = if (on) t.text else t.textDim, fontSize = 12.sp, fontWeight = FontWeight.Bold)
                        }
                    }
                }
            }
        }
        Spacer(Modifier.height(12.dp))

        if (d == null) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(
                    if (link == Link.Offline) "Can't reach Winklet on the PC.\nCheck both are on the same Wi-Fi and Deck › Phone is on."
                    else "Loading the Deck…",
                    color = t.textDim, fontSize = 15.sp, textAlign = TextAlign.Center,
                )
            }
            return@Column
        }

        val perPage = d.cols * d.rows
        val cols = if (landscape) d.cols else (d.cols / 2).coerceAtLeast(1)
        val rows = (perPage + cols - 1) / cols
        HorizontalPager(pager, Modifier.fillMaxSize()) { page ->
            val keys = d.keys.filter { it.page == page }.associateBy { it.slot }
            BoxWithConstraints(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                val gap = 10.dp
                val key = minOf((maxWidth - gap * (cols - 1)) / cols, (maxHeight - gap * (rows - 1)) / rows)
                Column(verticalArrangement = Arrangement.spacedBy(gap)) {
                    repeat(rows) { r ->
                        Row(horizontalArrangement = Arrangement.spacedBy(gap)) {
                            repeat(cols) { c ->
                                val slot = r * cols + c
                                Box(Modifier.size(key)) {
                                    if (slot < perPage) {
                                        val k = keys[slot]
                                        if (k == null) EmptyKey() else KeyFace(k, icons[k.id]) { link = if (it) Link.Online else Link.Offline }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun EmptyKey() {
    Box(
        Modifier
            .fillMaxSize()
            .clip(RoundedCornerShape(22))
            .background(Color.White.copy(alpha = 0.03f))
            .border(1.dp, Color.White.copy(alpha = 0.05f), RoundedCornerShape(22)),
    )
}

/** A key like the PC's: its colour with a sheen, the icon or glyph, and the title. */
@Composable
private fun KeyFace(k: RemoteKey, icon: ImageBitmap?, onResult: (Boolean) -> Unit) {
    val haptic = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()
    val press = remember { Animatable(1f) }
    val flash = remember { Animatable(0f) }
    val tint = Color(k.color)
    val shape = RoundedCornerShape(24)
    BoxWithConstraints(
        Modifier
            .fillMaxSize()
            .graphicsLayer { scaleX = press.value; scaleY = press.value }
            .clip(shape)
            .background(Brush.linearGradient(listOf(lerp(tint, Color.White, 0.25f), tint, lerp(tint, Color.Black, 0.3f))))
            .border(1.dp, Color.White.copy(alpha = 0.13f), shape)
            .pointerInput(k.id) {
                detectTapGestures(
                    onPress = {
                        scope.launch { press.animateTo(0.9f, tween(70)) }
                        tryAwaitRelease()
                        scope.launch { press.animateTo(1f, spring(dampingRatio = 0.45f, stiffness = 600f)) }
                    },
                    onTap = {
                        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                        scope.launch {
                            flash.snapTo(0.45f)
                            flash.animateTo(0f, tween(420))
                        }
                        scope.launch { onResult(PcDeck.press(k.id)) }
                    },
                )
            },
    ) {
        val s = minOf(maxWidth, maxHeight)
        // glassy top highlight
        Box(
            Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.48f)
                .background(Brush.verticalGradient(listOf(Color.White.copy(alpha = 0.15f), Color.Transparent))),
        )
        Column(
            Modifier.fillMaxSize().padding(horizontal = 4.dp, vertical = s * 0.1f),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Box(Modifier.weight(1f).aspectRatio(1f, matchHeightConstraintsFirst = true), contentAlignment = Alignment.Center) {
                if (icon != null) {
                    Image(icon, k.title, Modifier.size(s * 0.42f))
                } else {
                    Text(k.glyph, color = Color.White, fontFamily = IconFont, fontSize = (s.value * 0.3f).sp)
                }
            }
            Text(
                k.title, color = Color.White, fontSize = (s.value * 0.12f).coerceIn(9f, 14f).sp,
                fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center,
            )
        }
        // flashes when it fires
        Box(Modifier.fillMaxSize().graphicsLayer { alpha = flash.value }.background(Color.White))
    }
}
