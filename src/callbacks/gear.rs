//! Gear shifter (drive modes).

use crate::Bridge;
use crate::app::Shared;

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_gear_shift, |a, g| {
        a.shift_gear(g);
    });
    on!(b, app, on_gear_reverse, |a| {
        a.gear_reverse();
    });
}
