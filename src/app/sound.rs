//! Audio tab: master and per-app volume, microphone mute and mic test.

use super::{App, s};
use crate::{AppVolume, Bridge, MicItem, sys};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

impl App {
    pub fn refresh_audio(&mut self) {
        let Some(audio) = &self.audio else { return };
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let (vol, muted) = audio.master();
        b.set_master_volume(vol);
        b.set_master_muted(muted);
        b.set_mic_name(s(audio.mic_name()));
        let rows: Vec<AppVolume> = audio
            .apps()
            .into_iter()
            .map(|a| AppVolume {
                initial: s(if a.name == "System sounds" { "♪".into() } else { a.name.chars().next().unwrap_or('?').to_string() }),
                name: s(a.name),
                volume: a.volume,
                muted: a.muted,
            })
            .collect();
        // Update rows in place so a slider that is being dragged isn't recreated.
        let same = self.app_volumes.row_count() == rows.len()
            && rows.iter().enumerate().all(|(i, r)| self.app_volumes.row_data(i).is_some_and(|o| o.name == r.name));
        if same {
            for (i, r) in rows.into_iter().enumerate() {
                if self.app_volumes.row_data(i).as_ref() != Some(&r) {
                    self.app_volumes.set_row_data(i, r);
                }
            }
        } else {
            self.app_volumes.set_vec(rows);
        }
        self.refresh_mic();
    }

    pub fn refresh_mic(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let state = self.audio.as_ref().and_then(|a| a.mic_muted());
        if b.get_has_mic() != state.is_some() {
            b.set_has_mic(state.is_some());
        }
        if let Some(m) = state {
            if b.get_mic_muted() != m {
                b.set_mic_muted(m);
            }
        }
    }

    pub fn toggle_mic(&mut self) {
        let Some(audio) = &self.audio else { return };
        let Some(muted) = audio.mic_muted() else {
            self.toast("No microphone found");
            return;
        };
        audio.set_mic_muted(!muted);
        let name = audio.mic_name();
        self.refresh_mic();
        sys::play_sound(if muted { "DeviceConnect" } else { "DeviceDisconnect" });
        self.peek(9, if muted { "Microphone on" } else { "Microphone muted" }, &name, 2);
        self.eye_event(6, 4.0);
    }

    /// The mic list for the picker.
    pub fn refresh_mics(&self) {
        let Some(audio) = &self.audio else { return };
        let items: Vec<MicItem> =
            audio.mics().into_iter().map(|(id, name, active)| MicItem { id: s(id), name: s(name), active }).collect();
        self.ui().global::<Bridge>().set_mics(ModelRc::new(VecModel::from(items)));
    }

    pub fn select_mic(&mut self, id: &str) {
        let Some(audio) = &self.audio else { return };
        if !audio.set_default_mic(id) {
            self.toast("Couldn't switch microphone");
            return;
        }
        self.stop_mic_test(); // the test listens to the old one
        self.refresh_audio();
        self.refresh_mics();
        self.toast("Microphone switched");
    }

    pub fn stop_mic_test(&mut self) {
        if self.mic_test.take().is_some() {
            self.mic_timer.stop();
            let ui = self.ui();
            let b = ui.global::<Bridge>();
            b.set_mic_testing(false);
            b.set_mic_level(0.0);
        }
    }
}
