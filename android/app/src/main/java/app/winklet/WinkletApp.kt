package app.winklet

import android.app.Application

/** Loads settings and data once per process, and keeps water reminders paced to what you drink. */
class WinkletApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Prefs.init(this)
        Store.init(this)
        PcDeck.init(this)
        Reminders.channel(this)
        Store.onSipsChanged = { Reminders.scheduleWater(this) }
    }
}
