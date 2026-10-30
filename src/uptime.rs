//! How long the PC was on, per day, rebuilt from Windows' own System event log
//! (boot/shutdown/sleep/wake events), so the heatmap has months of history on day one.

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use std::collections::BTreeMap;
use windows::Win32::System::EventLog::{
    EVT_HANDLE, EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtRender, EvtRenderEventXml,
};
use windows::core::{HSTRING, w};

/// Power-state changes we care about: (provider, event id, turns the PC on?)
const EVENTS: [(&str, u32, bool); 8] = [
    ("Microsoft-Windows-Kernel-General", 12, true),   // OS started
    ("Microsoft-Windows-Kernel-General", 13, false),  // OS shut down
    ("Microsoft-Windows-Kernel-Power", 42, false),    // entering sleep
    ("Microsoft-Windows-Kernel-Power", 109, false),   // shutdown transition
    ("Microsoft-Windows-Kernel-Power", 506, false),   // modern standby entered
    ("Microsoft-Windows-Kernel-Power", 507, true),    // modern standby exited
    ("Microsoft-Windows-Power-Troubleshooter", 1, true), // woke from sleep
    ("Microsoft-Windows-Kernel-Boot", 27, true),      // boot type (fallback for start)
];

/// Seconds the PC was on for each local day over the last `days` days.
pub fn pc_on_per_day(days: u32) -> BTreeMap<NaiveDate, u32> {
    let mut events = read_events(days);
    events.sort_by_key(|e| e.0);
    let mut out = BTreeMap::new();
    let mut on_since: Option<DateTime<Local>> = None;
    for (t, on) in events {
        match (on, on_since) {
            (true, None) => on_since = Some(t),
            (false, Some(start)) => {
                add_span(&mut out, start, t);
                on_since = None;
            }
            // two "on" events in a row: the previous session ended without a clean
            // shutdown (crash, power loss). We don't know when, so it isn't counted.
            (true, Some(_)) => on_since = Some(t),
            (false, None) => {}
        }
    }
    // still on right now
    add_span(&mut out, on_since.unwrap_or_else(Local::now), Local::now());
    out
}

/// Splits [start, end) across local midnights.
fn add_span(out: &mut BTreeMap<NaiveDate, u32>, start: DateTime<Local>, end: DateTime<Local>) {
    if end <= start || (end - start).num_hours() > 24 * 14 {
        return; // clock jumps / garbage
    }
    let mut cur = start;
    while cur < end {
        let next_midnight = cur
            .date_naive()
            .succ_opt()
            .and_then(|d| Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest())
            .unwrap_or(end);
        let stop = end.min(next_midnight);
        *out.entry(cur.date_naive()).or_default() += (stop - cur).num_seconds().max(0) as u32;
        cur = stop;
    }
}

fn read_events(days: u32) -> Vec<(DateTime<Local>, bool)> {
    let ms = days as u64 * 86_400_000;
    let ors: Vec<String> = EVENTS
        .iter()
        .map(|(p, id, _)| format!("(Provider[@Name='{p}'] and EventID={id})"))
        .collect();
    let query = HSTRING::from(format!(
        "*[System[({}) and TimeCreated[timediff(@SystemTime) <= {ms}]]]",
        ors.join(" or ")
    ));
    let mut out = Vec::new();
    unsafe {
        let Ok(results) = EvtQuery(None, w!("System"), &query, EvtQueryChannelPath.0 as u32) else {
            return out;
        };
        let mut buf: Vec<u16> = vec![0; 4096];
        loop {
            let mut handles = [0isize; 64];
            let mut got = 0u32;
            if EvtNext(results, &mut handles, 1000, 0, &mut got).is_err() || got == 0 {
                break;
            }
            for &h in &handles[..got as usize] {
                let ev = EVT_HANDLE(h);
                let (mut used, mut props) = (0u32, 0u32);
                let size = (buf.len() * 2) as u32;
                if EvtRender(None, ev, EvtRenderEventXml.0 as u32, size, Some(buf.as_mut_ptr() as *mut _), &mut used, &mut props)
                    .is_ok()
                {
                    let xml = String::from_utf16_lossy(&buf[..(used as usize / 2).min(buf.len())]);
                    if let Some(e) = parse(&xml) {
                        out.push(e);
                    }
                }
                let _ = EvtClose(ev);
            }
        }
        let _ = EvtClose(results);
    }
    out
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

fn parse(xml: &str) -> Option<(DateTime<Local>, bool)> {
    let provider = between(xml, "Provider Name='", "'")?;
    let id: u32 = between(xml, "<EventID", "</EventID>")?.rsplit('>').next()?.trim().parse().ok()?;
    let time = between(xml, "SystemTime='", "'")?;
    let t = DateTime::parse_from_rfc3339(time).ok()?.with_timezone(&Local);
    let on = EVENTS.iter().find(|(p, i, _)| *p == provider && *i == id)?.2;
    Some((t, on))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_event_xml() {
        let xml = "<Event><System><Provider Name='Microsoft-Windows-Kernel-General' Guid='{x}'/><EventID>12</EventID><TimeCreated SystemTime='2026-10-29T08:00:00.5Z'/></System></Event>";
        let (_, on) = parse(xml).unwrap();
        assert!(on);
    }

    #[test]
    #[ignore = "reads this machine's event log"]
    fn real_event_log() {
        let m = pc_on_per_day(200);
        for (d, s) in m.iter().rev().take(14) {
            println!("{d} {:.1}h", *s as f32 / 3600.0);
        }
        println!("days with data: {}", m.len());
    }

    #[test]
    fn splits_spans_at_midnight() {
        let mut m = BTreeMap::new();
        let a = Local.with_ymd_and_hms(2026, 1, 1, 23, 0, 0).unwrap();
        let b = Local.with_ymd_and_hms(2026, 1, 2, 1, 0, 0).unwrap();
        add_span(&mut m, a, b);
        assert_eq!(m.values().sum::<u32>(), 7200);
        assert_eq!(m.len(), 2);
    }
}
