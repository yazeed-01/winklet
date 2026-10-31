//! The Clipboard tab: everything you copied (text, links, colours, pictures and files),
//! searchable, with a preview that has its own find bar. Pick an item to copy it again or
//! paste it straight into the app you were using. Kept in %APPDATA%\Winklet\clips.json
//! with pictures as PNGs in the clips folder.

use super::{App, s, schedule_shrink, with_app};
use crate::{Bridge, ClipItem, ClipLine, clip, keys, sys};
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use slint::{Color, ComponentHandle, Image, ModelRc, Timer, VecModel};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const TAB_CLIP: i32 = 16;
/// unpinned items kept; older ones are forgotten
const KEEP: usize = 300;
/// the preview shows at most this many wrapped lines
const MAX_ROWS: usize = 6000;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct ClipEntry {
    pub id: String,
    /// "text", "files" or "image"
    pub kind: String,
    pub text: String,
    pub files: Vec<String>,
    pub w: u32,
    pub h: u32,
    pub hash: u64,
    pub at: Option<NaiveDateTime>,
    pub pinned: bool,
}

#[derive(Default)]
pub struct Clips {
    pub entries: Vec<ClipEntry>,
    pub sel: String,
    query: String,
    filter: i32,
    find: String,
    find_idx: usize,
    /// lines with a match in the preview
    find_hits: usize,
    /// characters per preview line, measured by the page
    cols: usize,
    /// the picture shown in the preview (id, image)
    image: Option<(String, Image)>,
    pub paused: Arc<AtomicBool>,
}

fn file() -> std::path::PathBuf {
    crate::store::data_dir().join("clips.json")
}

impl Clips {
    pub fn load(paused: bool) -> Self {
        let entries = std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { entries, cols: 48, paused: Arc::new(AtomicBool::new(paused)), ..Default::default() }
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_vec(&self.entries) {
            let tmp = file().with_extension("tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, file());
            }
        }
    }
}

/// What a text really is: a link, a colour, or just text.
fn text_kind(t: &str) -> &'static str {
    let t = t.trim();
    if !t.contains(char::is_whitespace) && (t.starts_with("http://") || t.starts_with("https://") || t.starts_with("www.")) {
        return "link";
    }
    if parse_color(t).is_some() {
        return "color";
    }
    "text"
}

/// "#3b82f6", "#fff" or "rgb(59, 130, 246)".
fn parse_color(t: &str) -> Option<Color> {
    let t = t.trim();
    if let Some(hex) = t.strip_prefix('#') {
        let full = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => hex.to_string(),
            _ => return None,
        };
        let v = u32::from_str_radix(&full, 16).ok()?;
        return Some(Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8));
    }
    let inner = t.strip_prefix("rgb(").or_else(|| t.strip_prefix("rgba("))?.strip_suffix(')')?;
    let n: Vec<u8> = inner.split(',').take(3).map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    (n.len() == 3).then(|| Color::from_rgb_u8(n[0], n[1], n[2]))
}

fn kind_of(e: &ClipEntry) -> &'static str {
    match e.kind.as_str() {
        "files" => "files",
        "image" => "image",
        _ => text_kind(&e.text),
    }
}

fn file_name(p: &str) -> String {
    std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string())
}

fn ago(at: Option<NaiveDateTime>) -> String {
    let Some(at) = at else { return String::new() };
    let secs = (Local::now().naive_local() - at).num_seconds().max(0);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86400 => format!("{} h ago", secs / 3600),
        _ if secs < 7 * 86400 => format!("{} d ago", secs / 86400),
        _ => at.format("%-d %b").to_string(),
    }
}

/// Splits text into lines of at most `cols` characters, breaking at a space when it can.
/// Each row is (number of the real line it starts, or 0 when it continues one; its text).
fn wrap(text: &str, cols: usize) -> Vec<(usize, String)> {
    let cols = cols.max(8);
    let mut rows = Vec::new();
    for (n, line) in text.split('\n').enumerate() {
        let line: Vec<char> = line.trim_end_matches('\r').replace('\t', "    ").chars().collect();
        if line.is_empty() {
            rows.push((n + 1, String::new()));
        }
        let mut start = 0;
        while start < line.len() {
            let mut end = (start + cols).min(line.len());
            if end < line.len() {
                if let Some(sp) = line[start..end].iter().rposition(|c| *c == ' ').filter(|&i| i > cols / 3) {
                    end = start + sp + 1;
                }
            }
            rows.push((if start == 0 { n + 1 } else { 0 }, line[start..end].iter().collect()));
            start = end;
        }
        if rows.len() >= MAX_ROWS {
            break;
        }
    }
    rows.truncate(MAX_ROWS);
    rows
}

