//! Shelf > Send: a basket of files to send to a phone or PC on the same Wi-Fi with a QR
//! code (and to receive files from them into Downloads).

use super::{App, Shared, s, schedule_collapse, with_app};
use crate::share::{self, Event, Server};
use crate::{Bridge, SendItem, sys};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode, VecModel};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
pub struct Send {
    /// files and folders waiting to be sent
    pub items: Vec<PathBuf>,
    pub server: Option<Arc<Server>>,
    pub preparing: bool,
    pub status: String,
    pub timer: Timer,
}

impl App {
    /// Adds files / folders to the basket (and to the open share, if any).
    pub fn send_add(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        let mut n = 0;
        for p in paths {
            if (p.is_file() || p.is_dir()) && !self.send.items.iter().any(|x| x == &p) {
                self.send.items.push(p);
                n += 1;
            }
        }
        if n > 0 {
            self.send_sync();
        }
        self.refresh_send();
        n
    }

    pub fn send_remove(&mut self, i: usize) {
        if i < self.send.items.len() {
            self.send.items.remove(i);
            self.send_sync();
            self.refresh_send();
        }
    }

    pub fn send_clear(&mut self) {
        self.send.items.clear();
        self.send_sync();
        self.refresh_send();
    }

    /// The open share follows the basket (folders are zipped off the UI thread).
    fn send_sync(&mut self) {
        let Some(server) = self.send.server.clone() else { return };
        let items = self.send.items.clone();
        std::thread::spawn(move || {
            let _ = server.set_files(&items);
        });
    }

    /// Opens the link and shows its QR code.
    pub fn send_start(&mut self) {
        if self.send.server.is_some() || self.send.preparing {
            return;
        }
        self.send.preparing = true;
        self.send.status = if self.send.items.iter().any(|p| p.is_dir()) { "Zipping folders…".into() } else { "Starting…".into() };
        self.refresh_send();
        let items = self.send.items.clone();
        let inbox = PathBuf::from(super::folder_for(self, "downloads"));
        std::thread::spawn(move || {
            let res = share::start(&items, inbox, |ev| {
                let _ = slint::invoke_from_event_loop(move || with_app(|s| s.borrow_mut().send_event(ev)));
            });
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|s| {
                    let mut a = s.borrow_mut();
                    a.send.preparing = false;
                    match res {
                        Ok(server) => {
                            a.send.status = "Waiting for a phone or PC…".into();
                            if std::env::var_os("WINKLET_DEMO_TAB").is_some() {
                                eprintln!("share url: {}", server.url); // demo runs: lets a script test the link
                            }
                            a.ui().global::<Bridge>().set_send_qr(qr_image(&shown_url(&server.url)));
                            a.send.server = Some(server);
                            a.send_tick_start(s);
                        }
                        Err(e) => a.send.status = e,
                    }
                    a.refresh_send();
                })
            });
        });
    }

    pub fn send_stop(&mut self) {
        if let Some(server) = self.send.server.take() {
            server.stop();
        }
        self.send.timer.stop();
        self.send.status.clear();
        self.refresh_send();
    }

    /// Once a second while sharing: the countdown.
    fn send_tick_start(&mut self, shared: &Shared) {
        let weak = Rc::downgrade(shared);
        self.send.timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            if let Some(s) = weak.upgrade() {
                s.borrow().refresh_send_left();
            }
        });
    }

    fn send_event(&mut self, ev: Event) {
        let Some(server) = self.send.server.clone() else { return };
        match ev {
            Event::Connected => {
                self.send.status = "A device opened the link".into();
                self.peek(0, "Device connected", "It can now get your files and send some back", 3);
            }
            Event::Sent(name) => {
                let n = server.downloads.load(std::sync::atomic::Ordering::Relaxed);
                self.send.status = format!("Sent {name} · {n} download{}", if n == 1 { "" } else { "s" });
            }
            Event::Received(name) => {
                self.send.status = format!("Received {name}");
                self.peek(0, &format!("Received {name}"), "Saved in your Downloads folder", 4);
                sys::play_sound("Notification.Default");
            }
            Event::Expired => {
                self.send.server = None;
                self.send.timer.stop();
                self.send.status = "The link expired. Start again to keep sharing".into();
                self.peek(0, "Share link expired", "Open Shelf > Send to start a new one", 3);
            }
        }
        self.refresh_send();
    }

    fn refresh_send_left(&self) {
        let left = self.send.server.as_ref().map(|s| s.left().as_secs()).unwrap_or(0);
        self.ui().global::<Bridge>().set_send_left(s(format!("{}:{:02}", left / 60, left % 60)));
    }

    pub fn refresh_send(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let mut total = 0u64;
        let items: Vec<SendItem> = self
            .send
            .items
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let is_dir = p.is_dir();
                let size = if is_dir { 0 } else { std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) };
                total += size;
                SendItem {
                    index: i as i32,
                    name: s(p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                    size: s(if is_dir { "Folder · sent as .zip".to_string() } else { share::human_size(size) }),
                    is_dir,
                }
            })
            .collect();
        let n = items.len();
        b.set_send_items(ModelRc::new(VecModel::from(items)));
        b.set_send_total(s(match n {
            0 => String::new(),
            1 => format!("1 item · {}", share::human_size(total)),
            _ => format!("{n} items · {}", share::human_size(total)),
        }));
        b.set_send_sharing(self.send.server.is_some());
        b.set_send_preparing(self.send.preparing);
        b.set_send_status(s(self.send.status.clone()));
        b.set_send_url(s(self.send.server.as_ref().map(|s| shown_url(&s.url)).unwrap_or_default()));
        b.set_send_ip(s(if mock() { "192.168.x.x".into() } else { share::lan_ip().unwrap_or_default() }));
        self.refresh_send_left();
    }
}

