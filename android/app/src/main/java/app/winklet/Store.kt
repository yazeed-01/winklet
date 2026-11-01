package app.winklet

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.time.LocalDate
import java.util.UUID
import java.util.concurrent.Executors

data class Task(
    val id: String,
    val text: String,
    /** Epoch millis of the reminder, or null for no reminder. */
    val due: Long? = null,
    /** Day the task belongs to, or null for "any day". */
    val day: LocalDate? = null,
    val done: Boolean = false,
)

data class DrinkType(val id: String, val name: String, val color: Long, val ml: Int)

/** One drink logged: when, what, how much. */
data class Sip(val time: Long, val type: String, val ml: Int)

/** One day of phone wellbeing. */
data class Day(
    val steps: Int = 0,
    val screenSec: Int = 0,
    val unlocks: Int = 0,
    val waterMl: Int = 0,
)

data class Data(
    val tasks: List<Task> = emptyList(),
    /** The drinks to pick from: the defaults plus your own, in order. */
    val drinks: List<DrinkType> = Drinks,
    /** Today's drinks, oldest first (older days are rolled into [days]). */
    val sips: List<Sip> = emptyList(),
    val sipsDay: LocalDate = LocalDate.now(),
    /** The size picked last time for each drink. */
    val lastSize: Map<String, Int> = emptyMap(),
    val days: Map<LocalDate, Day> = emptyMap(),
    /** Steps per hour for [hoursDay]. */
    val hourSteps: List<Int> = List(24) { 0 },
    val hoursDay: LocalDate = LocalDate.now(),
    /** The step counter's last reading (it counts since boot), -1 = none yet. */
    val lastCounter: Long = -1,
) {
    fun today(): Day = days[LocalDate.now()] ?: Day()
    fun waterToday(): Int = if (sipsDay == LocalDate.now()) sips.sumOf { it.ml } else 0
    fun drink(id: String): DrinkType = drinks.firstOrNull { it.id == id } ?: Drinks.firstOrNull { it.id == id } ?: Drinks[0]
}

/** The drinks everyone starts with: the desktop's list. */
val Drinks = listOf(
    DrinkType("water", "Water", 0xFF38BDF8, 250),
    DrinkType("tea", "Tea", 0xFFD97706, 200),
    DrinkType("green-tea", "Green tea", 0xFFA3E635, 200),
    DrinkType("coffee", "Coffee", 0xFFB07A4F, 200),
    DrinkType("espresso", "Espresso", 0xFF8B5A3C, 40),
    DrinkType("iced-latte", "Iced latte", 0xFFE8C9A0, 350),
    DrinkType("spanish-latte", "Spanish latte", 0xFFF2B880, 300),
    DrinkType("matcha", "Matcha latte", 0xFF4ADE80, 300),
    DrinkType("juice", "Juice", 0xFFFB923C, 250),
    DrinkType("milk", "Milk", 0xFFE5E7EB, 250),
    DrinkType("sparkling", "Sparkling", 0xFF67E8F9, 330),
    DrinkType("hot-chocolate", "Hot chocolate", 0xFFA0522D, 250),
)

/** Colours for your own drinks, as on the desktop. */
val DrinkPalette = listOf(
    0xFF38BDF8, 0xFF60A5FA, 0xFFA78BFA, 0xFFF472B6, 0xFFF87171, 0xFFFB923C,
    0xFFFBBF24, 0xFFA3E635, 0xFF4ADE80, 0xFF2DD4BF, 0xFFC08457, 0xFFE8C9A0,
)

/** Cups drawn out for picking how much. */
val Sizes = listOf(
    "Shot" to 40, "Tea glass" to 150, "Cup" to 250, "Big mug" to 350,
    "Tall glass" to 400, "Bottle" to 500, "Big bottle" to 1000,
)

/**
 * Everything Winklet remembers, in one JSON file. The setup screen, the overlay and the
 * reminder receiver share it in one process; writes go to disk off the main thread.
 */
