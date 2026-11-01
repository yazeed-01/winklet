//! The Wallpaper tab: browse Windows' wallpapers or your own folders as thumbnails and
//! change the desktop picture with one click, pick how it fills the screen, shuffle, or let
//! it change by itself every so often. Thumbnails and setting the wallpaper run on worker
//! threads so the panel never stutters.

use super::{App, Shared, s, schedule_collapse, with_app};
use crate::{Bridge, WallItem, sys, wall};
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

/// "Change every" steps in minutes (0 = off)
const EVERY: [i32; 6] = [0, 15, 30, 60, 180, 1440];
/// big enough for the carousel's front picture
const THUMB: i32 = 288;
const PREVIEW: i32 = 480;

struct Job {
    gen_: u64,
    path: String,
    size: i32,
}

#[derive(Default)]
pub struct Walls {
    pub items: Rc<VecModel<WallItem>>,
    paths: Vec<String>,
    thumbs: HashMap<String, Image>,
    /// bumped on every refresh so stale thumbnail jobs are skipped
    gen_: Arc<AtomicU64>,
    tx: Option<Sender<Job>>,
    last_auto: Option<Instant>,
}

fn name_of(p: &str) -> String {
    Path::new(p).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn to_image(w: u32, h: u32, px: &[u8]) -> Image {
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(px, w, h))
}

fn every_text(m: i32) -> String {
    match m {
        0 => "Off".into(),
        m if m < 60 => format!("{m} min"),
        1440 => "1 day".into(),
        m => format!("{} h", m / 60),
    }
}

/// Runs `f` on a thread of its own with COM ready (the shell APIs need it).
fn with_com(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        f();
    });
}

impl App {
    /// Folder of a source: 0 = Windows' wallpapers, then your folders.
    fn wall_dir(&self, source: i32) -> PathBuf {
        match source {
            0 => wall::windows_dir(),
            i => self.data.settings.wallpaper_dirs.get(i as usize - 1).map(PathBuf::from).unwrap_or_else(wall::windows_dir),
        }
    }

    fn wall_worker(&mut self) -> Sender<Job> {
        if let Some(tx) = &self.wall.tx {
            return tx.clone();
        }
        let (tx, rx) = channel::<Job>();
        let gen_ = self.wall.gen_.clone();
        with_com(move || {
            for job in rx {
                // the preview of the current wallpaper (gen 0) is always wanted
                if job.gen_ != 0 && job.gen_ != gen_.load(Ordering::Relaxed) {
                    continue;
                }
                let Some((w, h, px)) = wall::thumbnail(&job.path, job.size) else { continue };
                let _ = slint::invoke_from_event_loop(move || {
                    with_app(|s| s.borrow_mut().wall_thumb(job.gen_, &job.path, job.size, w, h, &px));
                });
            }
        });
        self.wall.tx = Some(tx.clone());
        tx
    }

    pub fn refresh_wallpaper(&mut self) {
        let st = &self.data.settings;
        let source = st.wallpaper_source.clamp(0, st.wallpaper_dirs.len() as i32);
        let sources: Vec<slint::SharedString> = std::iter::once(s("Windows"))
            .chain(st.wallpaper_dirs.iter().map(|d| s(name_of(d).chars().take(18).collect::<String>())))
            .collect();
        let every = st.wallpaper_every;
        let dir = self.wall_dir(source);
        let paths: Vec<String> = wall::pictures(&dir).into_iter().map(|p| p.to_string_lossy().to_string()).collect();
        let current = wall::current().unwrap_or_default();
        let gen_ = self.wall.gen_.fetch_add(1, Ordering::Relaxed) + 1;
        let items: Vec<WallItem> = paths
            .iter()
            .map(|p| {
                let thumb = self.wall.thumbs.get(p).cloned();
                WallItem {
                    path: s(p.clone()),
                    name: s(name_of(p)),
                    loaded: thumb.is_some(),
                    thumb: thumb.unwrap_or_default(),
                    current: p.eq_ignore_ascii_case(&current),
                }
            })
            .collect();
        let tx = self.wall_worker();
        for p in paths.iter().filter(|p| !self.wall.thumbs.contains_key(*p)) {
            let _ = tx.send(Job { gen_, path: p.clone(), size: THUMB });
        }
        if !current.is_empty() {
            let _ = tx.send(Job { gen_: 0, path: current.clone(), size: PREVIEW });
        }
        self.wall.items.set_vec(items);
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_walls(ModelRc::from(self.wall.items.clone()));
        // the carousel opens on the picture on the desktop now
        b.set_wall_focus(paths.iter().position(|p| p.eq_ignore_ascii_case(&current)).unwrap_or(0) as i32);
        b.set_wall_view(self.data.settings.wallpaper_view);
        b.set_wall_sources(ModelRc::new(VecModel::from(sources)));
        b.set_wall_source(source);
        b.set_wall_current_name(s(if current.is_empty() { "A solid colour or slideshow".into() } else { name_of(&current) }));
        if current.is_empty() {
            b.set_wall_current(Image::default());
        }
        b.set_wall_fit(wall::position());
        b.set_wall_every_text(s(every_text(every)));
        b.set_wall_dir(s(dir.to_string_lossy().to_string()));
        self.wall.paths = paths;
    }

    /// Grid or carousel; remembered for next time.
    pub fn wall_view_set(&mut self, v: i32) {
        self.data.settings.wallpaper_view = v.clamp(0, 1);
        self.data.save();
        self.ui().global::<Bridge>().set_wall_view(self.data.settings.wallpaper_view);
    }