/// WINKLET_DEMO_SHARE_MOCK (screenshots): show an obviously fake address and link
/// instead of the real ones, so no real link ends up in a picture.
fn mock() -> bool {
    std::env::var_os("WINKLET_DEMO_SHARE_MOCK").is_some()
}

fn shown_url(url: &str) -> String {
    if mock() { "http://192.168.x.x:49152/demo-link".into() } else { url.to_string() }
}

/// QR code as a crisp black-on-white image (2-module quiet zone, 8 px per module).
fn qr_image(url: &str) -> Image {
    let Ok(code) = qrcode::QrCode::new(url.as_bytes()) else { return Image::default() };
    let n = code.width();
    let dark: Vec<bool> = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    let (quiet, px) = (2usize, 8usize);
    let side = (n + quiet * 2) * px;
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(side as u32, side as u32);
    let pixels = buf.make_mut_slice();
    for y in 0..side {
        for x in 0..side {
            let (mx, my) = ((x / px) as isize - quiet as isize, (y / px) as isize - quiet as isize);
            let on = mx >= 0 && my >= 0 && (mx as usize) < n && (my as usize) < n && dark[my as usize * n + mx as usize];
            let v = if on { 14 } else { 255 };
            pixels[y * side + x] = Rgba8Pixel { r: v, g: v, b: v + if on { 6 } else { 0 }, a: 255 };
        }
    }
    Image::from_rgba8(buf)
}

/// "Files" / "Folder" buttons: pick and add. The dialog is modal, so the app isn't borrowed meanwhile.
pub fn send_pick(shared: &Shared, folders: bool) {
    let hwnd = {
        let mut a = shared.borrow_mut();
        a.hovering = true;
        a.collapse_timer.stop();
        a.hwnd
    };
    let title = if folders { "Folders to send" } else { "Files to send" };
    let picked = sys::pick_paths_titled(hwnd, folders, title);
    let mut a = shared.borrow_mut();
    a.send_add(picked);
    a.hovering = false;
    schedule_collapse(shared, &mut a, 3000);
}
