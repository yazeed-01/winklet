//! Water and other drinks.

use crate::Bridge;
use crate::app::Shared;

pub fn wire(b: &Bridge, app: &Shared) {
    // the water reminder's "+ cup" button
    on!(b, app, on_water_add, |a, ml| { a.drink_add("water", Some(ml)); });
    on!(b, app, on_drink_add, |a, id| { a.drink_add(&id, None); });
    on!(b, app, on_drink_pick, |a, id| { a.drink_pick(&id); });
    on!(b, app, on_drink_add_size, |a, id, ml| { a.drink_add_size(&id, ml); });
    on!(b, app, on_water_undo, |a| { a.drink_undo(); });
    on!(b, app, on_drink_edit, |a, id| { a.drink_edit(&id); });
    on!(b, app, on_drink_save, |a| { a.drink_save(); });
    on!(b, app, on_drink_delete, |a| { a.drink_delete(); });
    on!(b, app, on_water_goal_adjust, |a, d| {
        a.data.settings.water_goal_ml = (a.data.settings.water_goal_ml + d * 250).clamp(500, 6000);
        a.data.save();
        a.refresh_water();
    });
    on!(b, app, on_cup_adjust, |a, d| {
        a.data.settings.cup_ml = (a.data.settings.cup_ml + d * 50).clamp(50, 1000);
        a.data.save();
        a.refresh_water();
    });
}
