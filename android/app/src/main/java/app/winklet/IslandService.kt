package app.winklet

import android.Manifest
import android.animation.ValueAnimator
import android.app.NotificationManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.content.res.Configuration
import android.graphics.PixelFormat
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager
import android.os.SystemClock
import android.view.Gravity
import android.view.WindowInsets
import android.view.WindowManager
import android.view.animation.DecelerateInterpolator
import android.view.inputmethod.InputMethodManager
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewCompositionStrategy
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.setViewTreeLifecycleOwner
import androidx.savedstate.SavedStateRegistry
import androidx.savedstate.SavedStateRegistryController
import androidx.savedstate.SavedStateRegistryOwner
import androidx.savedstate.setViewTreeSavedStateRegistryOwner
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Keeps the island on screen: a foreground service that owns the overlay window. */
class IslandService : LifecycleService(), SavedStateRegistryOwner, SensorEventListener {
    private val savedState = SavedStateRegistryController.create(this)
    override val savedStateRegistry: SavedStateRegistry get() = savedState.savedStateRegistry

    private lateinit var wm: WindowManager
    private lateinit var root: OverlayRoot
    private lateinit var params: WindowManager.LayoutParams
    private lateinit var stats: StatsReader
    private val model = IslandModel()
    private var snap: ValueAnimator? = null
    private var dragging = false
    private var stepSensor: Sensor? = null

