package app.winklet

import android.Manifest
import android.app.AlarmManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.runtime.LaunchedEffect
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import android.widget.Toast
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning

/** Setup and settings: permissions, show or hide the island, look, goals and reminders. */
class MainActivity : ComponentActivity() {
    private var canOverlay by mutableStateOf(false)
    private var canSteps by mutableStateOf(false)
    private var canExact by mutableStateOf(true)
    private var canNotifs by mutableStateOf(false)

    private val askPermissions =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
            refresh()
            startIsland()
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Prefs.init(this)
        Store.init(this)
        Reminders.channel(this)
        setContent {
            // settings follow Light too, like the open panel
            WinkletTheme(panel = true) {
                val light by Prefs.lightMode.state.collectAsState()
                LaunchedEffect(light) {
                    WindowCompat.getInsetsController(window, window.decorView).apply {
                        isAppearanceLightStatusBars = light
                        isAppearanceLightNavigationBars = light
                    }
                }
                SettingsScreen(
                    canOverlay, canSteps, canExact, canNotifs,
                    onGrantOverlay = ::openOverlaySettings,
                    onStart = ::requestStart,
                    onStop = {
                        Prefs.islandOn = false
                        stopService(Intent(this, IslandService::class.java))
                    },
                    onGrantSteps = { askPermissions.launch(runtimePermissions()) },
                    onGrantExact = {
                        if (Build.VERSION.SDK_INT >= 31) {
                            startActivity(Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:$packageName")))
                        }
                    },
                    onWaterEvery = {
                        Prefs.waterEvery.set(it)
                        Reminders.scheduleWater(this)
                    },
                    onScanDeck = ::scanDeck,
                    onSendNotifs = { on ->
                        Prefs.sendNotifs.set(on)
                        // the switch stays off until access is given (it's turned on when they come back)
                        if (on && !canNotifs) startActivity(Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS))
                    },
                    onOpenDeck = { DeckActivity.open(this) },
                    onDay = { start, end ->
                        // keep at least 4 waking hours
                        if (start in 0..20 && end in start + 4..24) {
                            Prefs.dayStart.set(start)
                            Prefs.dayEnd.set(end)
                            Reminders.scheduleWater(this)
                        }
                    },
                )
            }
        }
    }

    override fun onResume() {
        super.onResume()
        refresh()
        // back from the overlay settings page with the permission: start right away
        if (canOverlay && !IslandService.running.value && intent.getBooleanExtra(EXTRA_AUTOSTART, false)) {
            intent.removeExtra(EXTRA_AUTOSTART)
            requestStart()
        } else if (canOverlay && !IslandService.running.value && Prefs.islandOn) {
            // it was on but got stopped (update, or the phone killed it): bring it back
            startIsland()
        }
    }

    private fun refresh() {
        canOverlay = Settings.canDrawOverlays(this)
        canSteps = Build.VERSION.SDK_INT < 29 ||
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACTIVITY_RECOGNITION) == PackageManager.PERMISSION_GRANTED
        canExact = Build.VERSION.SDK_INT < 31 || getSystemService(AlarmManager::class.java).canScheduleExactAlarms()
        canNotifs = PhoneLink.allowed(this)
        if (canNotifs && Prefs.sendNotifs.value) PhoneLink.rebind(this)
    }

    private fun openOverlaySettings() {
        intent.putExtra(EXTRA_AUTOSTART, true)
        startActivity(Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:$packageName")))
    }

    private fun runtimePermissions(): Array<String> = buildList {
        if (Build.VERSION.SDK_INT >= 33) add(Manifest.permission.POST_NOTIFICATIONS)
        if (Build.VERSION.SDK_INT >= 29) add(Manifest.permission.ACTIVITY_RECOGNITION)
    }.toTypedArray()

    private fun requestStart() {
        if (!Settings.canDrawOverlays(this)) return openOverlaySettings()
        val missing = runtimePermissions().filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }
        // notifications and steps are optional: the island starts either way
        if (missing.isNotEmpty()) askPermissions.launch(missing.toTypedArray()) else startIsland()
    }

    /** Scans the QR code in Deck > Phone on the PC, then opens the deck. */
    private fun scanDeck() {
        val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
        GmsBarcodeScanning.getClient(this, options).startScan()
            .addOnSuccessListener { code ->
                if (PcDeck.pair(code.rawValue ?: "")) {
                    DeckActivity.open(this)
                } else {
                    Toast.makeText(this, "That isn't a Winklet code from your home network. Update Winklet on the PC, then scan Deck › Phone.", Toast.LENGTH_LONG).show()
                }
            }
            .addOnFailureListener {
                Toast.makeText(this, "Couldn't open the scanner: ${it.message}", Toast.LENGTH_LONG).show()
            }
    }

    private fun startIsland() {
        if (!Settings.canDrawOverlays(this)) return
        Prefs.islandOn = true
        ContextCompat.startForegroundService(this, Intent(this, IslandService::class.java))
    }

    private companion object {
        const val EXTRA_AUTOSTART = "autostart"
    }
}

