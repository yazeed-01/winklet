package app.winklet

import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.display.DisplayManager
import android.net.TrafficStats
import android.os.BatteryManager
import android.os.Build
import android.os.Environment
import android.os.PowerManager
import android.os.StatFs
import android.os.SystemClock
import android.view.Display
import java.io.File

/** One reading of the phone's state. Null = this phone doesn't let apps read it. */
data class SysStats(
    val cpuLoad: Int? = null,
    /** True when [cpuLoad] is estimated from core clocks because /proc/stat is closed. */
    val cpuEstimated: Boolean = false,
    val cpuMhz: Int? = null,
    val cpuMaxMhz: Int? = null,
    val cores: Int = Runtime.getRuntime().availableProcessors(),
    val cpuTemp: Float? = null,
    val gpuTemp: Float? = null,
    val gpuLoad: Int? = null,
    val ramUsed: Long = 0,
    val ramTotal: Long = 0,
    val storageUsed: Long = 0,
    val storageTotal: Long = 0,
    val batteryLevel: Int = 0,
    val batteryTemp: Float? = null,
    val charging: Boolean = false,
    /** Battery current in mA; negative while discharging. */
    val batteryMa: Int? = null,
    val batteryVolts: Float? = null,
    val downBps: Long = 0,
    val upBps: Long = 0,
    val refreshHz: Int = 60,
    val thermal: Int = 0,
    val uptimeMs: Long = 0,
) {
    val ramPct get() = if (ramTotal > 0) (ramUsed * 100 / ramTotal).toInt() else 0
    val storagePct get() = if (storageTotal > 0) (storageUsed * 100 / storageTotal).toInt() else 0
    val thermalName get() = when (thermal) {
        0 -> "Cool"; 1 -> "Light"; 2 -> "Moderate"; 3 -> "Severe"; 4 -> "Critical"; 5 -> "Emergency"; else -> "Shutdown"
    }
}

/**
 * Reads CPU, GPU, RAM, storage, battery and network. Created once and sampled about once a
 * second, only while the Device tab is showing.
 */
class StatsReader(private val ctx: Context) {
    private val am = ctx.getSystemService(ActivityManager::class.java)
    private val pm = ctx.getSystemService(PowerManager::class.java)
    private val bm = ctx.getSystemService(BatteryManager::class.java)
    private val display = ctx.getSystemService(DisplayManager::class.java).getDisplay(Display.DEFAULT_DISPLAY)

    private var lastIdle = -1L
    private var lastTotal = -1L
    private var statOpen = true
    private var lastRx = TrafficStats.getTotalRxBytes()
    private var lastTx = TrafficStats.getTotalTxBytes()
    private var lastAt = SystemClock.elapsedRealtime()

    private val coreDirs = (0 until 16).map { File("/sys/devices/system/cpu/cpu$it/cpufreq") }.filter { it.exists() }
    private val coreMax = coreDirs.map { readLong(File(it, "cpuinfo_max_freq")) ?: 0L }

    // thermal zones by name: cpu*/cpuss*, gpu*/gpuss*
    private val zones: List<Pair<String, File>> = File("/sys/class/thermal").listFiles()
        ?.filter { it.name.startsWith("thermal_zone") }
        ?.mapNotNull { z -> File(z, "type").readTextOrNull()?.trim()?.lowercase()?.let { it to File(z, "temp") } }
        ?.filter { (_, f) -> f.canRead() }
        ?: emptyList()
    private val cpuZones = zones.filter { it.first.startsWith("cpu") }.map { it.second }
    private val gpuZones = zones.filter { it.first.startsWith("gpu") }.map { it.second }

    private val gpuLoadFiles = listOf(
        "/sys/class/kgsl/kgsl-3d0/gpu_busy_percentage",
        "/sys/class/kgsl/kgsl-3d0/devfreq/gpu_load",
        "/sys/class/misc/mali0/device/utilization",
        "/sys/kernel/gpu/gpu_busy",
    ).map(::File).filter { it.canRead() }