object Store {
    private lateinit var file: File
    private val io = Executors.newSingleThreadExecutor()
    private val _data = MutableStateFlow(Data())
    val data: StateFlow<Data> = _data

    /** Called after a drink is logged or undone, so the next water reminder can move. */
    var onSipsChanged: (() -> Unit)? = null

    fun init(context: Context) {
        if (::file.isInitialized) return
        file = File(context.filesDir, "winklet.json")
        _data.value = runCatching { fromJson(JSONObject(file.readText())) }.getOrDefault(Data())
        edit { it }
    }

    /** Applies a change (rolling over to a new day first) and saves it. */
    fun edit(change: (Data) -> Data) {
        _data.update { change(rollover(it)) }
        val snapshot = _data.value
        io.execute {
            val tmp = File(file.parentFile, "winklet.json.tmp")
            tmp.writeText(toJson(snapshot).toString())
            tmp.renameTo(file)
        }
    }

    private fun rollover(d: Data): Data {
        val today = LocalDate.now()
        var out = d
        if (d.sipsDay != today) {
            // fold the finished day's drinks into its history
            val prev = d.days[d.sipsDay] ?: Day()
            out = out.copy(
                days = out.days + (d.sipsDay to prev.copy(waterMl = d.sips.sumOf { it.ml })),
                sips = emptyList(),
                sipsDay = today,
            )
        }
        if (d.hoursDay != today) out = out.copy(hourSteps = List(24) { 0 }, hoursDay = today)
        // keep two months of history
        val cutoff = today.minusDays(60)
        if (out.days.keys.any { it < cutoff }) out = out.copy(days = out.days.filterKeys { it >= cutoff })
        return out
    }

    // --- tasks ---

    fun addTask(input: String, day: LocalDate? = null): Task? {
        val p = parseTask(input, day) ?: return null
        val t = Task(UUID.randomUUID().toString().take(12), p.text, p.due, p.day)
        edit { it.copy(tasks = it.tasks + t) }
        return t
    }

    fun toggleTask(id: String) = edit { d ->
        d.copy(tasks = d.tasks.map { if (it.id == id) it.copy(done = !it.done) else it })
    }

    fun deleteTask(id: String) = edit { d -> d.copy(tasks = d.tasks.filterNot { it.id == id }) }

    fun clearDone() = edit { d -> d.copy(tasks = d.tasks.filterNot { it.done }) }

    // --- water ---

    fun addSip(type: String, ml: Int) = edit { d ->
        d.copy(
            sips = d.sips + Sip(System.currentTimeMillis(), type, ml),
            lastSize = d.lastSize + (type to ml),
        )
    }.also { onSipsChanged?.invoke() }

    fun undoSip() = edit { d -> d.copy(sips = d.sips.dropLast(1)) }.also { onSipsChanged?.invoke() }

    /** Adds a new drink ([id] null) or renames / recolours one. */
    fun saveDrink(id: String?, name: String, color: Long) = edit { d ->
        val clean = name.trim().ifEmpty { return@edit d }
        if (id == null) {
            d.copy(drinks = d.drinks + DrinkType(UUID.randomUUID().toString().take(12), clean, color, 250))
        } else {
            d.copy(drinks = d.drinks.map { if (it.id == id) it.copy(name = clean, color = color) else it })
        }
    }

    /** Removes a drink from the list (water always stays). Drinks already logged keep their colour. */
    fun removeDrink(id: String) = edit { d ->
        if (id == "water") d else d.copy(drinks = d.drinks.filterNot { it.id == id })
    }

    // --- health ---

