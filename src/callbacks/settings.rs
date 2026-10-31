//! Settings toggles and the car show options.

use crate::app::{App, Shared, defer};
use crate::{Bridge, sys};
use slint::ComponentHandle;

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_set_setting, |a, key, value| {
        match key.as_str() {
            "autostart" => {
                sys::set_autostart(value);
                a.data.settings.autostart = sys::autostart_enabled();
            }
            "clock24" => {
                a.data.settings.clock_24h = value;
                a.refresh_clock();
            }
            "fullscreen" => a.data.settings.hide_fullscreen = value,
            "water" => a.data.settings.water_reminders = value,
            "car" => a.data.settings.car_show = value,
            "homestart" => a.data.settings.island_home_on_start = value,
            "stand" => a.data.settings.stand_reminders = value,
            "eyerest" => a.data.settings.eye_rest = value,
            "waitpause" => a.data.settings.wait_for_pause = value,
            "pausecalls" => a.data.settings.pause_in_calls = value,
            "workhours" => a.data.settings.work_hours_only = value,
            "summary" => a.data.settings.summary_on = value,
            "gamemode" => a.set_game_mode(value, false),
            "gameauto" => a.data.settings.game_auto = value,
            "gamepower" => {
                a.data.settings.game_power = value;
                if a.data.settings.game_mode && value && !a.play.power_set {
                    a.play.power_set = sys::set_power_mode(sys::PowerMode::Performance);
                }
            }
            "gamebreak" => a.data.settings.game_break = value,
            "gearpower" => {
                a.data.settings.gear_power = value;
                a.refresh_gear();
            }
            "eyes" => {
                a.data.settings.eyes_lively = value;
                if !value {
                    a.ui().global::<Bridge>().set_eye_mood(0);
                    a.eye_end = None;
                }
            }
            _ => {}
        }
        a.data.save();
        a.refresh_settings();
        a.refresh_game();
        a.refresh_summary_setting();
    });
    on!(b, app, on_water_interval_adjust, |a, d| {
        a.data.settings.water_interval_min = (a.data.settings.water_interval_min + d * 15).clamp(15, 240);
        a.data.save();
        a.refresh_settings();
    });

    on!(b, app, on_set_car_model, |a, m| {
        a.data.settings.car_model = m.clamp(0, 3);
        a.data.save();
        a.refresh_settings();
    });
    on!(b, app, on_island_home, |a| { a.island_home(); });
    b.on_data_open(|| {
        let dir = crate::store::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        sys::open(&dir.to_string_lossy(), "");
    });
    on!(b, app, on_data_reset, |a| { a.reset_all_data(); });
    on!(b, app, on_set_theme, |a, t| {
        a.data.settings.theme = t.max(0);
        a.data.save();
        a.ui().global::<Bridge>().set_theme(t.max(0));
    });
    on!(b, app, on_ui_scale_set, |a, v| {
        a.data.settings.ui_scale = ((v * 20.0).round() / 20.0).clamp(0.8, 1.4);
        a.data.save();
        a.refresh_settings();
        a.place();
    });
    on!(b, app, on_island_alpha_set, |a, v| {
        a.data.settings.island_alpha = ((v * 20.0).round() / 20.0).clamp(0.4, 1.0);
        a.data.save();
        a.refresh_settings();
    });
    // collapses the panel it was clicked in: let the click finish first
    b.on_island_move(|| defer(App::start_move));
    b.on_car_preview(|| {
        defer(|s| {
            s.borrow_mut().stop_car_show();
            App::start_car_show(s);
        });
    });
}