@Composable
private fun SettingsScreen(
    canOverlay: Boolean,
    canSteps: Boolean,
    canExact: Boolean,
    canNotifs: Boolean,
    onGrantOverlay: () -> Unit,
    onStart: () -> Unit,
    onStop: () -> Unit,
    onGrantSteps: () -> Unit,
    onGrantExact: () -> Unit,
    onWaterEvery: (Int) -> Unit,
    onDay: (Int, Int) -> Unit,
    onScanDeck: () -> Unit,
    onOpenDeck: () -> Unit,
    onSendNotifs: (Boolean) -> Unit,
) {
    val t = LocalTokens.current
    val running by IslandService.running.collectAsState()
    val eye by Prefs.eyeColor.state.collectAsState()
    val lively by Prefs.lively.state.collectAsState()
    val theme by Prefs.theme.state.collectAsState()
    val light by Prefs.lightMode.state.collectAsState()
    val waterGoal by Prefs.waterGoal.state.collectAsState()
    val waterEvery by Prefs.waterEvery.state.collectAsState()
    val dayStart by Prefs.dayStart.state.collectAsState()
    val dayEnd by Prefs.dayEnd.state.collectAsState()
    val hidden by Prefs.bubbleHidden.state.collectAsState()
    val deckLink by PcDeck.link.collectAsState()
    val stepGoal by Prefs.stepGoal.state.collectAsState()
    val eyeRest by Prefs.eyeRest.state.collectAsState()
    val sendNotifs by Prefs.sendNotifs.state.collectAsState()

    Column(
        Modifier
            .fillMaxSize()
            .background(t.island)
            .safeDrawingPadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(BALL_DP.dp).clip(CircleShape).background(Color.Black).border(1.dp, t.stroke, CircleShape)) {
                Eyes(EyeTint.get(eye, t.palette), rememberEyeState(lively, awake = true), Modifier.size(BALL_DP.dp))
            }
            Spacer(Modifier.width(14.dp))
            Column {
                Text("Winklet", color = t.text, fontSize = 28.sp, fontWeight = FontWeight.SemiBold)
                Label("A tiny winking island over your apps", size = 13.sp)
            }
        }

        when {
            !canOverlay -> {
                Label("Winklet needs \"Display over other apps\" to float on screen.", size = 14.sp, color = t.text)
                Pill("Allow display over other apps", filled = true, modifier = Modifier.fillMaxWidth(), onClick = onGrantOverlay)
            }
            running && hidden -> {
                Label("The bubble is hidden. Tap the Winklet notification, or here, to bring it back.", size = 13.sp)
                Pill("Show the bubble", filled = true, modifier = Modifier.fillMaxWidth()) { Prefs.bubbleHidden.set(false) }
                Pill("Turn Winklet off", modifier = Modifier.fillMaxWidth(), onClick = onStop)
            }
            running -> {
                Pill("Hide the bubble", filled = true, modifier = Modifier.fillMaxWidth()) { Prefs.bubbleHidden.set(true) }
                Pill("Turn Winklet off", modifier = Modifier.fillMaxWidth(), onClick = onStop)
            }
            else -> Pill("Show the island", filled = true, modifier = Modifier.fillMaxWidth(), onClick = onStart)
        }
        if (!canSteps) Pill("Allow step counting (Health tab)", modifier = Modifier.fillMaxWidth(), onClick = onGrantSteps)
        if (!canExact) Pill("Allow on-time task reminders", modifier = Modifier.fillMaxWidth(), onClick = onGrantExact)

        Section("PC deck") {
            if (deckLink == null) {
                Label("Use your phone as the Deck of Winklet on your PC. On the PC open Deck › Phone, then scan the code.", size = 13.sp)
                Pill("Connect PC deck", filled = true, modifier = Modifier.fillMaxWidth(), onClick = onScanDeck)
            } else {
                Label("Paired with ${deckLink!!.removePrefix("http://").substringBefore('/')}", size = 13.sp)
                Pill("Open the deck", filled = true, modifier = Modifier.fillMaxWidth(), onClick = onOpenDeck)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Pill("Scan again", modifier = Modifier.weight(1f), onClick = onScanDeck)
                    Pill("Forget", modifier = Modifier.weight(1f)) { PcDeck.forget() }
                }
                Toggle(
                    "Phone notifications on PC",
                    "Your notifications and battery show on the PC island, with a Copy button for sign-in codes",
                    sendNotifs && canNotifs,
                    onSendNotifs,
                )
                if (sendNotifs && !canNotifs) {
                    Label(
                        "Turn on Winklet in the list Android opened. If it says the setting is restricted: " +
                            "App info › ⋮ › Allow restricted settings, then try again.",
                        size = 12.sp,
                    )
                }
            }
        }

        Section("Theme") {
            Palettes.chunked(3).forEachIndexed { r, row ->
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    row.forEachIndexed { c, p ->
                        val i = r * 3 + c
                        val on = i == theme
                        Column(
                            Modifier
                                .weight(1f)
                                .clip(RoundedCornerShape(14.dp))
                                .background(p.island)
                                .border(if (on) 2.dp else 1.dp, if (on) p.accent else p.stroke, RoundedCornerShape(14.dp))
                                .clickable { Prefs.theme.set(i) }
                                .padding(10.dp),
                        ) {
                            Box(
                                Modifier.fillMaxWidth().height(10.dp).clip(RoundedCornerShape(5.dp))
                                    .background(Brush.horizontalGradient(listOf(p.gradA, p.gradB, p.gradC))),
                            )
                            Spacer(Modifier.height(8.dp))
                            Text(p.name, color = p.text, fontSize = 12.sp, maxLines = 2, lineHeight = 14.sp, minLines = 2)
                        }
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Pill("Dark", filled = !light) { Prefs.lightMode.set(false) }
                Pill("Light", filled = light) { Prefs.lightMode.set(true) }
            }
            Label("Light turns the open panel into soft paper tones; the bubble stays dark so the eyes glow.", size = 12.sp)
        }

        Section("Eyes") {
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                (0 until EyeTint.COUNT).forEach { i ->
                    val c = EyeTint.get(i, t.palette)
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Box(
                            Modifier
                                .size(38.dp)
                                .background(c.fill, CircleShape)
                                .border(if (i == eye) 3.dp else 0.dp, if (i == eye) c.glow else Color.Transparent, CircleShape)
                                .clickable {
                                    Prefs.eyeColor.set(i)
                                    EyeEvents.play(Mood.Happy)
                                },
                        )
                        Label(EyeTint.name(i), Modifier.padding(top = 4.dp), size = 11.sp)
                    }
                }
            }
            Toggle("Lively eyes", "Random events every 7-17 s", lively) { Prefs.lively.set(it) }
            Label("Try an event", size = 12.sp)
            Mood.entries.drop(1).chunked(4).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    row.forEach { m -> Pill(m.label, modifier = Modifier.weight(1f), compact = true) { EyeEvents.play(m) } }
                }
            }
        }

        Section("Water") {
            Stepper("Daily goal", "$waterGoal ml", { Prefs.waterGoal.set((waterGoal - 250).coerceAtLeast(500)) }) {
                Prefs.waterGoal.set((waterGoal + 250).coerceAtMost(6000))
            }
            Label("Reminders", size = 12.sp)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf(-1 to "Smart", 0 to "Off", 30 to "30m", 60 to "1h", 120 to "2h").forEach { (m, l) ->
                    Pill(l, filled = waterEvery == m, modifier = Modifier.weight(1f), compact = true) { onWaterEvery(m) }
                }
            }
            Label(
                if (waterEvery < 0) "Smart spreads your goal over the day and reminds you only when you fall behind. Every drink pushes the next one back."
                else "Only during your day.",
                size = 12.sp,
            )
            Stepper("Day starts", "%02d:00".format(dayStart), { onDay(dayStart - 1, dayEnd) }) { onDay(dayStart + 1, dayEnd) }
            Stepper("Day ends", "%02d:00".format(dayEnd % 24), { onDay(dayStart, dayEnd - 1) }) { onDay(dayStart, dayEnd + 1) }
        }

        Section("Health") {
            Stepper("Step goal", "%,d".format(stepGoal), { Prefs.stepGoal.set((stepGoal - 1000).coerceAtLeast(1000)) }) {
                Prefs.stepGoal.set((stepGoal + 1000).coerceAtMost(30000))
            }
            Toggle("Eye rest 20-20-20", "After 20 minutes of screen, look away for 20 s", eyeRest) { Prefs.eyeRest.set(it) }
        }

        Label("Tap the bubble to open it, drag it to move it (it snaps to the nearest edge), long-press it to hide it. Back or a tap outside closes the panel.",
            size = 12.sp, color = t.textFaint)
    }
}

