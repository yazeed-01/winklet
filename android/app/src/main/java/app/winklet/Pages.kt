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
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlin.math.PI
import kotlin.math.sin

private val Hm = DateTimeFormatter.ofPattern("HH:mm")
private val DayName = DateTimeFormatter.ofPattern("EEE")

@Composable
private fun PageColumn(content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 8.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
        content = content,
    )
}

// ---------------------------------------------------------------- Home

@Composable
fun HomePage(m: IslandModel, onTab: (Tab) -> Unit, onDeck: () -> Unit) {
    val t = LocalTokens.current
    val data by Store.data.collectAsState()
    val goal by Prefs.waterGoal.state.collectAsState()
    val stepGoal by Prefs.stepGoal.state.collectAsState()
    val now by produceState(LocalDateTime.now()) {
        while (true) {
            value = LocalDateTime.now()
            // tick on each new second (only while Home is open)
            delay(1_000L - value.nano / 1_000_000)
        }
    }
    PageColumn {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.Bottom) {
                    Text(now.format(Hm), color = t.text, fontSize = 52.sp, fontWeight = FontWeight.Light)
                    Text(
                        ":%02d".format(now.second), color = t.textDim, fontSize = 26.sp, fontWeight = FontWeight.Light,
                        modifier = Modifier.padding(bottom = 9.dp),
                    )
                }
                Label(now.format(DateTimeFormatter.ofPattern("EEEE d MMMM")), size = 14.sp)
            }
            BatteryBadge(m.battery)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            val next = data.tasks.filter { !it.done }.sortedBy { it.due ?: Long.MAX_VALUE }.firstOrNull()
            Card(Modifier.weight(1f).height(96.dp), onClick = { onTab(Tab.Todo) }) {
                Label("Next task", size = 11.sp)
                Spacer(Modifier.height(4.dp))
                Text(next?.text ?: "All clear", color = t.text, fontSize = 15.sp, fontWeight = FontWeight.Medium,
                    maxLines = 2, overflow = TextOverflow.Ellipsis)
                next?.due?.let { Label(dueLabel(it), size = 11.sp, color = t.accent) }
            }
            Card(Modifier.weight(1f).height(96.dp), onClick = { onTab(Tab.Water) }) {
                val have = data.waterToday()
                Label("Water", size = 11.sp)
                Spacer(Modifier.height(4.dp))
                Big("$have ml", size = 18.sp)
                Label("of $goal", size = 11.sp)
                Spacer(Modifier.weight(1f))
                Progress(have / goal.toFloat(), t.blue)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Card(Modifier.weight(1f).height(84.dp), onClick = { onTab(Tab.Health) }) {
                val steps = data.today().steps
                Label("Steps", size = 11.sp)
                Spacer(Modifier.height(4.dp))
                Big("%,d".format(steps), size = 18.sp)
                Spacer(Modifier.weight(1f))
                Progress(steps / stepGoal.toFloat(), t.green)
            }
            Card(Modifier.weight(1f).height(84.dp), onClick = { onTab(Tab.Health) }) {
                Label("Screen time", size = 11.sp)
                Spacer(Modifier.height(4.dp))
                Big(duration(data.today().screenSec), size = 18.sp)
                Label("${data.today().unlocks} unlocks", size = 11.sp)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Pill("+ cup of water", filled = true, color = t.blue) { Store.addSip("water", 250) }
            Pill("Deck", onClick = onDeck)
            Pill("Hide bubble") { Prefs.bubbleHidden.set(true) }
        }
    }
}

@Composable
fun BatteryBadge(b: Battery) {
    val t = LocalTokens.current
    val tint = when {
        b.charging -> t.green
        b.level <= 15 -> t.red
        else -> t.text
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(if (b.charging) "⚡${b.level}%" else "${b.level}%", color = tint, fontSize = 14.sp)
        Box(
            Modifier
                .size(22.dp, 11.dp)
                .border(1.dp, tint.copy(alpha = 0.7f), RoundedCornerShape(3.dp))
                .padding(2.dp),
        ) {
            Box(Modifier.size((18 * b.level / 100f).coerceAtLeast(1f).dp, 7.dp).background(tint, RoundedCornerShape(1.dp)))
        }
    }
}

@Composable
private fun Progress(f: Float, color: Color) {
    val t = LocalTokens.current
    Box(Modifier.fillMaxWidth().height(5.dp).clip(RoundedCornerShape(3.dp)).background(t.wash(0.08f))) {
        Box(Modifier.fillMaxWidth(f.coerceIn(0f, 1f)).fillMaxHeight().background(color, RoundedCornerShape(3.dp)))
    }
}