    fn wall_thumb(&mut self, gen_: u64, path: &str, size: i32, w: u32, h: u32, px: &[u8]) {
        let img = to_image(w, h, px);
        if size == PREVIEW {
            self.ui().global::<Bridge>().set_wall_current(img);
            return;
        }
        self.wall.thumbs.insert(path.to_string(), img.clone());
        if gen_ != self.wall.gen_.load(Ordering::Relaxed) {
            return;
        }
        if let Some(i) = self.wall.paths.iter().position(|p| p == path) {
            if let Some(mut row) = self.wall.items.row_data(i) {
                row.thumb = img;
                row.loaded = true;
                self.wall.items.set_row_data(i, row);
            }
        }
    }

    /// Puts `path` on the desktop (on a worker thread: it can take a moment).
    pub fn wall_set(&mut self, path: &str) {
        let path = path.to_string();
        // mark it right away so the click feels instant
        for i in 0..self.wall.items.row_count() {
            if let Some(mut row) = self.wall.items.row_data(i) {
                let now = row.path == path.as_str();
                if row.current != now {
                    row.current = now;
                    self.wall.items.set_row_data(i, row);
                }
            }
        }
        if let Some(img) = self.wall.thumbs.get(&path).cloned() {
            let ui = self.ui();
            let b = ui.global::<Bridge>();
            b.set_wall_current(img);
            b.set_wall_current_name(s(name_of(&path)));
        }
        with_com(move || {
            let ok = wall::set(&path);
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|s| {
                    let mut a = s.borrow_mut();
                    if ok {
                        if let Some(tx) = a.wall.tx.clone() {
                            let _ = tx.send(Job { gen_: 0, path, size: PREVIEW });
                        }
                    } else {
                        a.toast("Couldn't change the wallpaper");
                    }
                });
            });
        });
    }

    pub fn wall_fit_set(&mut self, i: i32) {
        with_com(move || {
            wall::set_position(i);
        });
        self.ui().global::<Bridge>().set_wall_fit(i);
    }

    /// A random picture from the source on screen, not the one already up.
    pub fn wall_shuffle(&mut self) {
        let paths = if self.wall.paths.is_empty() {
            let dir = self.wall_dir(self.data.settings.wallpaper_source);
            wall::pictures(&dir).into_iter().map(|p| p.to_string_lossy().to_string()).collect()
        } else {
            self.wall.paths.clone()
        };
        let current = wall::current().unwrap_or_default();
        let pool: Vec<&String> = paths.iter().filter(|p| !p.eq_ignore_ascii_case(&current)).collect();
        if pool.is_empty() {
            self.toast("No other pictures here");
            return;
        }
        let pick = pool[((self.rand() * pool.len() as f32) as usize).min(pool.len() - 1)].clone();
        self.wall_set(&pick);
    }

    pub fn wall_source_set(&mut self, i: i32) {
        self.data.settings.wallpaper_source = i.max(0);
        self.data.save();
        self.refresh_wallpaper();
    }

    pub fn wall_remove_folder(&mut self, i: i32) {
        let dirs = &mut self.data.settings.wallpaper_dirs;
        if i < 1 || i as usize > dirs.len() {
            return;
        }
        dirs.remove(i as usize - 1);
        self.data.settings.wallpaper_source = 0;
        self.data.save();
        self.refresh_wallpaper();
        self.toast("Folder removed (the pictures stay where they are)");
    }

    pub fn wall_every_adjust(&mut self, d: i32) {
        let cur = EVERY.iter().position(|m| *m == self.data.settings.wallpaper_every).unwrap_or(0) as i32;
        self.data.settings.wallpaper_every = EVERY[(cur + d).clamp(0, EVERY.len() as i32 - 1) as usize];
        self.data.save();
        self.wall.last_auto = Some(Instant::now());
        self.ui().global::<Bridge>().set_wall_every_text(s(every_text(self.data.settings.wallpaper_every)));
    }

    /// From the heartbeat, once a minute: a new picture when it's time.
    pub fn wall_tick(&mut self) {
        let every = self.data.settings.wallpaper_every;
        if every <= 0 {
            return;
        }
        let last = *self.wall.last_auto.get_or_insert_with(Instant::now);
        if last.elapsed() >= Duration::from_secs(every as u64 * 60) {
            self.wall.last_auto = Some(Instant::now());
            self.wall_shuffle();
        }
    }
}

/// "+ Folder": add a folder of your own pictures. The dialog is modal, so the app isn't
/// borrowed meanwhile.
pub fn wall_add_folder(shared: &Shared) {
    let hwnd = {
        let mut a = shared.borrow_mut();
        a.hovering = true;
        a.collapse_timer.stop();
        a.hwnd
    };
    let picked = sys::pick_paths_titled(hwnd, true, "Pick a folder of wallpapers");
    let mut a = shared.borrow_mut();
    if let Some(dir) = picked.into_iter().next() {
        let dir = dir.to_string_lossy().to_string();
        let dirs = &mut a.data.settings.wallpaper_dirs;
        let at = match dirs.iter().position(|d| d.eq_ignore_ascii_case(&dir)) {
            Some(i) => i,
            None => {
                dirs.push(dir);
                dirs.len() - 1
            }
        };
        a.data.settings.wallpaper_source = at as i32 + 1;
        a.data.save();
        a.refresh_wallpaper();
    }
    a.hovering = false;
    schedule_collapse(shared, &mut a, 3000);
}