@Composable
private fun Section(title: String, content: @Composable ColumnScope.() -> Unit) {
    val t = LocalTokens.current
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(20.dp))
            .background(t.surface)
            .border(1.dp, t.stroke, RoundedCornerShape(20.dp))
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(title, color = t.text, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
        content()
    }
}

@Composable
private fun Toggle(title: String, sub: String, on: Boolean, set: (Boolean) -> Unit) {
    val t = LocalTokens.current
    Row(Modifier.fillMaxWidth().clickable { set(!on) }, verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, color = t.text, fontSize = 15.sp, fontWeight = FontWeight.Medium)
            Label(sub, size = 12.sp)
        }
        Box(
            Modifier.size(46.dp, 26.dp).clip(RoundedCornerShape(50)).background(if (on) t.accent else t.wash(0.15f)).padding(3.dp),
            contentAlignment = if (on) Alignment.CenterEnd else Alignment.CenterStart,
        ) {
            Box(Modifier.size(20.dp).clip(CircleShape).background(if (on) onColor(t.accent) else t.textDim))
        }
    }
}

@Composable
private fun Stepper(title: String, value: String, minus: () -> Unit, plus: () -> Unit) {
    val t = LocalTokens.current
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(title, color = t.text, fontSize = 15.sp, modifier = Modifier.weight(1f))
        Pill("−", onClick = minus)
        Text(value, color = t.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(horizontal = 12.dp))
        Pill("+", onClick = plus)
    }
}