impl App {
    /// A copy noticed by the watcher.
    pub fn clip_add(&mut self, got: clip::Got) {
        let now = Some(Local::now().naive_local());
        let c = &mut self.clips;
        let same = match &got {
            clip::Got::Text(t) => c.entries.iter().position(|e| e.kind == "text" && e.text == *t),
            clip::Got::Files(f) => c.entries.iter().position(|e| e.kind == "files" && e.files == *f),
            clip::Got::Image { hash, w, h, .. } => {
                c.entries.iter().position(|e| e.kind == "image" && e.hash == *hash && e.w == *w && e.h == *h)
            }
        };
        if let Some(i) = same {
            // copied again: move it to the top
            let mut e = c.entries.remove(i);
            e.at = now;
            c.entries.insert(0, e);
            if let clip::Got::Image { id, .. } = &got {
                let _ = std::fs::remove_file(clip::image_path(id));
                let _ = std::fs::remove_file(clip::thumb_path(id));
            }
        } else {
            let mut e = ClipEntry { id: crate::store::new_id(), at: now, ..Default::default() };
            match got {
                clip::Got::Text(t) => {
                    e.kind = "text".into();
                    e.text = t;
                }
                clip::Got::Files(f) => {
                    e.kind = "files".into();
                    e.files = f;
                }
                clip::Got::Image { id, w, h, hash } => {
                    e.kind = "image".into();
                    e.id = id;
                    (e.w, e.h, e.hash) = (w, h, hash);
                }
            }
            c.entries.insert(0, e);
            // forget the oldest unpinned ones
            let mut unpinned = 0;
            let mut gone = Vec::new();
            c.entries.retain(|e| {
                if e.pinned {
                    return true;
                }
                unpinned += 1;
                let keep = unpinned <= KEEP;
                if !keep && e.kind == "image" {
                    gone.push(e.id.clone());
                }
                keep
            });
            for id in gone {
                let _ = std::fs::remove_file(clip::image_path(&id));
                let _ = std::fs::remove_file(clip::thumb_path(&id));
            }
        }
        c.save();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        if b.get_expanded() && b.get_tab() == TAB_CLIP {
            self.refresh_clips();
        }
    }

    fn clip_visible(&self) -> Vec<&ClipEntry> {
        let q = self.clips.query.to_lowercase();
        let mut list: Vec<&ClipEntry> = self
            .clips
            .entries
            .iter()
            .filter(|e| match self.clips.filter {
                1 => kind_of(e) == "text" || kind_of(e) == "color",
                2 => kind_of(e) == "link",
                3 => e.kind == "image",
                4 => e.kind == "files",
                _ => true,
            })
            .filter(|e| {
                q.is_empty()
                    || e.text.to_lowercase().contains(&q)
                    || e.files.iter().any(|f| f.to_lowercase().contains(&q))
                    || (e.kind == "image" && "image picture screenshot".contains(&q))
            })
            .collect();
        // pinned ones first, each group newest first
        list.sort_by_key(|e| !e.pinned);
        list
    }

    pub fn refresh_clips(&mut self) {
        let visible: Vec<ClipEntry> = self.clip_visible().into_iter().cloned().collect();
        if !visible.iter().any(|e| e.id == self.clips.sel) {
            self.clips.sel = visible.first().map(|e| e.id.clone()).unwrap_or_default();
            self.clips.find_idx = 0;
        }
        let items: Vec<ClipItem> = visible
            .iter()
            .map(|e| {
                let kind = kind_of(e);
                let (title, extra) = match e.kind.as_str() {
                    "files" => {
                        let first = e.files.iter().take(2).map(|f| file_name(f)).collect::<Vec<_>>().join(", ");
                        let more = e.files.len().saturating_sub(2);
                        (if more > 0 { format!("{first} +{more}") } else { first }, format!("{} file{}", e.files.len(), if e.files.len() == 1 { "" } else { "s" }))
                    }
                    "image" => (format!("Image {} × {}", e.w, e.h), String::new()),
                    _ => (
                        e.text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(120).collect(),
                        if kind == "text" { format!("{} chars", e.text.chars().count()) } else { String::new() },
                    ),
                };
                let thumb = if e.kind == "image" { Image::load_from_path(&clip::thumb_path(&e.id)).unwrap_or_default() } else { Image::default() };
                ClipItem {
                    id: s(e.id.clone()),
                    kind: s(kind),
                    title: s(title),
                    meta: s(if extra.is_empty() { ago(e.at) } else { format!("{} · {extra}", ago(e.at)) }),
                    has_thumb: thumb.size().width > 0,
                    thumb,
                    color: parse_color(&e.text).unwrap_or_default(),
                    pinned: e.pinned,
                }
            })
            .collect();
        let total = self.clips.entries.len();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_clips(ModelRc::new(VecModel::from(items)));
        b.set_clip_selected(s(self.clips.sel.clone()));
        b.set_clip_filter(self.clips.filter);
        b.set_clip_count_text(s(match total {
            0 => "Nothing yet".to_string(),
            1 => "1 item".to_string(),
            n => format!("{n} items"),
        }));
        b.set_clip_paused(self.clips.paused.load(Ordering::Relaxed));
        self.refresh_clip_preview();
    }

