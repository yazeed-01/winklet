package app.winklet

import android.content.Context
import android.content.SharedPreferences
import android.graphics.BitmapFactory
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/** One key on the PC's Deck, as Winklet on the PC sends it. */
data class RemoteKey(
    val id: String,
    val page: Int,
    val slot: Int,
    val title: String,
    /** Segoe Fluent code point, drawn with the same icon font as the PC */
    val glyph: String,
    val color: Long,
    val hasIcon: Boolean,
)

data class RemoteDeck(val version: Long, val pages: Int, val cols: Int, val rows: Int, val keys: List<RemoteKey>)

/**
 * The PC deck: Winklet on the PC serves its Deck on the Wi-Fi behind a secret link, shown as
 * a QR code in Deck > Phone. This keeps that link and talks to it.
 */
object PcDeck {
    private lateinit var sp: SharedPreferences
    private val _link = MutableStateFlow<String?>(null)
    /** "http://192.168.1.5:49200/<secret>", or null when not paired. */
    val link: StateFlow<String?> = _link

    // http://<IPv4>:<port>/<32 hex>, exactly what the PC puts in the QR code
    private val shape = Regex("^http://(\\d{1,3}(\\.\\d{1,3}){3}):(\\d{2,5})/([0-9a-f]{32})$")

    fun init(ctx: Context) {
        if (::sp.isInitialized) return
        sp = ctx.applicationContext.getSharedPreferences("pc_deck", Context.MODE_PRIVATE)
        _link.value = sp.getString("link", null)
    }

    /** Takes a scanned code; false if it isn't a Winklet deck link. */
    fun pair(scanned: String): Boolean {
        val link = scanned.trim().trimEnd('/')
        if (!shape.matches(link)) return false
        sp.edit().putString("link", link).apply()
        _link.value = link
        return true
    }

    fun forget() {
        sp.edit().remove("link").apply()
        _link.value = null
    }

    suspend fun load(): RemoteDeck = withContext(Dispatchers.IO) {
        val o = JSONObject(String(get("deck")))
        val arr = o.getJSONArray("keys")
        RemoteDeck(
            version = o.optLong("version"),
            pages = o.optInt("pages", 1).coerceAtLeast(1),
            cols = o.optInt("cols", 8),
            rows = o.optInt("rows", 3),
            keys = List(arr.length()) { i ->
                val k = arr.getJSONObject(i)
                RemoteKey(
                    k.getString("id"), k.optInt("page"), k.optInt("slot"),
                    k.optString("title"), k.optString("glyph"),
                    hex(k.optString("color")), k.optBoolean("icon"),
                )
            },
        )
    }

    suspend fun icon(id: String): ImageBitmap? = withContext(Dispatchers.IO) {
        runCatching { get("icon/$id") }.getOrNull()
            ?.let { BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }
    }

    /** Presses a key on the PC; false if the PC couldn't be reached. */
    suspend fun press(id: String): Boolean = withContext(Dispatchers.IO) {
        runCatching {
            val c = open("press/$id")
            c.requestMethod = "POST"
            c.doOutput = true
            c.outputStream.use { }
            c.responseCode == 200
        }.getOrDefault(false)
    }

    /** Sends a phone event (a notification, the battery) to the PC; false if it couldn't be reached. */
    suspend fun send(json: String): Boolean = withContext(Dispatchers.IO) {
        runCatching {
            val c = open("phone")
            try {
                c.requestMethod = "POST"
                c.doOutput = true
                c.setRequestProperty("Content-Type", "application/json; charset=utf-8")
                c.outputStream.use { it.write(json.toByteArray()) }
                c.responseCode == 200
            } finally {
                c.disconnect()
            }
        }.getOrDefault(false)
    }

    private fun open(path: String): HttpURLConnection {
        val base = _link.value ?: error("not paired")
        return (URL("$base/$path").openConnection() as HttpURLConnection).apply {
            connectTimeout = 2_500
            readTimeout = 4_000
            useCaches = false
        }
    }

    private fun get(path: String): ByteArray {
        val c = open(path)
        try {
            if (c.responseCode != 200) error("HTTP ${c.responseCode}")
            return c.inputStream.use { it.readBytes() }
        } finally {
            c.disconnect()
        }
    }

    private fun hex(s: String): Long =
        s.trimStart('#').toLongOrNull(16)?.let { 0xFF000000 or it } ?: 0xFF2B2B33
}
