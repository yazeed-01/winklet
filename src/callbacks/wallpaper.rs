//! Wallpaper tab: pick a picture, how it fills the screen, folders, shuffle.

use crate::app::{Shared, defer, wall_add_folder};
use crate::{Bridge, sys};
use slint::ComponentHandle;

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_wall_set, |a, path| { a.wall_set(&path); });
    on!(b, app, on_wall_fit_set, |a, i| { a.wall_fit_set(i); });
    on!(b, app, on_wall_source_set, |a, i| { a.wall_source_set(i); });
    on!(b, app, on_wall_remove_folder, |a, i| { a.wall_remove_folder(i); });
    on!(b, app, on_wall_shuffle, |a| { a.wall_shuffle(); });
    on!(b, app, on_wall_every_adjust, |a, d| { a.wall_every_adjust(d); });
    b.on_wall_add_folder(|| defer(wall_add_folder));
    on!(b, app, on_wall_open_folder, |a| {
        let dir = a.ui().global::<Bridge>().get_wall_dir();
        sys::open(&dir, "");
    });
}