    /// The right side: the selected item, with find-in-text highlights.
    pub fn refresh_clip_preview(&mut self) {
        let e = self.clips.entries.iter().find(|e| e.id == self.clips.sel).cloned();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let Some(e) = e else {
            b.set_clip_kind(s(""));
            b.set_clip_lines(ModelRc::default());
            return;
        };
        let kind = kind_of(&e);
        let when = e.at.map(|t| t.format(if self.data.settings.clock_24h { "%a %-d %b, %H:%M" } else { "%a %-d %b, %-I:%M %p" }).to_string());
        b.set_clip_kind(s(kind));
        b.set_clip_when(s(when.unwrap_or_default()));
        b.set_clip_pinned(e.pinned);
        let mut find_text = String::new();
        let mut find_row = -1;
        match e.kind.as_str() {
            "image" => {
                b.set_clip_info(s(format!("{} × {} px", e.w, e.h)));
                let img = match &self.clips.image {
                    Some((id, img)) if *id == e.id => img.clone(),
                    _ => {
                        let img = Image::load_from_path(&clip::image_path(&e.id)).unwrap_or_default();
                        self.clips.image = Some((e.id.clone(), img.clone()));
                        img
                    }
                };
                b.set_clip_image(img);
            }
            "files" => {
                b.set_clip_info(s(format!("{} file{}", e.files.len(), if e.files.len() == 1 { "" } else { "s" })));
                b.set_clip_files(ModelRc::new(VecModel::from(e.files.iter().map(|f| s(f.clone())).collect::<Vec<_>>())));
            }
            _ => {
                let lines = e.text.lines().count().max(1);
                b.set_clip_info(s(format!("{} characters · {lines} line{}", e.text.chars().count(), if lines == 1 { "" } else { "s" })));
                b.set_clip_color(parse_color(&e.text).unwrap_or_default());
                let q = self.clips.find.to_lowercase();
                let qn = q.chars().count();
                let mut hits = Vec::new();
                let rows: Vec<ClipLine> = wrap(&e.text, self.clips.cols)
                    .into_iter()
                    .enumerate()
                    .map(|(i, (num, text))| {
                        let lower = text.to_lowercase();
                        let at = (qn > 0 && lower.chars().count() == text.chars().count())
                            .then(|| lower.find(&q).map(|byte| lower[..byte].chars().count()))
                            .flatten();
                        let num = s(if num > 0 { num.to_string() } else { String::new() });
                        match at {
                            Some(c) => {
                                hits.push(i);
                                let chars: Vec<char> = text.chars().collect();
                                ClipLine {
                                    num,
                                    pre: s(chars[..c].iter().collect::<String>()),
                                    hit: s(chars[c..c + qn].iter().collect::<String>()),
                                    post: s(chars[c + qn..].iter().collect::<String>()),
                                    has_hit: true,
                                    current: false,
                                }
                            }
                            None => ClipLine { num, pre: s(text), ..Default::default() },
                        }
                    })
                    .collect();
                let mut rows = rows;
                self.clips.find_hits = hits.len();
                if !q.is_empty() {
                    if hits.is_empty() {
                        find_text = "No matches".into();
                    } else {
                        let idx = self.clips.find_idx % hits.len();
                        rows[hits[idx]].current = true;
                        find_row = hits[idx] as i32;
                        find_text = format!("{} / {}", idx + 1, hits.len());
                    }
                }
                b.set_clip_lines(ModelRc::new(VecModel::from(rows)));
            }
        }
        b.set_clip_find_text(s(find_text));
        b.set_clip_find_row(find_row);
    }

    pub fn clip_search(&mut self, q: &str) {
        self.clips.query = q.trim().to_string();
        self.refresh_clips();
    }

    pub fn clip_filter_set(&mut self, f: i32) {
        self.clips.filter = f;
        self.refresh_clips();
    }