    // screen time: counted while the screen is on, flushed every minute and at screen off
    private var screenFlushedAt = 0L
    private var lastEyeRest = 0L

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            when (i.action) {
                Intent.ACTION_SCREEN_ON -> {
                    model.awake = true
                    screenFlushedAt = SystemClock.elapsedRealtime()
                    lastEyeRest = screenFlushedAt
                }
                Intent.ACTION_SCREEN_OFF -> {
                    flushScreenTime()
                    model.awake = false
                    openPanel(false)
                }
                Intent.ACTION_USER_PRESENT -> Store.addUnlock()
                Intent.ACTION_BATTERY_CHANGED -> model.battery = readBattery(i)
            }
        }
    }

    override fun onCreate() {
        savedState.performRestore(null)
        super.onCreate()
        Prefs.init(this)
        Store.init(this)
        Reminders.channel(this)
        startInForeground()

        wm = getSystemService(WINDOW_SERVICE) as WindowManager
        stats = StatsReader(this)
        model.awake = getSystemService(PowerManager::class.java).isInteractive
        screenFlushedAt = SystemClock.elapsedRealtime()
        lastEyeRest = screenFlushedAt

        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_ON)
            addAction(Intent.ACTION_SCREEN_OFF)
            addAction(Intent.ACTION_USER_PRESENT)
            addAction(Intent.ACTION_BATTERY_CHANGED)
        }
        ContextCompat.registerReceiver(this, receiver, filter, ContextCompat.RECEIVER_NOT_EXPORTED)
            ?.let { model.battery = readBattery(it) }

        addOverlay()
        Reminders.rescheduleAll(this)
        startLoops()
        _running.value = true
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        when (intent?.action) {
            ACTION_SHOW -> Prefs.bubbleHidden.set(false)
            ACTION_HIDE -> Prefs.bubbleHidden.set(true)
        }
        // started again from the app after it got the activity permission
        registerSteps()
        return START_STICKY
    }

    override fun onDestroy() {
        _running.value = false
        flushScreenTime()
        snap?.cancel()
        unregisterReceiver(receiver)
        getSystemService(SensorManager::class.java).unregisterListener(this)
        if (::root.isInitialized) wm.removeView(root)
        super.onDestroy()
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        // rotation: keep the island on screen and on its edge
        root.post { place() }
    }

    // ------------------------------------------------------------ overlay

    private fun addOverlay() {
        params = WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            RESTING_FLAGS,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            y = Prefs.restY.takeIf { it >= 0 } ?: (screen().second / 4)
            x = if (Prefs.onRight) screen().first else 0
        }

        root = OverlayRoot(
            this,
            canDrag = { !model.expanded },
            onDrag = { dx, dy ->
                snap?.cancel()
                dragging = true
                params.x += dx
                params.y += dy
                wm.updateViewLayout(root, params)
            },
            onDragEnd = ::snapToEdge,
            onOutside = { openPanel(false) },
            onBack = { model.expanded.also { openPanel(false) } },
        )
        root.setViewTreeLifecycleOwner(this)
        root.setViewTreeSavedStateRegistryOwner(this)
        root.addView(
            ComposeView(this).apply {
                setViewCompositionStrategy(ViewCompositionStrategy.DisposeOnDetachedFromWindow)
                setContent {
                    Island(
                        model,
                        onOpen = ::openPanel,
                        onTab = { model.tab = it },
                        onPeekTap = ::openFromPeek,
                    )
                }
            },
        )
        // whatever the island becomes (ball, capsule, peek, panel), keep it on its edge
        root.addOnLayoutChangeListener { _, l, t, r, b, ol, ot, or, ob ->
            if (r - l != or - ol || b - t != ob - ot) place()
        }
        wm.addView(root, params)
    }

    /** Puts the window against its side, clamped between the status and navigation bars. */
    private fun place() {
        if (dragging || snap?.isRunning == true || !root.isAttachedToWindow) return
        val w = root.width
        val h = root.height
        if (w == 0) return
        val (sw, sh) = screen()
        val (top, bottom) = bars()
        val m = dp(6)
        val x = if (Prefs.onRight) sw - w - m else m
        val rest = Prefs.restY.takeIf { it >= 0 } ?: (sh / 4)
        val y = rest.coerceIn(top + m, (sh - bottom - h - m).coerceAtLeast(top + m))
        if (x != params.x || y != params.y) {
            params.x = x
            params.y = y
            wm.updateViewLayout(root, params)
        }
    }

    /** Opens or closes the panel. Open, the window takes focus so the to-do field can type. */
    private fun openPanel(open: Boolean) {
        if (open == model.expanded || !::root.isInitialized) return
        snap?.cancel()
        if (open) {
            Peeks.clear()
            // where the bubble sits inside the panel-sized window, so the panel grows out of it
            val (_, sh) = screen()
            val (top, bottom) = bars()
            val m = dp(6)
            val rest = Prefs.restY.takeIf { it >= 0 } ?: (sh / 4)
            fun at(h: Int) = rest.coerceIn(top + m, (sh - bottom - h - m).coerceAtLeast(top + m))
            model.anchorY = at(dp(BALL_DP + 6)) - at(dp(PANEL_H_DP + 10))
            model.onRight = Prefs.onRight
        }
        model.peek = null
        model.expanded = open
        params.flags = if (open) OPEN_FLAGS else RESTING_FLAGS
        if (!open) {
            getSystemService(InputMethodManager::class.java).hideSoftInputFromWindow(root.windowToken, 0)
        }
        wm.updateViewLayout(root, params)
    }

    private fun openFromPeek() {
        val p = model.peek
        model.tab = when (p?.kind) {
            Peek.Kind.Task -> Tab.Todo
            Peek.Kind.Water -> Tab.Water
            Peek.Kind.Health -> Tab.Health
            else -> model.tab
        }
        openPanel(true)
    }

    /** Slides the island to the nearest side edge and remembers where it rests. */
    private fun snapToEdge() {
        val (sw, _) = screen()
        val w = root.width
        Prefs.onRight = params.x + w / 2 > sw / 2
        Prefs.restY = params.y
        val toX = if (Prefs.onRight) sw - w - dp(6) else dp(6)
        dragging = false
        snap = ValueAnimator.ofInt(params.x, toX).apply {
            duration = 220
            interpolator = DecelerateInterpolator()
            addUpdateListener {
                params.x = it.animatedValue as Int
                if (root.isAttachedToWindow) wm.updateViewLayout(root, params)
            }
            doOnEnd { place() }
            start()
        }
    }

    private fun ValueAnimator.doOnEnd(f: () -> Unit) = addListener(object : android.animation.AnimatorListenerAdapter() {
        override fun onAnimationEnd(a: android.animation.Animator) = f()
    })

    private fun screen(): Pair<Int, Int> {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val b = wm.currentWindowMetrics.bounds
            return b.width() to b.height()
        }
        val m = resources.displayMetrics
        return m.widthPixels to m.heightPixels
    }

    /** Status bar and navigation bar heights. */
    private fun bars(): Pair<Int, Int> {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val i = wm.currentWindowMetrics.windowInsets.getInsetsIgnoringVisibility(
                WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout(),
            )
            return i.top to i.bottom
        }
        return dp(24) to dp(48)
    }

    private fun dp(v: Int) = (v * resources.displayMetrics.density).toInt()

    // ------------------------------------------------------------ background loops

    private fun startLoops() {
        // live device stats: only while the Device tab is showing
        lifecycleScope.launch {
            while (true) {
                val need = model.awake && model.expanded && model.tab == Tab.Device
                if (need) {
                    val s = withContext(Dispatchers.IO) { stats.read() }
                    model.stats = s
                    model.cpuHistory.add((s.cpuLoad ?: 0).toFloat())
                    if (model.cpuHistory.size > 60) model.cpuHistory.removeAt(0)
                }
                // read every second while showing; otherwise check often so opening the tab is instant
                delay(if (need) 1_000 else 200)
            }
        }
        // peeks from reminders: show for a few seconds unless the panel is open
        lifecycleScope.launch {
            Peeks.peek.collect { p ->
                if (p == null) {
                    model.peek = null
                    return@collect
                }
                if (model.expanded || !model.awake) return@collect
                model.peek = p
                EyeEvents.play(Mood.Surprised)
                delay(7_000)
                if (Peeks.peek.value == p) Peeks.clear()
            }
        }
        // hidden: the window stays but draws nothing and lets every touch through
        lifecycleScope.launch {
            Prefs.bubbleHidden.state.collect { hidden ->
                if (hidden) openPanel(false)
                root.visibility = if (hidden) android.view.View.GONE else android.view.View.VISIBLE
                params.flags = if (hidden) RESTING_FLAGS or WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE else RESTING_FLAGS
                wm.updateViewLayout(root, params)
            }
        }
        // the notification card: rebuilt only when what it shows changes
        lifecycleScope.launch {
            combine(Store.data, Prefs.waterGoal.state, Prefs.eyeColor.state, Prefs.bubbleHidden.state, Prefs.theme.state) { d, _, _, _, _ ->
                IslandCard.content(d)
            }.distinctUntilChanged().collect { c ->
                getSystemService(NotificationManager::class.java).notify(IslandCard.ID, IslandCard.build(this@IslandService, c))
            }
        }
        // screen time and the 20-20-20 eye rest
        lifecycleScope.launch {
            while (true) {
                delay(60_000)
                if (!model.awake) continue
                flushScreenTime()
                val now = SystemClock.elapsedRealtime()
                if (Prefs.eyeRest.value && now - lastEyeRest >= 20 * 60_000L) {
                    lastEyeRest = now
                    Peeks.show(Peek("Rest your eyes", "Look 20 feet away for 20 seconds", Peek.Kind.Health))
                    Reminders.notify(this@IslandService, 3, "Rest your eyes", "Look 20 feet away for 20 seconds")
                }
            }
        }
    }

    private fun flushScreenTime() {
        if (!model.awake) return
        val now = SystemClock.elapsedRealtime()
        Store.addScreenTime(((now - screenFlushedAt) / 1000).toInt())
        screenFlushedAt = now
    }

    // ------------------------------------------------------------ steps

    private fun registerSteps() {
        val granted = Build.VERSION.SDK_INT < 29 ||
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACTIVITY_RECOGNITION) == PackageManager.PERMISSION_GRANTED
        val sm = getSystemService(SensorManager::class.java)
        val sensor = sm.getDefaultSensor(Sensor.TYPE_STEP_COUNTER)
        model.stepsReady = granted && sensor != null
        if (!model.stepsReady || stepSensor != null) return
        stepSensor = sensor
        // batched: the phone wakes us at most once a minute to hand over steps
        sm.registerListener(this, sensor, SensorManager.SENSOR_DELAY_NORMAL, 60_000_000)
    }

    override fun onSensorChanged(e: SensorEvent) {
        if (e.sensor.type == Sensor.TYPE_STEP_COUNTER) Store.stepCounter(e.values[0].toLong())
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}

    // ------------------------------------------------------------ misc

    private fun readBattery(i: Intent): Battery {
        val level = i.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
        val scale = i.getIntExtra(BatteryManager.EXTRA_SCALE, 100)
        val status = i.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
        return Battery(
            level = if (level < 0) 0 else level * 100 / scale,
            charging = status == BatteryManager.BATTERY_STATUS_CHARGING ||
                status == BatteryManager.BATTERY_STATUS_FULL,
        )
    }

    private fun startInForeground() {
        IslandCard.channel(this)
        ServiceCompat.startForeground(
            this, IslandCard.ID, IslandCard.build(this, IslandCard.content(Store.data.value)),
            if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE else 0,
        )
    }

    companion object {
        private const val BASE_FLAGS = WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL or
            WindowManager.LayoutParams.FLAG_WATCH_OUTSIDE_TOUCH or
            WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN or
            WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS
        private const val RESTING_FLAGS = BASE_FLAGS or WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
        private const val OPEN_FLAGS = BASE_FLAGS

        private const val ACTION_SHOW = "app.winklet.SHOW"
        private const val ACTION_HIDE = "app.winklet.HIDE"

        /** From the notification: bring the bubble back ([show]) or tuck it away. */
        fun toggleIntent(ctx: Context, show: Boolean): android.app.PendingIntent = android.app.PendingIntent.getService(
            ctx, if (show) 10 else 11,
            Intent(ctx, IslandService::class.java).setAction(if (show) ACTION_SHOW else ACTION_HIDE),
            android.app.PendingIntent.FLAG_IMMUTABLE or android.app.PendingIntent.FLAG_UPDATE_CURRENT,
        )

        private val _running = MutableStateFlow(false)
        val running: StateFlow<Boolean> = _running
    }
}