private fun dueLabel(due: Long): String {
    val dt = LocalDateTime.ofInstant(Instant.ofEpochMilli(due), ZoneId.systemDefault())
    val day = when (dt.toLocalDate()) {
        LocalDate.now() -> "Today"
        LocalDate.now().plusDays(1) -> "Tomorrow"
        else -> dt.format(DateTimeFormatter.ofPattern("EEE d MMM"))
    }
    return "$day ${dt.format(Hm)}"
}

// ---------------------------------------------------------------- To-do

@Composable
fun TodoPage() {
    val t = LocalTokens.current
    val ctx = LocalContext.current
    val data by Store.data.collectAsState()
    var input by remember { mutableStateOf("") }

    fun add() {
        Store.addTask(input)?.let { Reminders.scheduleTask(ctx, it) }
        input = ""
    }

    PageColumn {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Box(
                Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(12.dp))
                    .background(t.surface)
                    .border(1.dp, t.stroke, RoundedCornerShape(12.dp))
                    .padding(horizontal = 12.dp, vertical = 10.dp),
            ) {
                if (input.isEmpty()) Label("Call mom @18:30", size = 14.sp, color = t.textFaint)
                BasicTextField(
                    input, { input = it },
                    Modifier.fillMaxWidth(),
                    singleLine = true,
                    textStyle = TextStyle(color = t.text, fontSize = 14.sp),
                    cursorBrush = SolidColor(t.accent),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { add() }),
                )
            }
            Pill("Add", filled = true) { add() }
        }
        Label("Add a time with @: @18:30, @tomorrow 7am, @2026-11-02 09:00", size = 11.sp, color = t.textFaint)

        val tasks = data.tasks.sortedWith(compareBy<Task> { it.done }.thenBy { it.due ?: Long.MAX_VALUE })
        if (tasks.isEmpty()) {
            Box(Modifier.fillMaxWidth().padding(vertical = 36.dp), contentAlignment = Alignment.Center) {
                Label("Nothing to do. Nice.", size = 14.sp)
            }
        }
        tasks.forEach { task ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(12.dp))
                    .background(t.surface)
                    .padding(horizontal = 10.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Tick(task.done) {
                    Store.toggleTask(task.id)
                    if (!task.done) Reminders.cancelTask(ctx, task.id) else Reminders.scheduleTask(ctx, task.copy(done = false))
                }
                Column(Modifier.weight(1f)) {
                    Text(
                        task.text,
                        color = if (task.done) t.textFaint else t.text, fontSize = 14.sp,
                        textDecoration = if (task.done) TextDecoration.LineThrough else null,
                        maxLines = 2, overflow = TextOverflow.Ellipsis,
                    )
                    task.due?.let {
                        val late = !task.done && it < System.currentTimeMillis()
                        Label(dueLabel(it), size = 11.sp, color = if (late) t.red else t.accent)
                    }
                }
                Text("×", color = t.textFaint, fontSize = 20.sp,
                    modifier = Modifier.clickable {
                        Store.deleteTask(task.id)
                        Reminders.cancelTask(ctx, task.id)
                    }.padding(horizontal = 6.dp))
            }
        }
        if (tasks.any { it.done }) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                Pill("Clear done") { Store.clearDone() }
            }
        }
    }
}

// ---------------------------------------------------------------- Health