    /** Adds steps from a fresh step-counter reading. */
    fun stepCounter(value: Long) = edit { d ->
        val delta = when {
            d.lastCounter < 0 -> 0L
            value < d.lastCounter -> value // rebooted: the counter started over
            else -> value - d.lastCounter
        }.toInt().coerceAtMost(20_000)
        if (delta == 0) return@edit d.copy(lastCounter = value)
        val today = LocalDate.now()
        val day = d.today()
        val hour = java.time.LocalTime.now().hour
        d.copy(
            lastCounter = value,
            days = d.days + (today to day.copy(steps = day.steps + delta)),
            hourSteps = d.hourSteps.mapIndexed { i, s -> if (i == hour) s + delta else s },
        )
    }

    fun addScreenTime(sec: Int) = edit { d ->
        if (sec <= 0) return@edit d
        val day = d.today()
        d.copy(days = d.days + (LocalDate.now() to day.copy(screenSec = day.screenSec + sec)))
    }

    fun addUnlock() = edit { d ->
        val day = d.today()
        d.copy(days = d.days + (LocalDate.now() to day.copy(unlocks = day.unlocks + 1)))
    }

    // --- JSON ---

    private fun toJson(d: Data) = JSONObject().apply {
        put("tasks", JSONArray(d.tasks.map {
            JSONObject().put("id", it.id).put("text", it.text).put("done", it.done)
                .put("due", it.due ?: JSONObject.NULL).put("day", it.day?.toString() ?: JSONObject.NULL)
        }))
        put("drinks", JSONArray(d.drinks.map {
            JSONObject().put("id", it.id).put("name", it.name).put("color", it.color).put("ml", it.ml)
        }))
        put("sips", JSONArray(d.sips.map { JSONObject().put("t", it.time).put("type", it.type).put("ml", it.ml) }))
        put("sipsDay", d.sipsDay.toString())
        put("lastSize", JSONObject(d.lastSize))
        put("days", JSONObject().apply {
            d.days.forEach { (k, v) ->
                put(k.toString(), JSONObject().put("steps", v.steps).put("screen", v.screenSec)
                    .put("unlocks", v.unlocks).put("water", v.waterMl))
            }
        })
        put("hourSteps", JSONArray(d.hourSteps))
        put("hoursDay", d.hoursDay.toString())
        put("lastCounter", d.lastCounter)
    }

    private fun fromJson(o: JSONObject): Data {
        val tasks = o.optJSONArray("tasks").objects().map {
            Task(
                it.getString("id"), it.getString("text"),
                if (it.isNull("due")) null else it.getLong("due"),
                if (it.isNull("day")) null else LocalDate.parse(it.getString("day")),
                it.optBoolean("done"),
            )
        }
        val drinks = o.optJSONArray("drinks").objects()
            .map { DrinkType(it.getString("id"), it.getString("name"), it.getLong("color"), it.optInt("ml", 250)) }
            .ifEmpty { Drinks }
        val sips = o.optJSONArray("sips").objects().map { Sip(it.getLong("t"), it.getString("type"), it.getInt("ml")) }
        val last = o.optJSONObject("lastSize")?.let { j -> j.keys().asSequence().associateWith { j.getInt(it) } } ?: emptyMap()
        val days = o.optJSONObject("days")?.let { j ->
            j.keys().asSequence().associate { k ->
                val v = j.getJSONObject(k)
                LocalDate.parse(k) to Day(v.optInt("steps"), v.optInt("screen"), v.optInt("unlocks"), v.optInt("water"))
            }
        } ?: emptyMap()
        val hours = o.optJSONArray("hourSteps")?.let { a -> List(24) { a.optInt(it) } } ?: List(24) { 0 }
        return Data(
            tasks, drinks, sips,
            o.optString("sipsDay").toDateOr(LocalDate.now()),
            last, days, hours,
            o.optString("hoursDay").toDateOr(LocalDate.now()),
            o.optLong("lastCounter", -1),
        )
    }

    private fun JSONArray?.objects(): List<JSONObject> =
        if (this == null) emptyList() else List(length()) { getJSONObject(it) }

    private fun String.toDateOr(def: LocalDate) = runCatching { LocalDate.parse(this) }.getOrDefault(def)
}