    pub fn clip_select(&mut self, id: &str) {
        self.clips.sel = id.to_string();
        self.clips.find_idx = 0;
        self.ui().global::<Bridge>().set_clip_selected(s(id));
        self.refresh_clip_preview();
    }

    pub fn clip_find(&mut self, q: &str) {
        self.clips.find = q.to_string();
        self.clips.find_idx = 0;
        self.refresh_clip_preview();
    }

    pub fn clip_find_step(&mut self, d: i32) {
        let n = self.clips.find_hits.max(1) as i64;
        self.clips.find_idx = (self.clips.find_idx as i64 + d as i64).rem_euclid(n) as usize;
        self.refresh_clip_preview();
    }

    pub fn clip_cols(&mut self, cols: i32) {
        let cols = cols.clamp(8, 400) as usize;
        if cols != self.clips.cols {
            self.clips.cols = cols;
            self.refresh_clip_preview();
        }
    }

    /// Puts the selected item back on the clipboard.
    pub fn clip_copy(&mut self) -> bool {
        let Some(e) = self.clips.entries.iter().find(|e| e.id == self.clips.sel).cloned() else { return false };
        let ok = match e.kind.as_str() {
            "files" => clip::set_files(&e.files),
            "image" => Image::load_from_path(&clip::image_path(&e.id))
                .ok()
                .and_then(|img| img.to_rgba8())
                .is_some_and(|px| clip::set_image(px.width(), px.height(), px.as_bytes())),
            _ => sys::set_clipboard(&e.text),
        };
        self.toast(if ok { "Copied" } else { "Couldn't copy it" });
        ok
    }

    /// Copies the item, steps aside and pastes it into the app you were using.
    pub fn clip_paste(shared: &super::Shared) {
        let mut a = shared.borrow_mut();
        if !a.clip_copy() {
            return;
        }
        a.collapse();
        schedule_shrink(shared, &mut a);
        Timer::single_shot(Duration::from_millis(180), || {
            keys::combo("ctrl+v");
        });
    }

    pub fn clip_selected_text(&self) -> String {
        self.clips.entries.iter().find(|e| e.id == self.clips.sel).map(|e| e.text.clone()).unwrap_or_default()
    }

    pub fn clip_pin(&mut self) {
        if let Some(e) = self.clips.entries.iter_mut().find(|e| e.id == self.clips.sel) {
            e.pinned = !e.pinned;
        }
        self.clips.save();
        self.refresh_clips();
    }

    pub fn clip_delete(&mut self) {
        let id = self.clips.sel.clone();
        if let Some(i) = self.clips.entries.iter().position(|e| e.id == id) {
            let e = self.clips.entries.remove(i);
            if e.kind == "image" {
                let _ = std::fs::remove_file(clip::image_path(&e.id));
                let _ = std::fs::remove_file(clip::thumb_path(&e.id));
            }
        }
        self.clips.save();
        self.refresh_clips();
    }

    /// Forgets everything except pinned items.
    pub fn clip_clear(&mut self) {
        let mut gone = Vec::new();
        self.clips.entries.retain(|e| {
            if !e.pinned && e.kind == "image" {
                gone.push(e.id.clone());
            }
            e.pinned
        });
        for id in gone {
            let _ = std::fs::remove_file(clip::image_path(&id));
            let _ = std::fs::remove_file(clip::thumb_path(&id));
        }
        self.clips.save();
        self.refresh_clips();
        self.toast("Cleared (pinned items stay)");
    }

    pub fn clip_pause_toggle(&mut self) {
        let paused = !self.clips.paused.load(Ordering::Relaxed);
        self.clips.paused.store(paused, Ordering::Relaxed);
        self.data.settings.clip_paused = paused;
        self.data.save();
        self.ui().global::<Bridge>().set_clip_paused(paused);
        self.toast(if paused { "Not saving copies for now" } else { "Saving copies again" });
    }
}

/// Starts the clipboard watcher; copies arrive on the UI thread.
pub fn start_watch(paused: Arc<AtomicBool>) {
    clip::watch(paused, |got| {
        let _ = slint::invoke_from_event_loop(move || with_app(|s| s.borrow_mut().clip_add(got)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_kinds() {
        let rows = wrap("hello world this is a long line\nshort", 12);
        assert_eq!(rows[0], (1, "hello world ".into()));
        assert!(rows.iter().any(|(n, t)| *n == 2 && t == "short"));
        assert_eq!(text_kind("https://example.com/a"), "link");
        assert_eq!(text_kind("#3b82f6"), "color");
        assert_eq!(text_kind("rgb(1, 2, 3)"), "color");
        assert_eq!(text_kind("hello there"), "text");
    }
}
