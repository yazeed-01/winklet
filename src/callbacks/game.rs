//! Game mode: the switch, the library (launch / forget) and the FPS permission.

use crate::Bridge;
use crate::app::{Shared, defer};

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_game_toggle, |a| {
        let on = !a.data.settings.game_mode;
        a.set_game_mode(on, false);
    });
    on!(b, app, on_game_view_changed, |a, v| {
        if v == 1 {
            a.refresh_games();
        }
    });
    on!(b, app, on_game_launch, |a, key| { a.game_launch(&key); });
    on!(b, app, on_game_forget, |a, key| { a.game_forget(&key); });
    // may switch game mode off, which re-reads the tab: let the click finish first
    b.on_game_not_game(|| defer(|s| s.borrow_mut().game_not_game()));
    on!(b, app, on_game_allow_fps, |a| { a.game_allow_fps(); });
    on!(b, app, on_game_scan, |a| { a.game_scan(true); });
    on!(b, app, on_game_search, |a, q| { a.games_search(&q); });
    // the file dialog is modal and pumps messages: open it after the click, unborrowed
    b.on_game_add(|| defer(crate::app::games_pick));
}
