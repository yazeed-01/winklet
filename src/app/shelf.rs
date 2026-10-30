//! The shelf: pinned files and folders.

use super::{App, s};
use crate::{Bridge, ShelfItem, store};
use slint::{ComponentHandle, ModelRc, VecModel};

impl App {
    pub fn refresh_shelf(&self) {
        let ui = self.ui();
        let items: Vec<ShelfItem> = self
            .data
            .shelf
            .iter()
            .map(|it| {
                let p = std::path::Path::new(&it.path);
                let is_dir = p.is_dir();
                let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| it.path.clone());
                ShelfItem {
                    id: s(it.id.clone()),
                    name: s(name),
                    path: s(it.path.clone()),
                    icon: s(file_glyph(is_dir, &ext)),
                    ext: s(ext),
                    is_dir,
                    missing: !p.exists(),
                }
            })
            .collect();
        ui.global::<Bridge>().set_shelf(ModelRc::new(VecModel::from(items)));
    }

    pub fn shelf_add(&mut self, paths: impl IntoIterator<Item = std::path::PathBuf>) -> usize {
        let mut added = 0;
        for p in paths {
            let path = p.to_string_lossy().to_string();
            if !path.is_empty() && !self.data.shelf.iter().any(|x| x.path.eq_ignore_ascii_case(&path)) {
                self.data.shelf.push(store::ShelfItem { id: store::new_id(), path });
                added += 1;
            }
        }
        if added > 0 {
            self.data.save();
            self.refresh_shelf();
        }
        added
    }

    pub fn shelf_path(&self, id: &str) -> Option<String> {
        self.data.shelf.iter().find(|x| x.id == id).map(|x| x.path.clone())
    }
}

fn file_glyph(is_dir: bool, ext: &str) -> &'static str {
    if is_dir {
        return "\u{E8B7}";
    }
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "heic" => "\u{EB9F}",
        "mp4" | "mkv" | "mov" | "avi" | "webm" => "\u{E714}",
        "mp3" | "wav" | "flac" | "m4a" | "ogg" => "\u{E8D6}",
        "zip" | "rar" | "7z" | "tar" | "gz" | "apk" | "aab" | "ipa" => "\u{E7B8}",
        "rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "json" | "html" | "css" | "c" | "cpp" | "h" | "java" | "kt"
        | "swift" | "go" | "sh" | "ps1" | "md" | "yml" | "yaml" | "toml" | "xml" | "gradle" | "env" => "\u{E943}",
        "exe" | "msi" | "lnk" | "bat" | "cmd" | "url" => "\u{E71D}",
        _ => "\u{E8A5}",
    }
}
