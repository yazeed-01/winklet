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
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
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
 * The PC link: Winklet on the PC serves its Deck on the Wi-Fi behind a secret link, shown as
 * a QR code in Deck > Phone; this phone presses its keys and sends its notifications back.
 * The code carries a key after the `#`, which never goes over the network: every request and
 * reply is sealed with it (AES-256-GCM), so others on the Wi-Fi can't read or forge them.
 */
object PcDeck {
    private lateinit var sp: SharedPreferences
    private val _link = MutableStateFlow<String?>(null)
    /** "http://192.168.1.5:49200/<secret>", or null when not paired. */
    val link: StateFlow<String?> = _link
    private var key: ByteArray? = null
    private val rng = SecureRandom()

    // http://<IPv4>:<port>/<32 hex>#<64 hex key>, exactly what the PC puts in the QR code
    private val shape = Regex("""^http://(\d{1,3}(?:\.\d{1,3}){3}):(\d{2,5})/([0-9a-f]{32})#([0-9a-f]{64})$""")

    fun init(ctx: Context) {
        if (::sp.isInitialized) return
        sp = ctx.applicationContext.getSharedPreferences("pc_deck", Context.MODE_PRIVATE)
        key = sp.getString("key", null)?.let(::unhex)
        // a pairing from before the key (0.10 and older) can't talk to the PC any more
        if (key == null) sp.edit().remove("link").apply()
        _link.value = sp.getString("link", null)
    }

    /** Takes a scanned code; false if it isn't a Winklet link on a home or office network. */
    fun pair(scanned: String): Boolean {
        val m = shape.matchEntire(scanned.trim()) ?: return false
        if (!isLan(m.groupValues[1])) return false
        val base = scanned.trim().substringBefore('#')
        sp.edit().putString("link", base).putString("key", m.groupValues[4]).apply()
        key = unhex(m.groupValues[4])
        _link.value = base
        return true
    }

    fun forget() {
        sp.edit().remove("link").remove("key").apply()
        key = null
        _link.value = null
    }

    suspend fun load(): RemoteDeck = withContext(Dispatchers.IO) {
        val o = JSONObject(String(call("deck")))
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
        runCatching { call("icon", id) }.getOrNull()
            ?.let { BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }
    }

    /** Presses a key on the PC; false if the PC couldn't be reached. */
    suspend fun press(id: String): Boolean = withContext(Dispatchers.IO) {
        runCatching { call("press", id) }.isSuccess
    }

    /** Sends a phone event (a notification, a call, the battery) to the PC; false if it couldn't be reached. */
    suspend fun send(json: String): Boolean = withContext(Dispatchers.IO) {
        runCatching { call("phone", body = json) }.isSuccess
    }

    /** One sealed round trip: `op` with its argument, the PC's reply opened. Throws on any failure. */
    private fun call(op: String, arg: String = "", body: String? = null): ByteArray {
        val base = _link.value ?: error("not paired")
        val k = key ?: error("no key")
        val env = JSONObject().put("ts", System.currentTimeMillis() / 1000).put("op", op).put("arg", arg)
        if (body != null) env.put("body", body)
        val nonce = ByteArray(12).also(rng::nextBytes)
        val sealed = nonce + gcm(Cipher.ENCRYPT_MODE, k, nonce, REQ_AAD).doFinal(env.toString().toByteArray())
        val c = (URL("$base/x").openConnection() as HttpURLConnection).apply {
            connectTimeout = 2_500
            readTimeout = 4_000
            useCaches = false
            requestMethod = "POST"
            doOutput = true
            setRequestProperty("Content-Type", "application/octet-stream")
        }
        try {
            c.outputStream.use { it.write(sealed) }
            if (c.responseCode != 200) error("HTTP ${c.responseCode}")
            val reply = c.inputStream.use { it.readBytes() }
            if (reply.size < 12 + 16) error("short reply")
            // the reply is tied to this request's nonce, so an old one can't be slipped in
            return gcm(Cipher.DECRYPT_MODE, k, reply.copyOfRange(0, 12), REQ_RES + nonce).doFinal(reply, 12, reply.size - 12)
        } finally {
            c.disconnect()
        }
    }

    private fun gcm(mode: Int, k: ByteArray, nonce: ByteArray, aad: ByteArray): Cipher =
        Cipher.getInstance("AES/GCM/NoPadding").apply {
            init(mode, SecretKeySpec(k, "AES"), GCMParameterSpec(128, nonce))
            updateAAD(aad)
        }

    /** 10.x, 172.16-31.x, 192.168.x and link-local: the PC is on your own network. */
    private fun isLan(ip: String): Boolean {
        val p = ip.split('.').map { it.toIntOrNull() ?: return false }
        if (p.any { it !in 0..255 }) return false
        return p[0] == 10 || (p[0] == 172 && p[1] in 16..31) || (p[0] == 192 && p[1] == 168) || (p[0] == 169 && p[1] == 254)
    }

    private fun unhex(s: String): ByteArray? =
        if (s.length != 64) null else runCatching { ByteArray(32) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() } }.getOrNull()

    private val REQ_AAD = "winklet-req".toByteArray()
    private val REQ_RES = "winklet-res".toByteArray()

    private fun hex(s: String): Long =
        s.trimStart('#').toLongOrNull(16)?.let { 0xFF000000 or it } ?: 0xFF2B2B33
}
