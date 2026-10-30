//! GPU load and dedicated video memory, from the same performance counters
//! Task Manager uses ("GPU Engine" and "GPU Adapter Memory"). No admin needed.
//!
//! The query is opened the first time the Dash asks and kept open, because
//! utilization is a rate: each sample compares against the previous one.

use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::System::Performance::{
    PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER,
    PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};
use windows::core::{PCWSTR, w};

/// don't clamp per-engine values at 100 before we sum them
const PDH_FMT_NOCAP100: u32 = 0x8000;

struct Query {
    query: PDH_HQUERY,
    engine: PDH_HCOUNTER,
    memory: PDH_HCOUNTER,
}

thread_local! {
    // None inside = the counters aren't available on this PC; don't try again.
    static QUERY: RefCell<Option<Option<Query>>> = const { RefCell::new(None) };
}

fn open() -> Option<Query> {
    unsafe {
        let mut query = PDH_HQUERY::default();
        if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
            return None;
        }
        let mut engine = PDH_HCOUNTER::default();
        let mut memory = PDH_HCOUNTER::default();
        if PdhAddEnglishCounterW(query, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut engine) != 0 {
            return None;
        }
        // optional: older drivers don't publish it
        let _ = PdhAddEnglishCounterW(query, w!("\\GPU Adapter Memory(*)\\Dedicated Usage"), 0, &mut memory);
        PdhCollectQueryData(query);
        Some(Query { query, engine, memory })
    }
}

/// Every instance of a wildcard counter as (instance name, value).
fn read_array(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    if counter.is_invalid() {
        return Vec::new();
    }
    let fmt = PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100);
    unsafe {
        let (mut size, mut count) = (0u32, 0u32);
        if PdhGetFormattedCounterArrayW(counter, fmt, &mut size, &mut count, None) != PDH_MORE_DATA || size == 0 {
            return Vec::new();
        }
        // items followed by their name strings, all in one buffer (u64 keeps it aligned)
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        if PdhGetFormattedCounterArrayW(counter, fmt, &mut size, &mut count, Some(items)) != 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(items, count as usize)
            .iter()
            .filter(|i| matches!(i.FmtValue.CStatus, PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA))
            .map(|i| (i.szName.to_string().unwrap_or_default(), i.FmtValue.Anonymous.doubleValue))
            .collect()
    }
}

/// (GPU load 0..1, dedicated video memory in use in GB). (0, 0) when unavailable.
///
/// Like Task Manager: add up every process's use of each engine, then take the
/// busiest engine (3D, video decode, copy, …).
pub fn usage() -> (f32, f64) {
    QUERY.with(|q| {
        let mut q = q.borrow_mut();
        let Some(query) = q.get_or_insert_with(open) else { return (0.0, 0.0) };
        if unsafe { PdhCollectQueryData(query.query) } != 0 {
            return (0.0, 0.0);
        }
        // instance names look like "pid_1234_luid_0x…_0x…_phys_0_eng_3_engtype_3D"
        let mut engines: HashMap<String, f64> = HashMap::new();
        for (name, v) in read_array(query.engine) {
            let key = name.split_once("_luid_").map(|(_, k)| k.to_string()).unwrap_or(name);
            *engines.entry(key).or_default() += v;
        }
        let load = engines.values().copied().fold(0.0, f64::max);
        let vram: f64 = read_array(query.memory).iter().map(|(_, b)| b).sum();
        ((load / 100.0).clamp(0.0, 1.0) as f32, vram / 1024f64.powi(3))
    })
}
