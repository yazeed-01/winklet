//! Little things the island says through the day: good morning / afternoon / evening, welcome
//! back after a break, and now and then a short line that fits the hour, the day or how your
//! day is going. Each one types itself out in a speech peek next to the eyes.

use super::{App, s, with_app};
use crate::Bridge;
use chrono::{Datelike, Local, NaiveDate, Timelike, Weekday};
use slint::{ComponentHandle, Timer, TimerMode};
use std::time::{Duration, Instant};

/// Peek kind for a line the island says (see the list in bridge.slint).
pub const SAY: i32 = 21;

// glyphs and tones (the tone picks the colour in Slint)
const SUN: &str = "\u{E706}";
const MOON: &str = "\u{E708}";
const HEART: &str = "\u{EB51}";
const STAR: &str = "\u{E735}";
const SMILE: &str = "\u{E76E}";
const WALK: &str = "\u{E805}";
const EYE: &str = "\u{E7B3}";
const TIMER: &str = "\u{E916}";
const MORNING: i32 = 0;
const DAY: i32 = 1;
const EVENING: i32 = 2;
const NIGHT: i32 = 3;
const CHEER: i32 = 4;
const WATER: i32 = 5;

/// Minutes between the little lines, give or take a random half hour.
const EVERY_MIN: f32 = 80.0;
/// A break at least this long earns a "welcome back".
const AWAY_MIN: u64 = 10;

pub struct Greet {
    /// the day and part of day (0 morning .. 3 night) already greeted
    greeted: Option<(NaiveDate, u8)>,
    /// when the next little line may come
    next: Instant,
    /// since when the user has been away (idle)
    away_since: Option<Instant>,
    /// recent lines, so they don't repeat soon
    recent: Vec<&'static str>,
    /// the line being typed and how much of it shows
    text: Vec<char>,
    typed: usize,
    typer: Timer,
}

impl Default for Greet {
    fn default() -> Self {
        Greet {
            greeted: None,
            // give the first launch a moment before anything else
            next: Instant::now() + Duration::from_secs(25 * 60),
            away_since: None,
            recent: Vec::new(),
            text: Vec::new(),
            typed: 0,
            typer: Timer::default(),
        }
    }
}

/// Part of the day: 0 morning (5–12), 1 afternoon (12–17), 2 evening (17–22), 3 night.
fn part_of_day(hour: u32) -> u8 {
    match hour {
        5..=11 => 0,
        12..=16 => 1,
        17..=21 => 2,
        _ => 3,
    }
}

impl App {
    /// Once a minute: greet a new part of the day, or now and then say a little line.
    pub fn greet_tick(&mut self) {
        if !self.can_say() {
            return;
        }
        let now = Local::now();
        let part = part_of_day(now.hour());
        let today = now.date_naive();
        if self.greet.greeted != Some((today, part)) {
            // late at night only once, not again after midnight
            let night_done = part == 3 && self.greet.greeted.is_some_and(|(d, p)| p == 3 && (d == today || now.hour() < 5));
            self.greet.greeted = Some((today, part));
            if !night_done {
                let (title, sub, glyph, tone) = self.greeting(part);
                self.say(&title, &sub, glyph, tone);
                return;
            }
        }
        if Instant::now() >= self.greet.next {
            let (title, sub, glyph, tone) = self.little_line();
            self.say(&title, &sub, glyph, tone);
        }
    }

    /// The user went idle (true) or came back (false).
    pub fn greet_idle(&mut self, idle: bool) {
        if idle {
            self.greet.away_since = Some(Instant::now());
            return;
        }
        let away = self.greet.away_since.take().map(|t| t.elapsed().as_secs() / 60).unwrap_or(0);
        if away < AWAY_MIN || !self.can_say() {
            return;
        }
        let part = part_of_day(Local::now().hour());
        // a new part of the day started while away: the greeting covers it
        if self.greet.greeted != Some((Local::now().date_naive(), part)) {
            return self.greet_tick();
        }
        let subs: &[&str] = if away >= 60 {
            &["Hope it was a good break", "Ready when you are", "Let's pick up where you left off"]
        } else {
            &["Missed you a little", "Ready for the next bit?", "Hope that was a nice break"]
        };
        let sub = subs[(self.rand() * subs.len() as f32) as usize % subs.len()];
        self.say("Welcome back", sub, SMILE, CHEER);
    }

