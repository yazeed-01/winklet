//! How long the PC was on, per day, rebuilt from systemd's list of boots (first and last
//! journal entry of each), so the heatmap has history on day one. Without access to the
//! journal only the current boot is counted.

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use std::collections::BTreeMap;

/// Seconds the PC was on for each local day over the last `days` days.
pub fn pc_on_per_day(days: u32) -> BTreeMap<NaiveDate, u32> {
    let mut out = BTreeMap::new();
    let since = Local::now() - chrono::Duration::days(days as i64);
    let mut spans = boots();
    if spans.is_empty() {
        let up = std::fs::read_to_string("/proc/uptime").ok().and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok()).unwrap_or(0.0);
        spans.push((Local::now() - chrono::Duration::seconds(up as i64), Local::now()));
    }
    for (start, end) in spans {
        if end > since {
            add_span(&mut out, start.max(since), end);
        }
    }
    out
}

/// (first, last) journal entry of every boot the journal still holds, the current one ending now.
fn boots() -> Vec<(DateTime<Local>, DateTime<Local>)> {
    let Ok(o) = std::process::Command::new("journalctl").args(["--list-boots", "-o", "json", "--no-pager"]).output() else { return Vec::new() };
    if !o.status.success() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&o.stdout) else { return Vec::new() };
    let at = |us: Option<u64>| us.and_then(|us| Local.timestamp_micros(us as i64).single());
    let list = v.as_array().cloned().unwrap_or_default();
    let n = list.len();
    list.iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let first = at(b.get("first_entry").and_then(|x| x.as_u64()))?;
            let last = if i + 1 == n { Some(Local::now()) } else { at(b.get("last_entry").and_then(|x| x.as_u64())) }?;
            Some((first, last))
        })
        .collect()
}

/// Splits [start, end) across local midnights.
fn add_span(out: &mut BTreeMap<NaiveDate, u32>, start: DateTime<Local>, end: DateTime<Local>) {
    if end <= start || (end - start).num_hours() > 24 * 60 {
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
