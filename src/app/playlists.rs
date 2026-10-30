//! Saved playlists on the Home media card: paste a link (Spotify, YouTube Music,
//! Apple Music, Anghami, SoundCloud, …), then one click opens it. Nothing is fetched
//! from the internet; the service is read from the link itself.

use super::{App, s};
use crate::{Bridge, PlaylistItem, store};
use slint::{Color, ComponentHandle, ModelRc, VecModel};

pub struct Service {
    pub name: &'static str,
    /// cover gradient
    pub from: u32,
    pub to: u32,
}

const SERVICES: &[(&str, Service)] = &[
    ("open.spotify.com", Service { name: "Spotify", from: 0x1ed760, to: 0x0a6b31 }),
    ("spotify:", Service { name: "Spotify", from: 0x1ed760, to: 0x0a6b31 }),
    ("music.youtube.com", Service { name: "YouTube Music", from: 0xff4e45, to: 0x8c0d0d }),
    ("youtube.com", Service { name: "YouTube", from: 0xff5252, to: 0x7a1010 }),
    ("youtu.be", Service { name: "YouTube", from: 0xff5252, to: 0x7a1010 }),
    ("music.apple.com", Service { name: "Apple Music", from: 0xfc5c7d, to: 0xa4133c }),
    ("anghami.com", Service { name: "Anghami", from: 0xb57bff, to: 0x5b21b6 }),
    ("soundcloud.com", Service { name: "SoundCloud", from: 0xffa142, to: 0xc2410c }),
    ("deezer.com", Service { name: "Deezer", from: 0xc084fc, to: 0x6d28d9 }),
    ("tidal.com", Service { name: "TIDAL", from: 0x6b7280, to: 0x111827 }),
    ("music.amazon", Service { name: "Amazon Music", from: 0x38e1ea, to: 0x0e7490 }),
];
const OTHER: Service = Service { name: "Link", from: 0xa78bfa, to: 0x3b82f6 };

pub fn service(url: &str) -> &'static Service {
    let u = url.to_lowercase();
    SERVICES.iter().find(|(host, _)| u.contains(host)).map(|(_, s)| s).unwrap_or(&OTHER)
}

/// "Gym mix https://open.spotify.com/playlist/…" → (Some("Gym mix"), link).
/// The name is optional; returns None when there's no link.
pub fn parse(input: &str) -> Option<(Option<String>, String)> {
    let input = input.trim();
    let start = input.find("https://").or_else(|| input.find("http://")).or_else(|| input.find("spotify:"))?;
    let url: String = input[start..].split_whitespace().next()?.to_string();
    let name = input[..start].trim().trim_end_matches(['-', ':', '|', '–']).trim();
    Some(((!name.is_empty()).then(|| name.to_string()), url))
}

/// Spotify links open straight in the Spotify app when it's installed.
fn launch_target(url: &str) -> String {
    let lower = url.to_lowercase();
    if let Some(i) = lower.find("open.spotify.com/") {
        let path = url[i + "open.spotify.com/".len()..].split(['?', '#']).next().unwrap_or("");
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty() && !p.starts_with("intl-")).collect();
        if parts.len() >= 2 && crate::sys::protocol_registered("spotify") {
            return format!("spotify:{}:{}", parts[0], parts[1]);
        }
    }
    url.to_string()
}

fn rgb(c: u32) -> Color {
    Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

impl App {
    pub fn refresh_playlists(&self) {
        let items: Vec<PlaylistItem> = self
            .data
            .playlists
            .iter()
            .map(|p| {
                let sv = service(&p.url);
                PlaylistItem {
                    id: s(p.id.clone()),
                    name: s(p.name.clone()),
                    service: s(sv.name),
                    from: rgb(sv.from),
                    to: rgb(sv.to),
                    url: s(p.url.clone()),
                }
            })
            .collect();
        self.ui().global::<Bridge>().set_playlists(ModelRc::new(VecModel::from(items)));
    }

    pub fn playlist_add(&mut self, input: &str) {
        let Some((name, url)) = parse(input) else {
            self.toast("Paste a playlist link");
            return;
        };
        if self.data.playlists.iter().any(|p| p.url == url) {
            self.toast("Already saved");
            return;
        }
        let sv = service(&url).name;
        let name = name.unwrap_or_else(|| {
            let n = self.data.playlists.iter().filter(|p| service(&p.url).name == sv).count();
            if n == 0 { format!("{sv} playlist") } else { format!("{sv} playlist {}", n + 1) }
        });
        self.data.playlists.insert(0, store::Playlist { id: store::new_id(), name, url });
        self.data.save();
        self.refresh_playlists();
        self.toast("Playlist saved");
    }

    pub fn playlist_open(&mut self, id: &str) {
        let Some(i) = self.data.playlists.iter().position(|p| p.id == id) else { return };
        let target = launch_target(&self.data.playlists[i].url);
        if crate::sys::open(&target, "") {
            // most recently played first
            let p = self.data.playlists.remove(i);
            self.toast(&format!("Opening {}", p.name));
            self.data.playlists.insert(0, p);
            self.data.save();
            self.refresh_playlists();
        } else {
            self.toast("Couldn't open the link");
        }
    }

    /// Drag and drop in the grid.
    pub fn playlist_move(&mut self, from: i32, to: i32) {
        let n = self.data.playlists.len() as i32;
        if from == to || !(0..n).contains(&from) || !(0..n).contains(&to) {
            return;
        }
        let p = self.data.playlists.remove(from as usize);
        self.data.playlists.insert(to as usize, p);
        self.data.save();
        self.refresh_playlists();
    }

    pub fn playlist_remove(&mut self, id: &str) {
        self.data.playlists.retain(|p| p.id != id);
        self.data.save();
        self.refresh_playlists();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_name_and_link() {
        let (n, u) = parse("Gym mix https://open.spotify.com/playlist/37i9dQ?si=1").unwrap();
        assert_eq!(n.as_deref(), Some("Gym mix"));
        assert_eq!(u, "https://open.spotify.com/playlist/37i9dQ?si=1");
        let (n, _) = parse("  https://music.youtube.com/playlist?list=PL1 ").unwrap();
        assert_eq!(n, None);
        assert_eq!(parse("Focus - https://x.y/z").unwrap().0.as_deref(), Some("Focus"));
        assert!(parse("no link here").is_none());
    }

    #[test]
    fn knows_services() {
        assert_eq!(service("https://music.youtube.com/playlist?list=1").name, "YouTube Music");
        assert_eq!(service("https://www.youtube.com/playlist?list=1").name, "YouTube");
        assert_eq!(service("https://play.anghami.com/playlist/123").name, "Anghami");
        assert_eq!(service("spotify:playlist:abc").name, "Spotify");
        assert_eq!(service("https://example.com/mix").name, "Link");
    }
}
