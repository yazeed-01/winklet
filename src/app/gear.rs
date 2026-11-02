//! Drive modes: a 6-speed gear shifter (plus N and R) that decides which
//! peeks get through, can start the focus timer, mute the mic and switch
//! the Windows power mode. 0 = N (away), 1 rest … 6 redline (deep work).

use super::{App, TAB_GEAR, s, trace};
use crate::{Bar, Bridge, sys};
use chrono::Local;
use slint::{ComponentHandle, ModelRc, VecModel};

pub const NEUTRAL: i32 = 0;
pub const TOP_GEAR: i32 = 6;

const NAMES: [&str; 7] = ["Neutral", "Rest", "Cruise", "Normal", "Busy", "Focus", "Redline"];

const RULES: [&[&str]; 7] = [
    &["You're away: all reminders wait", "Only alarms get through", "Sitting timer keeps counting breaks"],
    &["Every reminder is on", "Good time for a stretch or water", "Unwind before the next sprint"],
    &["Every reminder is on", "For light work, chat and browsing"],
    &["Winklet's normal behavior"],
    &["Music and charger popups hidden", "Eye-rest reminders wait", "Water and stand-up still remind you"],
    &["Starts the focus timer", "Water, stand-up and eye-rest wait", "Alarms, tasks and timer still show"],
    &["Mic muted until you shift down", "Only alarms get through", "Everything else waits for you"],
];

/// Peeks the user triggered themselves (or can't miss) always show.
fn always_shown(kind: i32) -> bool {
    matches!(kind, 5 | 8 | 9 | 12 | 14 | 15 | 18)
}

/// Whether a peek of `kind` may show in `gear` (see the peek-kind list in bridge.slint).
pub fn allows(gear: i32, kind: i32) -> bool {
    if always_shown(kind) {
        return true;
    }
    match gear {
        1..=3 => true,
        // music, charger, eye rest, phone notifications (sign-in codes still come through), little lines
        4 => !matches!(kind, 1 | 2 | 7 | 13 | 19 | 21),
        // + water and stand-up
        5 => !matches!(kind, 1 | 2 | 3 | 7 | 11 | 13 | 19 | 21),
        _ => false,
    }
}

/// Windows power mode for a gear: efficiency for N/1/2, balanced for 3/4, performance for 5/6.
pub(super) fn power_for(gear: i32) -> sys::PowerMode {
    match gear {
        5 | 6 => sys::PowerMode::Performance,
        3 | 4 => sys::PowerMode::Balanced,
        _ => sys::PowerMode::Efficiency,
    }
}

pub fn label(gear: i32) -> String {
    if gear == NEUTRAL { "N".into() } else { gear.to_string() }
}

impl App {
    pub fn gear(&self) -> i32 {
        self.data.settings.gear
    }

    pub fn gear_allows(&self, kind: i32) -> bool {
        allows(self.gear(), kind)
    }

    pub fn shift_gear(&mut self, to: i32) {
        let to = to.clamp(NEUTRAL, TOP_GEAR);
        let from = self.gear();
        trace(&format!("gear {from} -> {to}"));
        if to == from {
            return;
        }
        self.prev_gear = from;
        self.data.settings.gear = to;
        self.data.save();

        // Redline mutes the mic; shifting out gives it back (only if we were the ones who muted it).
        if let Some(audio) = &self.audio {
            if to == TOP_GEAR && audio.mic_muted() == Some(false) {
                audio.set_mic_muted(true);
                self.gear_muted_mic = true;
            } else if from == TOP_GEAR && self.gear_muted_mic {
                if audio.mic_muted() == Some(true) {
                    audio.set_mic_muted(false);
                }
                self.gear_muted_mic = false;
            }
        }
        self.refresh_mic();

        if to == 5 && !self.focus.running && !self.focus.is_break {
            self.focus.running = true;
            self.refresh_focus();
        }

        if self.data.settings.gear_power && power_for(to) != power_for(from) && !sys::set_power_mode(power_for(to)) {
            self.toast("Couldn't change power mode");
        }

        match to {
            1 => self.eye_event(4, 3.0),
            6 => self.eye_event(6, 3.0),
            NEUTRAL => self.eye_event(9, 3.0),
            _ => {}
        }
        self.refresh_gear();
        let sub = RULES[to as usize][0];
        self.peek(15, &format!("Gear {} · {}", label(to), NAMES[to as usize]), sub, 2);
    }

    pub fn shift_up(&mut self) {
        self.shift_gear((self.gear() + 1).min(TOP_GEAR));
    }

    pub fn shift_down(&mut self) {
        self.shift_gear((self.gear() - 1).max(NEUTRAL));
    }

    /// R: back to the gear you were in before the last shift.
    pub fn gear_reverse(&mut self) {
        let back = self.prev_gear;
        if back != self.gear() {
            self.shift_gear(back);
        }
    }

    /// Counts active time per gear (called from the wellbeing tick with the seconds just added).
    pub fn gear_count(&mut self, secs: u32) {
        let g = self.gear() as usize;
        let day = self.data.gear_time.entry(Local::now().date_naive()).or_default();
        day.resize(7, 0);
        day[g] += secs;
    }

    pub fn refresh_gear(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let g = self.gear();
        b.set_gear(g);
        b.set_gear_name(s(NAMES[g as usize]));
        let rules: Vec<_> = RULES[g as usize].iter().map(|r| s(*r)).collect();
        b.set_gear_rules(ModelRc::new(VecModel::from(rules)));
        b.set_gear_power(self.data.settings.gear_power);
        b.set_gear_prev(self.prev_gear);
        if b.get_expanded() && b.get_tab() == TAB_GEAR {
            self.refresh_gear_time();
        }
    }

    /// Today's active time in each gear, as bars N, 1 … 6.
    pub fn refresh_gear_time(&self) {
        let mut today = self.data.gear_time.get(&Local::now().date_naive()).cloned().unwrap_or_default();
        today.resize(7, 0);
        let max = today.iter().copied().max().unwrap_or(0).max(60);
        let g = self.gear() as usize;
        let bars: Vec<Bar> = today
            .iter()
            .enumerate()
            .map(|(i, &secs)| {
                let m = secs / 60;
                let text = if m >= 60 { format!("{}h{:02}", m / 60, m % 60) } else { format!("{m}m") };
                Bar { label: s(text), value: secs as f32 / max as f32, today: i == g }
            })
            .collect();
        self.ui().global::<Bridge>().set_gear_time(ModelRc::new(VecModel::from(bars)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alarms_always_show() {
        for g in NEUTRAL..=TOP_GEAR {
            assert!(allows(g, 5));
        }
    }

    #[test]
    fn higher_gears_hold_more() {
        assert!(allows(3, 3) && allows(4, 3) && !allows(5, 3) && !allows(6, 3));
        assert!(allows(5, 6) && !allows(6, 6) && !allows(NEUTRAL, 6));
        assert!(!allows(4, 7) && allows(2, 7));
    }
}
