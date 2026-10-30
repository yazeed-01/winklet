//! Volume, mute and the microphone test.

use crate::Bridge;
use crate::app::Shared;
use slint::{ComponentHandle, Model, TimerMode};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_master_set, |a, v| {
        if let Some(au) = &a.audio {
            au.set_master(v);
        }
        let ui = a.ui();
        ui.global::<Bridge>().set_master_volume(v);
        if v > 0.0 {
            ui.global::<Bridge>().set_master_muted(false);
        }
    });
    on!(b, app, on_master_mute_toggle, |a| {
        if let Some(au) = &a.audio {
            au.toggle_master_mute();
        }
        a.refresh_audio();
    });
    on!(b, app, on_app_volume_set, |a, name, v| {
        if let Some(au) = &a.audio {
            au.set_app_volume(&name, v);
        }
        for i in 0..a.app_volumes.row_count() {
            if let Some(mut row) = a.app_volumes.row_data(i) {
                if row.name == name {
                    row.volume = v;
                    row.muted = row.muted && v == 0.0;
                    a.app_volumes.set_row_data(i, row);
                }
            }
        }
    });
    on!(b, app, on_app_mute_toggle, |a, name| {
        if let Some(au) = &a.audio {
            au.toggle_app_mute(&name);
        }
        a.refresh_audio();
    });
    on!(b, app, on_mic_toggle, |a| { a.toggle_mic(); });
    on!(b, app, on_mic_list, |a| { a.refresh_mics(); });
    on!(b, app, on_mic_select, |a, id| { a.select_mic(&id); });
    {
        let shared = app.clone();
        b.on_mic_test(move || {
            let mut a = shared.borrow_mut();
            if a.mic_test.is_some() {
                a.stop_mic_test();
                return;
            }
            let Some(test) = a.audio.as_ref().and_then(|au| au.start_mic_test()) else {
                a.toast("Microphone unavailable");
                return;
            };
            a.mic_test = Some(test);
            a.mic_test_started = Instant::now();
            a.ui().global::<Bridge>().set_mic_testing(true);
            let weak = Rc::downgrade(&shared);
            a.mic_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
                let Some(s) = weak.upgrade() else { return };
                let mut a = s.borrow_mut();
                if a.mic_test_started.elapsed() > Duration::from_secs(20) {
                    a.stop_mic_test();
                    return;
                }
                let level = a.mic_test.as_mut().map(|t| t.level()).unwrap_or(0.0);
                // decibel scale: whisper ≈ 0.3, normal speech ≈ 0.6–0.8, shouting = full
                let level = if level > 0.0 { ((20.0 * level.log10() + 50.0) / 50.0).clamp(0.0, 1.0) } else { 0.0 };
                let silent = a.mic_test.as_ref().is_some_and(|t| !t.heard_anything);
                if silent && a.mic_test_started.elapsed() > Duration::from_secs(3) && a.mic_test_started.elapsed() < Duration::from_millis(3100) {
                    a.toast("No sound: check Settings > Privacy > Microphone");
                }
                let ui = a.ui();
                let b = ui.global::<Bridge>();
                // fast attack, slow release, like a real VU meter
                let shown = level.max(b.get_mic_level() * 0.82);
                b.set_mic_level(shown);
            });
        });
    }
}
