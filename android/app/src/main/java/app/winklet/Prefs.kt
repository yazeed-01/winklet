package app.winklet

import android.content.Context
import android.content.SharedPreferences
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** How the resting island looks. Bubble is the default on Android. */
enum class IslandStyle { Bubble }

/**
 * Settings shared by the setup screen, the overlay and the reminder receiver. Values live in
 * SharedPreferences and are mirrored in flows so a change shows on the island at once.
 */
object Prefs {
    private lateinit var sp: SharedPreferences

    private fun int(key: String, def: Int) = Setting(key, def, { sp.getInt(key, def) }, { sp.edit().putInt(key, it).apply() })
    private fun bool(key: String, def: Boolean) =
        Setting(key, def, { sp.getBoolean(key, def) }, { sp.edit().putBoolean(key, it).apply() })

    class Setting<T>(val key: String, def: T, private val load: () -> T, private val store: (T) -> Unit) {
        private val flow = MutableStateFlow(def)
        val state: StateFlow<T> = flow
        val value: T get() = flow.value
        fun set(v: T) {
            flow.value = v
            store(v)
        }
        internal fun reload() {
            flow.value = load()
        }
    }

    val eyeColor = int("eye_color", 0)
    val lively = bool("lively", true)
    val theme = int("theme", 0)
    val lightMode = bool("light", false)
    val waterGoal = int("water_goal", 2500)
    /** Water reminders: -1 = smart (paced to the goal), 0 = off, else minutes between them. */
    val waterEvery = int("water_every_v2", -1)
    /** The waking day reminders are spread over, in hours. */
    val dayStart = int("day_start", 8)
    val dayEnd = int("day_end", 22)
    /** The bubble is tucked away: Winklet keeps running in the notification, tap it to bring it back. */
    val bubbleHidden = bool("bubble_hidden", false)
    val stepGoal = int("step_goal", 8000)
    /** 20-20-20: after 20 minutes of screen time, look 20 feet away for 20 seconds. */
    val eyeRest = bool("eye_rest", false)

    private val all = listOf(eyeColor, lively, theme, lightMode, waterGoal, waterEvery, dayStart, dayEnd, bubbleHidden, stepGoal, eyeRest)

    val style: IslandStyle get() = IslandStyle.Bubble

    fun init(context: Context) {
        if (::sp.isInitialized) return
        sp = context.applicationContext.getSharedPreferences("winklet", Context.MODE_PRIVATE)
        all.forEach { it.reload() }
    }

    /** Which side the bubble rests on, and how far down. */
    var onRight: Boolean
        get() = sp.getBoolean("on_right", true)
        set(v) = sp.edit().putBoolean("on_right", v).apply()

    var restY: Int
        get() = sp.getInt("rest_y", -1)
        set(v) = sp.edit().putInt("rest_y", v).apply()

    /** Whether the island should be on screen (brought back after a reboot). */
    var islandOn: Boolean
        get() = sp.getBoolean("island_on", false)
        set(v) = sp.edit().putBoolean("island_on", v).apply()

    /** When the next water reminder is due (epoch millis), 0 = none. */
    var waterNextAt: Long
        get() = sp.getLong("water_next", 0)
        set(v) = sp.edit().putLong("water_next", v).apply()

    /** The day the morning coffee was had (epoch day), so it happens once a day. */
    var coffeeDay: Long
        get() = sp.getLong("coffee_day", -1)
        set(v) = sp.edit().putLong("coffee_day", v).apply()
}