    fun read(): SysStats {
        val mem = ActivityManager.MemoryInfo().also { am.getMemoryInfo(it) }
        val fs = StatFs(Environment.getDataDirectory().path)
        val battery = ctx.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))

        val now = SystemClock.elapsedRealtime()
        val secs = ((now - lastAt) / 1000f).coerceAtLeast(0.2f)
        val rx = TrafficStats.getTotalRxBytes()
        val tx = TrafficStats.getTotalTxBytes()
        val down = ((rx - lastRx) / secs).toLong().coerceAtLeast(0)
        val up = ((tx - lastTx) / secs).toLong().coerceAtLeast(0)
        lastRx = rx; lastTx = tx; lastAt = now

        val freqs = coreDirs.map { readLong(File(it, "scaling_cur_freq")) ?: 0L }
        val cpuStat = if (statOpen) procStatLoad() else null
        val freqLoad = if (coreMax.isNotEmpty() && coreMax.all { it > 0 }) {
            (freqs.zip(coreMax).sumOf { (f, m) -> f.toDouble() / m } / freqs.size * 100).toInt()
        } else null

        val level = battery?.getIntExtra(BatteryManager.EXTRA_LEVEL, 0) ?: 0
        val scale = battery?.getIntExtra(BatteryManager.EXTRA_SCALE, 100) ?: 100
        val status = battery?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
        val microAmps = bm.getIntProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW)

        return SysStats(
            cpuLoad = cpuStat ?: freqLoad,
            cpuEstimated = cpuStat == null,
            cpuMhz = freqs.maxOrNull()?.let { (it / 1000).toInt() },
            cpuMaxMhz = coreMax.maxOrNull()?.let { (it / 1000).toInt() },
            cpuTemp = cpuZones.mapNotNull { readTemp(it) }.maxOrNull(),
            gpuTemp = gpuZones.mapNotNull { readTemp(it) }.maxOrNull(),
            gpuLoad = gpuLoadFiles.firstNotNullOfOrNull { f -> f.readTextOrNull()?.trim()?.split(' ', '%')?.firstOrNull()?.toIntOrNull() },
            ramUsed = mem.totalMem - mem.availMem,
            ramTotal = mem.totalMem,
            storageUsed = fs.totalBytes - fs.availableBytes,
            storageTotal = fs.totalBytes,
            batteryLevel = if (scale > 0) level * 100 / scale else level,
            batteryTemp = battery?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, Int.MIN_VALUE)
                ?.takeIf { it != Int.MIN_VALUE }?.let { it / 10f },
            charging = status == BatteryManager.BATTERY_STATUS_CHARGING || status == BatteryManager.BATTERY_STATUS_FULL,
            // some phones report µA, some mA
            batteryMa = microAmps.takeIf { it != Int.MIN_VALUE && it != 0 }
                ?.let { if (kotlin.math.abs(it) > 20_000) it / 1000 else it },
            batteryVolts = battery?.getIntExtra(BatteryManager.EXTRA_VOLTAGE, 0)?.takeIf { it > 0 }?.let { it / 1000f },
            downBps = down,
            upBps = up,
            refreshHz = display?.refreshRate?.toInt() ?: 60,
            thermal = if (Build.VERSION.SDK_INT >= 29) pm.currentThermalStatus else 0,
            uptimeMs = SystemClock.elapsedRealtime(),
        )
    }

    /** Busy share of all cores since the last reading, from /proc/stat. */
    private fun procStatLoad(): Int? {
        val line = runCatching { File("/proc/stat").bufferedReader().use { it.readLine() } }.getOrNull()
        if (line == null || !line.startsWith("cpu ")) {
            statOpen = false
            return null
        }
        val v = line.split(Regex("\\s+")).drop(1).mapNotNull { it.toLongOrNull() }
        if (v.size < 5) return null
        val idle = v[3] + v[4]
        val total = v.sum()
        val load = if (lastTotal >= 0 && total > lastTotal) {
            (100 * (1 - (idle - lastIdle).toDouble() / (total - lastTotal))).toInt().coerceIn(0, 100)
        } else null
        lastIdle = idle
        lastTotal = total
        return load
    }

    private fun readTemp(f: File): Float? = readLong(f)?.let { if (it > 1000) it / 1000f else it.toFloat() }?.takeIf { it in 1f..150f }

    private fun readLong(f: File): Long? = f.readTextOrNull()?.trim()?.toLongOrNull()

    private fun File.readTextOrNull(): String? = runCatching { readText() }.getOrNull()
}
