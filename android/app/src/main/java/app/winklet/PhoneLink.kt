package app.winklet

import android.app.Notification
import android.app.NotificationManager
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.SystemClock
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Phone notifications on the PC: with notification access, sends each new notification and
 * the battery to the paired PC (the same secret link as the PC deck). The PC shows them as
 * peeks, with a Copy button for sign-in codes. Nothing is sent unless it's turned on.
 */
class PhoneLink : NotificationListenerService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var loop: Job? = null
    private var battery: Pair<Int, Boolean>? = null
    /** message -> when it was last sent, so apps re-posting the same text don't repeat it */
    private val recent = LinkedHashMap<String, Long>()

    private val batteryReceiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            val level = i.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
            val scale = i.getIntExtra(BatteryManager.EXTRA_SCALE, 100)
            val status = i.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
            if (level < 0) return
            val now = level * 100 / scale to (status == BatteryManager.BATTERY_STATUS_CHARGING || status == BatteryManager.BATTERY_STATUS_FULL)
            // this fires for voltage and temperature too: only send real changes
            if (now != battery) {
                battery = now
                sendStatus()
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        Prefs.init(this)
        PcDeck.init(this)
    }

    override fun onListenerConnected() {
        ContextCompat.registerReceiver(this, batteryReceiver, IntentFilter(Intent.ACTION_BATTERY_CHANGED), ContextCompat.RECEIVER_NOT_EXPORTED)
        loop?.cancel()
        loop = scope.launch {
            // right away when turned on or paired, then a check-in every minute
            launch { combine(Prefs.sendNotifs.state, PcDeck.link) { on, link -> on && link != null }.collect { if (it) sendStatus() } }
            while (true) {
                delay(60_000)
                sendStatus()
            }
        }
    }

    override fun onListenerDisconnected() {
        loop?.cancel()
        runCatching { unregisterReceiver(batteryReceiver) }
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private fun active() = Prefs.sendNotifs.value && PcDeck.link.value != null

    private fun sendStatus() {
        val (pct, charging) = battery ?: return
        if (!active()) return
        val json = JSONObject().put("type", "status").put("battery", pct).put("charging", charging).toString()
        scope.launch { PcDeck.send(json) }
    }

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        if (!active() || sbn.packageName == packageName) return
        val n = sbn.notification
        // music players, downloads, navigation and the like keep a notification up: skip them
        if (sbn.isOngoing || n.flags and Notification.FLAG_GROUP_SUMMARY != 0) return
        if (n.category in SKIP) return
        // silent ones (low importance) aren't worth a peek on the PC
        val rank = Ranking()
        if (currentRanking?.getRanking(sbn.key, rank) == true && rank.importance < NotificationManager.IMPORTANCE_DEFAULT) return

        val e = n.extras
        val title = e.getCharSequence(Notification.EXTRA_TITLE)?.toString().orEmpty().trim()
        val text = (e.getCharSequence(Notification.EXTRA_BIG_TEXT) ?: e.getCharSequence(Notification.EXTRA_TEXT))
            ?.toString().orEmpty().trim()
        if (title.isEmpty() && text.isEmpty()) return

        val sig = "${sbn.packageName}|$title|$text"
        val now = SystemClock.elapsedRealtime()
        if (recent[sig]?.let { now - it < 30_000 } == true) return
        recent[sig] = now
        if (recent.size > 60) recent.remove(recent.keys.first())

        val app = runCatching {
            packageManager.getApplicationLabel(packageManager.getApplicationInfo(sbn.packageName, 0)).toString()
        }.getOrDefault(sbn.packageName)
        val json = JSONObject()
            .put("type", "notif")
            .put("app", app)
            .put("title", title.take(200))
            .put("text", text.take(600))
            .toString()
        scope.launch { PcDeck.send(json) }
    }

    companion object {
        private val SKIP = setOf(
            Notification.CATEGORY_TRANSPORT,
            Notification.CATEGORY_SERVICE,
            Notification.CATEGORY_PROGRESS,
            Notification.CATEGORY_SYSTEM,
        )

        /** Whether the user gave Winklet notification access. */
        fun allowed(ctx: Context) = NotificationManagerCompat.getEnabledListenerPackages(ctx).contains(ctx.packageName)

        /** Asks Android to bind the listener again (after access was turned on). */
        fun rebind(ctx: Context) = requestRebind(ComponentName(ctx, PhoneLink::class.java))
    }
}
