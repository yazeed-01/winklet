package app.winklet

import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId

data class ParsedTask(val text: String, val day: LocalDate?, val due: Long?)

/**
 * Parses "Buy milk @18:30" / "Deploy @2026-11-02 09:00" / "Gym @tomorrow 7am", as on the
 * desktop. A time without a day means today, or tomorrow if that time already passed.
 */
fun parseTask(input: String, defaultDay: LocalDate? = null): ParsedTask? {
    val trimmed = input.trim()
    if (trimmed.isEmpty()) return null
    val at = trimmed.lastIndexOf('@')
    if (at < 0) return ParsedTask(trimmed, defaultDay, null)

    val text = trimmed.substring(0, at).trim().ifEmpty { trimmed }
    val today = LocalDate.now()
    var day = defaultDay
    var time: LocalTime? = null
    for (part in trimmed.substring(at + 1).lowercase().split(Regex("\\s+"))) {
        when {
            part.isEmpty() -> {}
            part == "today" -> day = today
            part == "tomorrow" || part == "tmr" -> day = today.plusDays(1)
            else -> runCatching { LocalDate.parse(part) }.getOrNull()?.let { day = it }
                ?: parseTime(part)?.let { time = it }
        }
    }
    val t = time
    if (t != null && day == null) {
        day = if (LocalDateTime.of(today, t) > LocalDateTime.now()) today else today.plusDays(1)
    }
    val due = if (t != null) LocalDateTime.of(day, t).atZone(ZoneId.systemDefault()).toInstant().toEpochMilli() else null
    return ParsedTask(text, day, due)
}

fun parseTime(raw: String): LocalTime? {
    var s = raw.trim().lowercase()
    val pm = s.endsWith("pm")
    val am = s.endsWith("am")
    if (pm || am) s = s.dropLast(2).trim()
    val (h0, m) = s.split(':').let { p ->
        when (p.size) {
            1 -> (p[0].toIntOrNull() ?: return null) to 0
            2 -> (p[0].toIntOrNull() ?: return null) to (p[1].toIntOrNull() ?: return null)
            else -> return null
        }
    }
    val h = when {
        pm && h0 < 12 -> h0 + 12
        am && h0 == 12 -> 0
        else -> h0
    }
    if (h !in 0..23 || m !in 0..59) return null
    return LocalTime.of(h, m)
}