@Composable
fun HealthPage(stepsReady: Boolean) {
    val t = LocalTokens.current
    val data by Store.data.collectAsState()
    val goal by Prefs.stepGoal.state.collectAsState()
    val eyeRest by Prefs.eyeRest.state.collectAsState()
    val today = data.today()

    PageColumn {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Ring(today.steps / goal.toFloat(), "%,d".format(today.steps), "of %,d steps".format(goal), t.green, size = 96.dp, stroke = 9.dp)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                StatRow("Screen time", duration(today.screenSec))
                StatRow("Unlocks", "${today.unlocks}")
                StatRow("Distance", "%.1f km".format(today.steps * 0.00075f))
                StatRow("Calories", "${(today.steps * 0.04f).toInt()} kcal")
            }
        }
        if (!stepsReady) {
            Card {
                Label("Steps need \"Physical activity\" access. Open the Winklet app to allow it.", size = 12.sp, color = t.orange)
            }
        }
        Label("Steps by hour", size = 12.sp)
        Bars(
            data.hourSteps.map { it.toFloat() }, color = t.green, highlight = LocalTime.now().hour,
            labels = listOf("0", "", "", "", "", "", "6", "", "", "", "", "", "12", "", "", "", "", "", "18", "", "", "", "", ""),
        )
        Label("Last 7 days", size = 12.sp)
        val days = (6 downTo 0).map { LocalDate.now().minusDays(it.toLong()) }
        Bars(
            days.map { (data.days[it]?.steps ?: 0).toFloat() }, color = t.green, highlight = 6, goal = goal.toFloat(),
            labels = days.map { it.format(DayName) },
        )
        Bars(
            days.map { (data.days[it]?.screenSec ?: 0) / 3600f }, color = t.accent, highlight = 6,
            labels = days.map { d -> duration(data.days[d]?.screenSec ?: 0) },
        )
        Label("Screen time", size = 11.sp, color = t.textFaint)
        Card(onClick = { Prefs.eyeRest.set(!eyeRest) }) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Eye rest 20-20-20", color = t.text, fontSize = 14.sp, fontWeight = FontWeight.Medium)
                    Label("Every 20 min of screen, look 20 ft away for 20 s", size = 11.sp)
                }
                Text(if (eyeRest) "On" else "Off", color = if (eyeRest) t.accent else t.textFaint, fontSize = 14.sp)
            }
        }
    }
}

// ---------------------------------------------------------------- Device

/** Live device status: CPU, RAM, storage, battery, temperatures and network. */
@Composable
fun DevicePage(m: IslandModel) {
    val t = LocalTokens.current
    val s = m.stats

    PageColumn {
        if (s == null) {
            Label("Reading…", size = 13.sp)
            return@PageColumn
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Ring((s.cpuLoad ?: 0) / 100f, s.cpuLoad?.let { "$it%" } ?: "—", if (s.cpuEstimated) "CPU ≈" else "CPU", t.accent)
            Ring(s.ramPct / 100f, "${s.ramPct}%", "RAM", t.blue)
            Ring(s.storagePct / 100f, "${s.storagePct}%", "Storage", t.orange)
            Ring(s.batteryLevel / 100f, "${s.batteryLevel}%", "Battery", if (s.charging) t.green else t.pink)
        }
        Card {
            Row {
                Label("CPU last minute", size = 11.sp, modifier = Modifier.weight(1f))
                s.cpuMhz?.let { Label("$it / ${s.cpuMaxMhz} MHz", size = 11.sp) }
            }
            Spark(m.cpuHistory.toList(), t.accent, Modifier.padding(top = 6.dp))
        }
        Card {
            StatRow("CPU temp", s.cpuTemp?.let { "%.0f °C".format(it) } ?: "—", tempColor(s.cpuTemp, t))
            StatRow("GPU temp", s.gpuTemp?.let { "%.0f °C".format(it) } ?: "—", tempColor(s.gpuTemp, t))
            s.gpuLoad?.let { StatRow("GPU load", "$it%") }
            StatRow("Battery temp", s.batteryTemp?.let { "%.1f °C".format(it) } ?: "—", tempColor(s.batteryTemp, t))
            StatRow("Thermal", s.thermalName, if (s.thermal >= 2) t.orange else t.green)
        }
        Card {
            StatRow("RAM", "${bytes(s.ramUsed)} / ${bytes(s.ramTotal)}")
            StatRow("Storage", "${bytes(s.storageUsed)} / ${bytes(s.storageTotal)}")
            StatRow("CPU cores", "${s.cores}")
            StatRow("Refresh rate", "${s.refreshHz} Hz")
        }
        Card {
            s.batteryMa?.let { StatRow(if (it >= 0) "Charging at" else "Drawing", "${kotlin.math.abs(it)} mA") }
            s.batteryVolts?.let { StatRow("Voltage", "%.2f V".format(it)) }
            StatRow("Download", "${bytes(s.downBps)}/s")
            StatRow("Upload", "${bytes(s.upBps)}/s")
            StatRow("Up for", duration((s.uptimeMs / 1000).toInt()))
        }
        Label("Android doesn't let apps read GPU load or FPS, so those aren't shown.", size = 11.sp, color = t.textFaint)
    }
}

private fun tempColor(c: Float?, t: Tokens) = when {
    c == null -> t.textDim
    c >= 45 -> t.red
    c >= 38 -> t.orange
    else -> t.green
}
