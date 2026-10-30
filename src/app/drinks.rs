//! Drinks: water plus tea, coffee, lattes… each with its own colour. Every drink counts
//! towards the daily goal, and the bottle fills in layers of what you drank.

use super::{App, s};
use crate::store::DrinkType;
use crate::{Bridge, DrinkItem, MixPart};
use chrono::{Local, NaiveDate};
use slint::{Color, ComponentHandle, ModelRc, VecModel};
use std::time::Instant;

/// Colours offered for your own drinks.
pub const PALETTE: [&str; 12] = [
    "#38bdf8", "#60a5fa", "#a78bfa", "#f472b6", "#f87171", "#fb923c", "#fbbf24", "#a3e635", "#4ade80", "#2dd4bf",
    "#c08457", "#e8c9a0",
];

pub fn color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| h.get(i..i + 2).and_then(|v| u8::from_str_radix(v, 16).ok()).unwrap_or(0x80);
    Color::from_rgb_u8(p(0), p(2), p(4))
}

impl App {
    /// Opens "how much?" for a drink: cup sizes drawn out, its usual one marked.
    pub fn drink_pick(&mut self, id: &str) {
        let Some(d) = self.data.drinks.iter().find(|d| d.id == id) else { return };
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_drink_pick_id(s(d.id.clone()));
        b.set_drink_pick_name(s(d.name.clone()));
        b.set_drink_pick_color(color(&d.color));
        b.set_drink_pick_ml(d.ml);
        b.set_drink_pick_custom(d.ml);
        b.set_drink_pick_open(true);
    }

    /// Logs a drink picked from the size sheet; that size becomes its usual one.
    pub fn drink_add_size(&mut self, id: &str, ml: i32) {
        let ml = ml.clamp(10, 3000);
        if let Some(d) = self.data.drinks.iter_mut().find(|d| d.id == id) {
            d.ml = ml;
            if d.id == "water" {
                self.data.settings.cup_ml = ml; // the reminder's "+ cup" follows it
            }
        }
        self.ui().global::<Bridge>().set_drink_pick_open(false);
        self.drink_add(id, Some(ml));
    }

    /// Logs one drink (its usual size, or `ml`).
    pub fn drink_add(&mut self, id: &str, ml: Option<i32>) {
        let Some(d) = self.data.drinks.iter().find(|d| d.id == id).cloned() else { return };
        let ml = ml.unwrap_or(d.ml).max(1);
        let today = Local::now().date_naive();
        let before = self.data.water_today();
        *self.data.water.entry(today).or_default() += ml;
        *self.data.drink_days.entry(today).or_default().entry(d.id.clone()).or_default() += ml;
        self.data.drink_undo.retain(|u| u.0 == today);
        self.data.drink_undo.push((today, d.id, ml));
        self.last_drink = Instant::now();
        self.data.save();
        self.refresh_water();
        let goal = self.data.settings.water_goal_ml;
        if before < goal && before + ml >= goal {
            self.toast("Daily goal reached!");
            self.eye_event(10, 4.0);
        } else {
            self.toast(&format!("{} · {ml} ml", d.name));
        }
    }

    /// Takes back today's last drink.
    pub fn drink_undo(&mut self) {
        let today = Local::now().date_naive();
        let (id, ml) = match self.data.drink_undo.pop() {
            Some((day, id, ml)) if day == today => (Some(id), ml),
            Some(_) => {
                self.data.drink_undo.clear();
                return;
            }
            // sips logged before drinks existed
            None => match self.data.water_log.pop() {
                Some(ml) => (None, ml),
                None => return,
            },
        };
        let w = self.data.water.entry(today).or_default();
        *w = (*w - ml).max(0);
        if let Some(id) = id {
            if let Some(day) = self.data.drink_days.get_mut(&today) {
                if let Some(v) = day.get_mut(&id) {
                    *v = (*v - ml).max(0);
                }
                day.retain(|_, v| *v > 0);
            }
        }
        self.data.save();
        self.refresh_water();
    }

