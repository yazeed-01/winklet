package app.winklet

import android.Manifest
import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import java.time.Duration
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId

/** A short message that drops out of the bubble for a few seconds. */
data class Peek(val title: String, val body: String = "", val kind: Kind = Kind.Info, val at: Long = System.currentTimeMillis()) {
    enum class Kind { Info, Task, Water, Health }
}

object Peeks {
    private val _peek = MutableStateFlow<Peek?>(null)
    val peek: StateFlow<Peek?> = _peek
    fun show(p: Peek) {
        _peek.value = p
    }
    fun clear() {
        _peek.value = null
    }
}

/** Task reminders and water reminders, through AlarmManager so they fire while dozing. */
object Reminders {
    const val CHANNEL = "reminders"
    private const val WATER_ID = 2
    private const val CUP = 250
    private const val ACTION_TASK = "app.winklet.TASK"
    private const val ACTION_WATER = "app.winklet.WATER"
    private const val ACTION_CUP = "app.winklet.CUP"
    private const val ACTION_DONE = "app.winklet.DONE"

    fun channel(ctx: Context) {
        ctx.getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Reminders", NotificationManager.IMPORTANCE_DEFAULT).apply {
                description = "Task, water and eye-rest reminders"
            },
        )
    }

    private fun alarms(ctx: Context) = ctx.getSystemService(AlarmManager::class.java)

    private fun pending(ctx: Context, action: String, code: Int, extra: (Intent) -> Unit = {}) =
        PendingIntent.getBroadcast(
            ctx, code,
            Intent(ctx, ReminderReceiver::class.java).setAction(action).also(extra),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    private fun at(ctx: Context, time: Long, pi: PendingIntent) {
        val am = alarms(ctx)
        if (Build.VERSION.SDK_INT < 31 || am.canScheduleExactAlarms()) {
            am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, time, pi)
        } else {
            // without the exact-alarm permission it may come a few minutes late
            am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, time, pi)
        }
    }

    fun scheduleTask(ctx: Context, t: Task) {
        val due = t.due ?: return
        val pi = pending(ctx, ACTION_TASK, t.id.hashCode()) { it.putExtra("id", t.id) }
        if (t.done || due < System.currentTimeMillis()) alarms(ctx).cancel(pi) else at(ctx, due, pi)
    }

    fun cancelTask(ctx: Context, id: String) {
        alarms(ctx).cancel(pending(ctx, ACTION_TASK, id.hashCode()))
        ctx.getSystemService(NotificationManager::class.java).cancel(id.hashCode())
    }

    /**
     * Arms the next water reminder. Smart mode spreads the goal evenly over the waking day
     * and reminds you when you fall a cup behind that pace; every drink pushes it back.
     */
    fun scheduleWater(ctx: Context) {
        val pi = pending(ctx, ACTION_WATER, WATER_ID)
        val every = Prefs.waterEvery.value
        if (every == 0) {
            Prefs.waterNextAt = 0
            return alarms(ctx).cancel(pi)
        }
        val next = if (every < 0) smartNext() else fixedNext(every)
        Prefs.waterNextAt = next
        at(ctx, next, pi)
    }

    private fun millis(t: LocalDateTime) = t.atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()

    private fun day(date: LocalDate): Pair<LocalDateTime, LocalDateTime> {
        val start = Prefs.dayStart.value.coerceIn(0, 23)
        val end = Prefs.dayEnd.value.coerceIn(start + 1, 24)
        return date.atTime(start, 0) to date.atStartOfDay().plusHours(end.toLong())
    }

    /** How much you should have had by [now] to finish the goal by the end of the day. */
    fun pace(now: LocalDateTime = LocalDateTime.now()): Int {
        val (start, end) = day(now.toLocalDate())
        val span = Duration.between(start, end).toMinutes().toDouble()
        val done = Duration.between(start, now).toMinutes().coerceIn(0, span.toLong())
        return (Prefs.waterGoal.value * done / span).toInt()
    }

    private fun smartNext(): Long {
        val now = LocalDateTime.now()
        val (start, end) = day(now.toLocalDate())
        val goal = Prefs.waterGoal.value
        val have = Store.data.value.waterToday()
        // done for today, or the day is over: first reminder an hour after the next day starts
        if (have >= goal || now >= end.minusMinutes(20)) return millis(day(now.toLocalDate().plusDays(1)).first.plusHours(1))
        if (now < start) return millis(start.plusHours(1))
        // the moment the pace line passes what you've had by one more cup
        val span = Duration.between(start, end).toMinutes()
        var due = start.plusMinutes(((have + CUP).toDouble() / goal * span).toLong())
        // never nag more than every 30 min, never stay quiet more than 2 h
        due = maxOf(due, now.plusMinutes(30))
        due = minOf(due, now.plusHours(2), end.minusMinutes(15))
        return millis(due)
    }

    private fun fixedNext(every: Int): Long {
        val now = LocalDateTime.now()
        val (start, end) = day(now.toLocalDate())
        var due = now.plusMinutes(every.toLong())
        if (due < start) due = start
        if (due > end) due = day(now.toLocalDate().plusDays(1)).first.plusMinutes(every.toLong())
        return millis(due)
    }

    /** "+cup of water" from a notification button. */
    fun cupIntent(ctx: Context): PendingIntent = pending(ctx, ACTION_CUP, WATER_ID + 1)

    fun rescheduleAll(ctx: Context) {
        Store.data.value.tasks.forEach { scheduleTask(ctx, it) }
        scheduleWater(ctx)
    }

    internal fun handle(ctx: Context, i: Intent) {
        when (i.action) {
            ACTION_TASK -> {
                val t = Store.data.value.tasks.firstOrNull { it.id == i.getStringExtra("id") } ?: return
                if (t.done) return
                Peeks.show(Peek("Reminder", t.text, Peek.Kind.Task))
                notify(ctx, t.id.hashCode(), t.text, "Task reminder",
                    NotificationCompat.Action(0, "Done", pending(ctx, ACTION_DONE, t.id.hashCode()) { it.putExtra("id", t.id) }))
            }
            ACTION_DONE -> {
                val id = i.getStringExtra("id") ?: return
                Store.toggleTask(id)
                cancelTask(ctx, id)
            }
            ACTION_WATER -> {
                val now = LocalDateTime.now()
                val (start, end) = day(now.toLocalDate())
                val have = Store.data.value.waterToday()
                val goal = Prefs.waterGoal.value
                scheduleWater(ctx)
                if (now < start || now > end || have >= goal) return
                val behind = pace(now) - have
                val body = if (behind > 0) "$behind ml behind · $have of $goal ml" else "$have of $goal ml · stay on track"
                Peeks.show(Peek("Time for some water", body, Peek.Kind.Water))
                notify(ctx, WATER_ID, "Time for some water", body,
                    NotificationCompat.Action(0, "+cup", cupIntent(ctx)))
            }
            ACTION_CUP -> {
                Store.addSip("water", CUP)
                ctx.getSystemService(NotificationManager::class.java).cancel(WATER_ID)
            }
        }
    }

    fun notify(ctx: Context, id: Int, title: String, text: String, action: NotificationCompat.Action? = null) {
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return
        val open = PendingIntent.getActivity(ctx, 0, Intent(ctx, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val n = NotificationCompat.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_eyes)
            .setContentTitle(title)
            .setContentText(text)
            .setContentIntent(open)
            .setAutoCancel(true)
            .apply { action?.let { addAction(it) } }
            .build()
        ctx.getSystemService(NotificationManager::class.java).notify(id, n)
    }
}

class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, i: Intent) {
        Prefs.init(ctx)
        Store.init(ctx)
        Reminders.channel(ctx)
        Reminders.handle(ctx, i)
    }
}

/** After a reboot or an update: put the island back if it was on, and re-arm the alarms. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, i: Intent) {
        Prefs.init(ctx)
        Store.init(ctx)
        Reminders.rescheduleAll(ctx)
        if (Prefs.islandOn && android.provider.Settings.canDrawOverlays(ctx)) {
            ContextCompat.startForegroundService(ctx, Intent(ctx, IslandService::class.java))
        }
    }
}