    fn can_say(&self) -> bool {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        self.data.settings.greetings
            && !self.hidden
            && !b.get_expanded()
            && !b.get_peeking()
            && !b.get_user_idle()
            && !self.data.settings.game_mode
            && !self.game_in_front()
            && !(self.focus.running && !self.focus.is_break)
            && self.gear_allows(SAY)
            && !self.audio.as_ref().is_some_and(|a| a.mic_in_use())
    }

    fn greeting(&mut self, part: u8) -> (String, String, &'static str, i32) {
        let now = Local::now();
        let today = now.date_naive();
        let tasks = self.data.todos.iter().filter(|t| !t.done && t.date == Some(today)).count();
        let water = self.data.water_today();
        let sessions = self.data.sessions_today();
        let pick = |a: &mut App, list: &[&'static str]| list[(a.rand() * list.len() as f32) as usize % list.len()].to_string();
        match part {
            0 => {
                let sub = match (tasks, now.weekday()) {
                    (1, _) => "One task waiting for you today".to_string(),
                    (n, _) if n > 1 => format!("{n} tasks on today's list. You've got this"),
                    (_, Weekday::Mon) => "New week, fresh start".into(),
                    (_, Weekday::Fri) => "It's Friday. Almost there".into(),
                    (_, Weekday::Sat | Weekday::Sun) => "Enjoy a slow weekend morning".into(),
                    _ if water == 0 => "Start with a glass of water".into(),
                    _ => pick(self, &["Let's make it a good one", "Fresh day, fresh ideas", "Coffee first, then conquer"]),
                };
                ("Good morning".into(), sub, SUN, MORNING)
            }
            1 => {
                let sub = if sessions > 0 {
                    format!("{sessions} focus {} done already. Nice", if sessions == 1 { "session" } else { "sessions" })
                } else {
                    pick(self, &["Hope lunch was good", "Halfway through the day", "Keep the good pace going"])
                };
                ("Good afternoon".into(), sub, SUN, DAY)
            }
            2 => {
                let sub = match (sessions, water) {
                    (n, _) if n >= 2 => format!("{n} focus sessions today. Proud of you"),
                    (_, w) if w >= self.data.settings.water_goal_ml => "Water goal done today. Well done".into(),
                    _ => pick(self, &["Time to slow down a little", "Hope today treated you well", "You did good today"]),
                };
                ("Good evening".into(), sub, MOON, EVENING)
            }
            _ => ("It's getting late".into(), pick(self, &["Wrap up and get some rest", "Sleep is a superpower", "Tomorrow-you says thanks for sleeping"]), MOON, NIGHT),
        }
    }

    /// A short line that fits the moment, not one said lately.
    fn little_line(&mut self) -> (String, String, &'static str, i32) {
        let now = Local::now();
        let hour = now.hour();
        let mut pool: Vec<(&'static str, &'static str, &'static str, i32)> = vec![
            ("You're doing great", "Seriously. Keep it up", HEART, CHEER),
            ("Deep breath in", "…and slowly out. Better?", HEART, CHEER),
            ("One thing at a time", "Small steps still count", STAR, CHEER),
            ("Posture check", "Shoulders down, back straight", WALK, DAY),
            ("Blink a few times", "Your eyes will thank you", EYE, DAY),
            ("Unclench your jaw", "You were doing it, weren't you", SMILE, CHEER),
            ("Proud of you", "Just thought you should know", HEART, CHEER),
            ("Stretch a little?", "Reach up high for ten seconds", WALK, DAY),
            ("Smile", "It suits you", SMILE, CHEER),
            ("Look out a window", "Far away things rest your eyes", EYE, DAY),
        ];
        match hour {
            5..=11 => pool.extend([
                ("Morning energy", "Tackle the hardest thing first", SUN, MORNING),
                ("Rise and shine", "The day is yours", SUN, MORNING),
            ]),
            12..=16 => pool.extend([
                ("Afternoon slump?", "A short walk works wonders", WALK, DAY),
                ("Snack time?", "Something with fruit maybe", SMILE, DAY),
            ]),
            17..=21 => pool.extend([
                ("Winding down", "What went well today?", MOON, EVENING),
                ("Nice work today", "Time to rest those eyes", MOON, EVENING),
            ]),
            _ => pool.extend([
                ("Still up?", "Don't stay too late", MOON, NIGHT),
                ("Screens off soon?", "Your sleep will be better", MOON, NIGHT),
            ]),
        }
        match now.weekday() {
            Weekday::Mon => pool.push(("New week, new wins", "Start small, keep going", STAR, CHEER)),
            Weekday::Fri => pool.push(("Happy Friday", "The weekend is so close", STAR, CHEER)),
            Weekday::Sat | Weekday::Sun => pool.push(("It's the weekend", "Don't forget to have fun", SMILE, CHEER)),
            _ => {}
        }
        let water = self.data.water_today();
        let goal = self.data.settings.water_goal_ml.max(1);
        if water < goal / 3 && hour >= 12 {
            pool.push(("Water break", "You're a little behind on water today", HEART, WATER));
        } else if water >= goal {
            pool.push(("Water goal done", "Your body says thanks", HEART, WATER));
        }
        let sessions = self.data.sessions_today();
        if sessions >= 3 {
            pool.push(("Focus champion", "Three or more sessions today", TIMER, MORNING));
        }

        let fresh: Vec<_> = pool.iter().filter(|l| !self.greet.recent.contains(&l.0)).cloned().collect();
        let choose = if fresh.is_empty() { &pool } else { &fresh };
        let line = choose[(self.rand() * choose.len() as f32) as usize % choose.len()];
        self.greet.recent.push(line.0);
        if self.greet.recent.len() > 8 {
            self.greet.recent.remove(0);
        }
        (line.0.to_string(), line.1.to_string(), line.2, line.3)
    }

    /// Shows a line in the speech peek and types it out.
    pub fn say(&mut self, title: &str, sub: &str, glyph: &str, tone: i32) {
        let jitter = (self.rand() - 0.5) * 60.0;
        self.greet.next = Instant::now() + Duration::from_secs_f32((EVERY_MIN + jitter) * 60.0);
        {
            let ui = self.ui();
            let b = ui.global::<Bridge>();
            b.set_say_text(s(title));
            b.set_say_shown(s(""));
            b.set_say_sub(s(sub));
            b.set_say_glyph(s(glyph));
            b.set_say_tone(tone);
            b.set_say_done(false);
        }
        self.greet.text = title.chars().collect();
        self.greet.typed = 0;
        let secs = 6 + (title.len() + sub.len()) as u64 / 18;
        self.peek(SAY, title, sub, secs);
        // a beat for the peek to open, then one letter at a time
        self.greet.typer.start(TimerMode::Repeated, Duration::from_millis(45), || {
            with_app(|s| {
                let Ok(mut a) = s.try_borrow_mut() else { return };
                a.say_type();
            });
        });
    }

    fn say_type(&mut self) {
        let g = &mut self.greet;
        g.typed += 1;
        // the first few ticks are the pause while the peek opens
        let shown = g.typed.saturating_sub(6).min(g.text.len());
        let text: String = g.text[..shown].iter().collect();
        let done = shown >= g.text.len();
        if done {
            g.typer.stop();
        }
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_say_shown(s(text));
        if done {
            b.set_say_done(true);
        }
    }

    /// Settings › "Say hi": a line right now, to see what it looks like.
    pub fn say_hi(&mut self) {
        let part = part_of_day(Local::now().hour());
        let (title, sub, glyph, tone) = if self.rand() < 0.5 { self.greeting(part) } else { self.little_line() };
        self.collapse();
        // after the panel has closed
        Timer::single_shot(Duration::from_millis(700), move || {
            with_app(|s| s.borrow_mut().say(&title, &sub, glyph, tone));
        });
    }
}