    /// Opens the drink editor: `id` = "" for a new drink.
    pub fn drink_edit(&mut self, id: &str) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let (name, hex, ml) = match self.data.drinks.iter().find(|d| d.id == id) {
            Some(d) => (d.name.clone(), d.color.clone(), d.ml),
            None => {
                // a colour nobody uses yet
                let free = PALETTE.iter().find(|c| !self.data.drinks.iter().any(|d| d.color.eq_ignore_ascii_case(c)));
                (String::new(), free.unwrap_or(&PALETTE[0]).to_string(), 250)
            }
        };
        b.set_drink_edit_id(s(id));
        b.set_drink_edit_name(s(name));
        b.set_drink_edit_color(PALETTE.iter().position(|c| c.eq_ignore_ascii_case(&hex)).map(|i| i as i32).unwrap_or(-1));
        b.set_drink_edit_ml(ml);
        b.set_drink_edit_open(true);
    }

    pub fn drink_save(&mut self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let name = b.get_drink_edit_name().trim().to_string();
        if name.is_empty() {
            self.toast("Give your drink a name");
            return;
        }
        let id = b.get_drink_edit_id().to_string();
        let ml = b.get_drink_edit_ml().clamp(10, 2000);
        let pick = usize::try_from(b.get_drink_edit_color()).ok().and_then(|i| PALETTE.get(i)).map(|c| c.to_string());
        match self.data.drinks.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.name = name;
                d.ml = ml;
                if let Some(c) = pick {
                    d.color = c;
                }
                if d.id == "water" {
                    self.data.settings.cup_ml = ml; // the water reminder's "+ cup" button
                }
            }
            None => self.data.drinks.push(DrinkType {
                id: crate::store::new_id(),
                name,
                color: pick.unwrap_or_else(|| PALETTE[0].to_string()),
                ml,
            }),
        }
        b.set_drink_edit_open(false);
        self.data.save();
        self.refresh_water();
    }

    /// Removes the drink being edited (water stays: reminders use it). Past days keep their totals.
    pub fn drink_delete(&mut self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let id = b.get_drink_edit_id().to_string();
        if id != "water" {
            self.data.drinks.retain(|d| d.id != id);
            self.data.save();
        }
        b.set_drink_edit_open(false);
        self.refresh_water();
    }

    /// Drink tiles and the bottle's layers for today.
    pub fn refresh_drinks(&self, today: NaiveDate) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let day = self.data.drink_days.get(&today).cloned().unwrap_or_default();
        let items: Vec<DrinkItem> = self
            .data
            .drinks
            .iter()
            .map(|d| DrinkItem {
                id: s(d.id.clone()),
                name: s(d.name.clone()),
                color: color(&d.color),
                ml: d.ml,
                today: *day.get(&d.id).unwrap_or(&0),
            })
            .collect();
        b.set_drinks(ModelRc::new(VecModel::from(items)));

        // layers from the bottom: water logged before drinks existed, then each drink in list order
        let total = self.data.water_today().max(1) as f32;
        let water = self.data.drinks.iter().find(|d| d.id == "water").map(|d| d.color.clone()).unwrap_or_else(|| PALETTE[0].into());
        let tracked: i32 = day.values().sum();
        let mut layers: Vec<(String, i32)> = Vec::new();
        if self.data.water_today() > tracked {
            layers.push((water.clone(), self.data.water_today() - tracked));
        }
        for d in &self.data.drinks {
            if let Some(ml) = day.get(&d.id).filter(|v| **v > 0) {
                layers.push((d.color.clone(), *ml));
            }
        }
        // drinks deleted since: show them in water colour
        let known: i32 = self.data.drinks.iter().filter_map(|d| day.get(&d.id)).sum();
        if tracked > known {
            layers.push((water.clone(), tracked - known));
        }
        let mut at = 0.0;
        let parts: Vec<MixPart> = layers
            .iter()
            .map(|(c, ml)| {
                let start = at;
                at = (at + *ml as f32 / total).min(1.0);
                MixPart { color: color(c), start, end: at }
            })
            .collect();
        b.set_water_top(color(&layers.last().map(|l| l.0.clone()).unwrap_or(water)));
        b.set_water_mix(ModelRc::new(VecModel::from(parts)));
        b.set_drink_palette(ModelRc::new(VecModel::from(PALETTE.iter().map(|c| color(c)).collect::<Vec<_>>())));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_colours() {
        let c = super::color("#38bdf8");
        assert_eq!((c.red(), c.green(), c.blue()), (0x38, 0xbd, 0xf8));
    }
}
