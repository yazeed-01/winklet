package app.winklet

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BlurMaskFilter
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import androidx.compose.ui.graphics.toArgb
import androidx.core.app.NotificationCompat
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/**
 * The island's ongoing notification: the eyes, water with a +cup button, steps and the next
 * task. It also hides and brings back the bubble. Kept off the lock screen.
 */
object IslandCard {
    private const val CHANNEL = "island_card3"
    const val ID = 1

    fun channel(ctx: Context) {
        val nm = ctx.getSystemService(NotificationManager::class.java)
        // earlier versions' channels
        listOf("island", "island_card", "island_card2").forEach(nm::deleteNotificationChannel)
        // quiet, and never on the lock screen
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, "Island", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Water, steps and your next task; tap to show or hide the bubble"
                setShowBadge(false)
                lockscreenVisibility = Notification.VISIBILITY_SECRET
            },
        )
    }

    /** What the card shows; the notification is only rebuilt when this changes. */
    data class Content(val title: String, val text: String, val eye: Int, val hidden: Boolean)

    fun content(d: Data): Content {
        val goal = Prefs.waterGoal.value
        val steps = d.today().steps
        val title = "Water %,d / %,d ml  ·  %,d steps".format(d.waterToday(), goal, steps)
        val next = d.tasks.filter { !it.done }.minByOrNull { it.due ?: Long.MAX_VALUE }
        val hidden = Prefs.bubbleHidden.value
        val text = when {
            hidden -> "Bubble hidden · tap to show it"
            next != null -> "Next: ${next.text}" + (next.due?.let { "  ·  " + due(it) } ?: "")
            else -> "Screen ${duration(d.today().screenSec)}  ·  ${d.today().unlocks} unlocks"
        }
        return Content(title, text, Prefs.eyeColor.value, hidden)
    }

    fun build(ctx: Context, c: Content): Notification {
        val open = PendingIntent.getActivity(ctx, 0, Intent(ctx, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val toggle = IslandService.toggleIntent(ctx, show = c.hidden)
        return NotificationCompat.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_eyes)
            .setLargeIcon(eyes(EyeTint.get(c.eye, Palettes[Prefs.theme.value.coerceIn(0, Palettes.lastIndex)])))
            .setContentTitle(c.title)
            .setContentText(c.text)
            // hidden: a tap brings the bubble back; shown: it opens settings
            .setContentIntent(if (c.hidden) toggle else open)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setVisibility(NotificationCompat.VISIBILITY_SECRET)
            .addAction(0, "+cup of water", Reminders.cupIntent(ctx))
            .addAction(0, if (c.hidden) "Show bubble" else "Hide bubble", toggle)
            .build()
    }

    private fun due(ms: Long): String {
        val t = Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault())
        val hm = t.format(DateTimeFormatter.ofPattern("HH:mm"))
        return if (t.toLocalDate() == LocalDate.now()) hm else t.format(DateTimeFormatter.ofPattern("EEE ")) + hm
    }

    /** The bubble with its two glowing eyes, as a picture for the notification. */
    private fun eyes(color: EyeColor): Bitmap {
        val size = 192
        val bmp = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val cv = Canvas(bmp)
        val p = Paint(Paint.ANTI_ALIAS_FLAG)
        p.color = 0xFF0B0B0F.toInt()
        cv.drawCircle(size / 2f, size / 2f, size / 2f, p)
        val w = 26f
        val h = 50f
        for (cx in listOf(size / 2f - 27f, size / 2f + 27f)) {
            val r = RectF(cx - w / 2, size / 2f - h / 2, cx + w / 2, size / 2f + h / 2)
            p.color = color.glow.copy(alpha = 0.7f).toArgb()
            p.maskFilter = BlurMaskFilter(16f, BlurMaskFilter.Blur.NORMAL)
            cv.drawRoundRect(r, w / 2, w / 2, p)
            p.maskFilter = null
            p.color = color.fill.toArgb()
            cv.drawRoundRect(r, w / 2, w / 2, p)
        }
        return bmp
    }
}
