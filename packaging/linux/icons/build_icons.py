"""Builds ui/fonts/winklet-icons.ttf: the Segoe Fluent Icons code points the UI uses, drawn with
Microsoft's open-source Fluent UI System Icons (MIT), for systems without Segoe (Linux).

    pip install fonttools
    python packaging/linux/icons/build_icons.py <FluentSystemIcons-Regular.ttf> <FluentSystemIcons-Regular.json>

Both inputs come from https://github.com/microsoft/fluentui-system-icons/tree/main/fonts.
"""

import json
import sys
from pathlib import Path

from fontTools.subset import Options, Subsetter
from fontTools.ttLib import TTFont

# Segoe Fluent Icons code point -> Fluent UI System Icons name (ic_fluent_<name>_<size>_regular)
MAP = {
    0xE708: "weather_moon",
    0xE70D: "chevron_down",
    0xE70E: "chevron_up",
    0xE70F: "edit",
    0xE710: "add",
    0xE711: "dismiss",
    0xE712: "more_horizontal",
    0xE713: "settings",
    0xE714: "video",
    0xE715: "mail",
    0xE718: "pin",
    0xE71D: "apps",
    0xE720: "mic",
    0xE721: "search",
    0xE722: "camera",
    0xE724: "send",
    0xE72C: "arrow_clockwise",
    0xE72E: "lock_closed",
    0xE734: "star",
    0xE735: "star",
    0xE738: "subtract",
    0xE73E: "checkmark",
    0xE74D: "delete",
    0xE74F: "speaker_mute",
    0xE753: "cloud",
    0xE756: "window_console",
    0xE765: "keyboard",
    0xE767: "speaker_2",
    0xE768: "play",
    0xE769: "pause",
    0xE76B: "chevron_left",
    0xE76C: "chevron_right",
    0xE771: "paint_brush",
    0xE774: "globe",
    0xE777: "arrow_counterclockwise",
    0xE77F: "clipboard",
    0xE786: "slide_multiple",
    0xE787: "calendar_ltr",
    0xE790: "color",
    0xE7A7: "arrow_undo",
    0xE7A8: "crop",
    0xE7B3: "eye",
    0xE7B5: "rocket",
    0xE7B8: "box",
    0xE7C2: "arrow_move",
    0xE7E8: "power",
    0xE7F4: "desktop",
    0xE7F6: "headphones",
    0xE7FC: "games",
    0xE804: "vehicle_car",
    0xE805: "person_walking",
    0xE80F: "home",
    0xE838: "folder_open",
    0xE892: "previous",
    0xE893: "next",
    0xE896: "arrow_download",
    0xE8A1: "split_vertical",
    0xE8A5: "document",
    0xE8A9: "grid",
    0xE8B7: "folder",
    0xE8BD: "chat",
    0xE8C8: "copy",
    0xE8D2: "text_font",
    0xE8D6: "music_note_2",
    0xE8D7: "key",
    0xE916: "timer",
    0xE917: "clock",
    0xE943: "code",
    0xE945: "flash",
    0xE950: "developer_board",
    0xE993: "speaker_1",
    0xE995: "speaker_2",
    0xE9D2: "data_area",
    0xE9D5: "task_list_ltr",
    0xE9D9: "pulse",
    0xEA8F: "alert",
    0xEB51: "heart",
    0xEB9F: "image",
    0xEC54: "mic_off",
    0xED14: "qr_code",
    0xF0E2: "grid",
    0xF3B1: "sign_out",
}

SIZES = ["24", "20", "28", "16", "32", "48"]


def main(ttf: str, names_json: str) -> None:
    names = json.loads(Path(names_json).read_text())
    font = TTFont(ttf)
    cmap = font.getBestCmap()
    picked = {}
    missing = []
    for cp, name in MAP.items():
        src = next((names[k] for s in SIZES if (k := f"ic_fluent_{name}_{s}_regular") in names), None)
        if src is None or src not in cmap:
            missing.append(f"{cp:04X} {name}")
            continue
        picked[cp] = cmap[src]
    if missing:
        sys.exit("not in the font: " + ", ".join(missing))

    # keep only the glyphs we use, then point the Segoe code points at them
    opts = Options()
    opts.glyph_names = True
    opts.notdef_outline = True
    opts.name_IDs = ["*"]
    sub = Subsetter(opts)
    sub.populate(glyphs=sorted(set(picked.values())))
    sub.subset(font)
    for table in font["cmap"].tables:
        if table.isUnicode():
            table.cmap = dict(picked)
    family = "Winklet Icons"
    for rec in font["name"].names:
        if rec.nameID in (1, 16):
            rec.string = family
        elif rec.nameID == 4:
            rec.string = family
        elif rec.nameID == 6:
            rec.string = "WinkletIcons-Regular"
    out = Path(__file__).resolve().parents[3] / "ui" / "fonts" / "winklet-icons.ttf"
    out.parent.mkdir(parents=True, exist_ok=True)
    font.save(out)
    print(f"{out} ({out.stat().st_size // 1024} KB, {len(picked)} icons)")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
