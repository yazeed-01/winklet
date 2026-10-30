//! Connects the Slint `Bridge` callbacks to the app, one file per feature.

use crate::app::Shared;
use crate::{AppWindow, Bridge};
use slint::ComponentHandle;

/// Registers a `Bridge` callback that runs `$body` with the app borrowed mutably.
macro_rules! on {
    ($b:ident, $app:ident, $setter:ident, |$a:ident $(, $arg:ident)*| $body:block) => {{
        let shared = $app.clone();
        $b.$setter(move |$($arg),*| {
            #[allow(unused_mut)]
            let mut $a = shared.borrow_mut();
            $body
        });
    }};
}

mod dev;
mod file_drop;
mod focus;
mod game;
mod gear;
mod health;
mod planner;
mod player;
mod settings;
mod shelf;
mod sound;
mod water;
mod window;

pub use file_drop::wire_file_drop;

pub fn wire_callbacks(ui: &AppWindow, app: &Shared) {
    let b = ui.global::<Bridge>();
    window::wire(&b, app);
    player::wire(&b, app);
    focus::wire(&b, app);
    gear::wire(&b, app);
    game::wire(&b, app);
    water::wire(&b, app);
    planner::wire(&b, app);
    dev::wire(&b, app);
    settings::wire(&b, app);
    shelf::wire(&b, app);
    sound::wire(&b, app);
    health::wire(&b, app);
}
