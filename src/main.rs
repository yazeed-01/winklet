#![windows_subsystem = "windows"]

mod app;
mod audio;
mod callbacks;
mod capture;
mod clip;
mod fg;
mod fps;
mod games_scan;
mod gpu;
mod gpu_info;
mod keys;
mod lenovo;
mod media;
mod pad;
mod ports;
mod share;
mod store;
mod sys;
mod uptime;
mod wall;

use app::{APP, App, CAR_PUFFS, CAR_SKIDS, Focus, SMALL, Shared, Wellbeing, s, schedule_collapse, schedule_shrink, take_screenshot, with_app};
use callbacks::{wire_callbacks, wire_file_drop};
use chrono::{Datelike, Local};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use store::Data;
use windows::Win32::Foundation::HWND;

slint::include_modules!();

thread_local! {
    static MEDIA_SINK: RefCell<Option<Box<dyn Fn(&AppWindow, media::Info)>>> = RefCell::new(None);
}

fn main() {
    // demo runs (WINKLET_DEMO_TAB, used for screenshots) may run next to the real one
    if !sys::single_instance() && std::env::var_os("WINKLET_DEMO_TAB").is_none() {
        return;
    }
    sys::boost_priority();
    // STA: required for file dialogs and OLE drag & drop (winit registers the drop target).
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }

    // Skia's CPU rasterizer: measured lighter (≈50 MB vs ≈100 MB) and faster while animating than
    // FemtoVG on OpenGL, with the same look. SLINT_BACKEND still overrides it (for testing).
    if std::env::var_os("SLINT_BACKEND").is_none() {
        let _ = slint::BackendSelector::new().backend_name("winit".into()).renderer_name("skia-software".into()).select();
    }
    let ui = AppWindow::new().expect("create window");
    let mut data = Data::load();
    if data.settings.island_home_on_start {
        data.settings.island_pos = None;
    }

    // Start off-screen; the real placement happens once the native window exists.
    ui.window().set_size(slint::LogicalSize::new(SMALL.0, SMALL.1));
    ui.window().set_position(slint::PhysicalPosition::new(-10000, -10000));
    ui.show().expect("show window");

    let want_pos = Arc::new(AtomicBool::new(false));
    let weak = ui.as_weak();
    let media_tx = media::spawn(want_pos.clone(), move |info| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                MEDIA_SINK.with(|f| {
                    if let Some(f) = f.borrow().as_ref() {
                        f(&ui, info)
                    }
                });
            }
        });
    });

    let today = Local::now().date_naive();
    let mut app = App {
        ui: ui.as_weak(),
        hwnd: HWND::default(),
        prev_fg: HWND::default(),
        hovering: false,
        hidden: false,
        focus: Focus::default(),
        cal_month: today.with_day(1).unwrap(),
        cal_selected: today,
        last_drink: Instant::now(),
        plugged: None,
        collapsed_at: Some(Instant::now()),
        trimmed: false,
        last_track: String::new(),
        last_minute: 99,
        ticks: 0,
        want_pos,
        media_tx,
        hover_timer: Timer::default(),
        collapse_timer: Timer::default(),
        shrink_timer: Timer::default(),
        peek_timer: Timer::default(),
        toast_timer: Timer::default(),
        audio: audio::Audio::new(),
        app_volumes: Rc::new(VecModel::default()),
        mic_test: None,
        mic_timer: Timer::default(),
        mic_test_started: Instant::now(),
        last_shot: None,
        cur_size: (0.0, 0.0),
        car_room: true,
        car: None,
        car_timer: Timer::default(),
        last_car_show: None,
        skids: Rc::new(VecModel::from(vec![Skid { x: 0.0, y: 0.0, angle: 0.0, alpha: 0.0 }; CAR_SKIDS])),
        puffs: Rc::new(VecModel::from(vec![Puff { x: 0.0, y: 0.0, size: 0.0, alpha: 0.0 }; CAR_PUFFS])),
        rng: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7) | 1,
        eye_next: Instant::now() + Duration::from_secs(6),
        eye_end: None,
        pill_car_timer: Timer::default(),
        pill_car_start: Instant::now(),
        well: Wellbeing::default(),
        pc_on: Default::default(),
        pc_on_at: None,
        coffee_day: None,
        ports_system: false,
        net_prev: None,
        dash_swept: false,
        drag_from: None,
        drag_timer: Timer::default(),
        moving: false,
        move_pressed: false,
        prev_gear: data.settings.gear,
        gear_muted_mic: false,
        dev_ws_sel: String::new(),
        screen: app::ScreenTrack::load(),
        play: Default::default(),
        send: Default::default(),
        power: None,
        wall: Default::default(),
        clips: app::Clips::load(data.settings.clip_paused),
        data,
    };
    ui.global::<Bridge>().set_app_volumes(ModelRc::from(app.app_volumes.clone()));
    ui.global::<Bridge>().set_skids(ModelRc::from(app.skids.clone()));
    ui.global::<Bridge>().set_puffs(ModelRc::from(app.puffs.clone()));
    app.refresh_mic();
    app.refresh_shelf();
    sys::migrate_autostart();
    app.data.settings.autostart = sys::autostart_enabled();
    app.focus_reset_to(false);
    app.refresh_all();
    let app: Shared = Rc::new(RefCell::new(app));

    // winit creates the HWND lazily once the event loop runs; poll briefly for it.
    init_window(app.clone());

    {
        let app = app.clone();
        // WINKLET_DEMO_NO_MEDIA ignores whatever is playing (screenshots)
        if std::env::var_os("WINKLET_DEMO_NO_MEDIA").is_none() {
            MEDIA_SINK.with(move |f| {
                *f.borrow_mut() = Some(Box::new(move |_ui: &AppWindow, info| app.borrow_mut().on_media(info)));
            });
        }
    }

    APP.with(|a| *a.borrow_mut() = Rc::downgrade(&app));
    app::start_watch(app.borrow().clips.paused.clone());
    wire_callbacks(&ui, &app);
    wire_file_drop(&ui);

    {
        let mut a = app.borrow_mut();
        let m = a.data.settings.perf_mode;
        a.set_perf_mode(m);
    }
    lenovo::watch(|mode| {
        let _ = slint::invoke_from_event_loop(move || with_app(|s| s.borrow_mut().set_perf_mode(mode)));
    });

    sys::spawn_hotkeys(|id| {
        app::trace(&format!("hotkey {id}"));
        let _ = slint::invoke_from_event_loop(move || match id {
            sys::HOTKEY_SCREENSHOT => with_app(take_screenshot),
            sys::HOTKEY_MIC => with_app(|s| s.borrow_mut().toggle_mic()),
            sys::HOTKEY_GEAR_UP => with_app(|s| s.borrow_mut().shift_up()),
            sys::HOTKEY_GEAR_DOWN => with_app(|s| s.borrow_mut().shift_down()),
            sys::HOTKEY_GAME => with_app(|s| {
                let mut a = s.borrow_mut();
                let on = !a.data.settings.game_mode;
                a.set_game_mode(on, false);
            }),
            // Ctrl+Alt+V: the clipboard history
            sys::HOTKEY_CLIP => with_app(|s| {
                let mut a = s.borrow_mut();
                let ui = a.ui();
                let b = ui.global::<Bridge>();
                if b.get_tab() != app::TAB_CLIP {
                    b.set_tab_dir(1);
                    b.set_tab(app::TAB_CLIP);
                }
                if b.get_expanded() {
                    a.refresh_clips();
                } else {
                    a.expand();
                }
                schedule_collapse(s, &mut a, 8000);
            }),
            sys::HOTKEY_ISLAND => with_app(|s| {
                let mut a = s.borrow_mut();
                if a.ui().global::<Bridge>().get_expanded() {
                    a.collapse();
                    schedule_shrink(s, &mut a);
                } else {
                    a.expand();
                    schedule_collapse(s, &mut a, 6000);
                }
            }),
            _ => {}
        });
    });

    let heartbeat = Timer::default();
    {
        let app = app.clone();
        heartbeat.start(TimerMode::Repeated, Duration::from_secs(1), move || app.borrow_mut().tick());
    }

    let tray = Tray::new().ok();
    if let Some(tray) = &tray {
        let a = app.clone();
        tray.on_open_island(move || {
            let mut a2 = a.borrow_mut();
            a2.expand();
            schedule_collapse(&a, &mut a2, 5000);
        });
        let a = app.clone();
        tray.on_quit(move || {
            a.borrow().data.save();
            let _ = slint::quit_event_loop();
        });
    }

    slint::run_event_loop_until_quit().expect("event loop");
    app.borrow().data.save();
    app.borrow().screen.log.save();
    fps::stop();
}

fn init_window(app: Shared) {
    Timer::single_shot(Duration::from_millis(15), move || {
        let mut a = app.borrow_mut();
        let Some(hwnd) = sys::hwnd_of(a.ui().window()) else {
            drop(a);
            return init_window(app);
        };
        a.hwnd = hwnd;
        sys::make_tool_window(hwnd);
        sys::set_visible(hwnd, false);
        a.cur_size = (0.0, 0.0); // force resize(false) to apply
        a.resize(false);
        sys::set_visible(hwnd, true);
        a.check_system();
        // game mode was on when Winklet last ran
        if a.data.settings.game_mode {
            a.data.settings.game_mode = false;
            a.set_game_mode(true, true);
            a.play.auto_on = false;
        }
        // WINKLET_DEMO_GAME turns game mode on (without touching the Windows power mode)
        if std::env::var_os("WINKLET_DEMO_GAME").is_some() && !a.data.settings.game_mode {
            a.data.settings.game_power = false;
            a.set_game_mode(true, true);
            a.ui().global::<Bridge>().set_peeking(false);
        }
        // WINKLET_DEMO_TAB=n keeps the island open on tab n (handy for screenshots).
        if let Some(tab) = std::env::var("WINKLET_DEMO_TAB").ok().and_then(|t| t.parse().ok()) {
            a.ui().global::<Bridge>().set_tab(tab);
            a.hovering = true;
            // WINKLET_DEMO_PLAYLISTS opens the playlist grid on Home
            if std::env::var_os("WINKLET_DEMO_PLAYLISTS").is_some() {
                a.ui().global::<Bridge>().set_show_playlists(true);
            }
            // WINKLET_DEMO_VOLUME opens the volume slider on the Home media card
            if std::env::var_os("WINKLET_DEMO_VOLUME").is_some() {
                a.refresh_master();
                a.ui().global::<Bridge>().set_home_volume_open(true);
            }
            // WINKLET_DEMO_VIEW=n picks the sub-view (Health, Developer and Game tabs)
            if let Some(v) = std::env::var("WINKLET_DEMO_VIEW").ok().and_then(|v| v.parse::<i32>().ok()) {
                let ui = a.ui();
                let b = ui.global::<Bridge>();
                b.set_health_view(v);
                b.set_dev_view(v);
                b.set_game_view(v);
                b.set_settings_view(v);
            }
            // WINKLET_DEMO_DRINK=name opens the "New drink" editor (after the Water page exists)
            if let Ok(name) = std::env::var("WINKLET_DEMO_DRINK") {
                slint::Timer::single_shot(std::time::Duration::from_millis(1500), move || {
                    app::with_app(|s| {
                        let mut a = s.borrow_mut();
                        a.drink_edit("");
                        let ui = a.ui();
                        let b = ui.global::<Bridge>();
                        b.set_drink_edit_name(name.as_str().into());
                        b.set_drink_edit_color(6);
                        b.set_drink_edit_ml(300);
                    })
                });
            }
            // WINKLET_DEMO_SEND=path;path fills Shelf > Send (add WINKLET_DEMO_SEND_START to open the link)
            if let Ok(paths) = std::env::var("WINKLET_DEMO_SEND") {
                a.ui().global::<Bridge>().set_shelf_view(1);
                a.send_add(paths.split(';').filter(|p| !p.is_empty()).map(std::path::PathBuf::from));
                if std::env::var_os("WINKLET_DEMO_SEND_START").is_some() {
                    a.send_start();
                }
            }
            // WINKLET_DEMO_THEME=n shows theme n; WINKLET_DEMO_THEMES opens the theme picker
            if let Some(t) = std::env::var("WINKLET_DEMO_THEME").ok().and_then(|t| t.parse::<i32>().ok()) {
                a.ui().global::<Bridge>().set_theme(t);
            }
            if std::env::var_os("WINKLET_DEMO_THEMES").is_some() {
                slint::Timer::single_shot(std::time::Duration::from_millis(1200), || {
                    app::with_app(|s| s.borrow().ui().global::<Bridge>().set_theme_picking(true))
                });
            }
            // WINKLET_DEMO_SIZES=drink-id opens the cup-size sheet for that drink
            if let Ok(id) = std::env::var("WINKLET_DEMO_SIZES") {
                slint::Timer::single_shot(std::time::Duration::from_millis(1500), move || {
                    app::with_app(|s| s.borrow_mut().drink_pick(&id))
                });
            }
            // WINKLET_DEMO_POWER shows a planned shutdown on the Power tab (the UI only: nothing is planned)
            if std::env::var_os("WINKLET_DEMO_POWER").is_some() {
                slint::Timer::single_shot(std::time::Duration::from_millis(800), || {
                    app::with_app(|s| {
                        let a = s.borrow();
                        let ui = a.ui();
                        let b = ui.global::<Bridge>();
                        b.set_power_pending(4);
                        b.set_power_left(app::s("44:12"));
                        b.set_power_at(app::s(if a.data.settings.clock_24h { "23:30" } else { "11:30 PM" }));
                        b.set_power_delay(45);
                    })
                });
            }
            // WINKLET_DEMO_DECK_EDIT=slot opens the Deck key editor (WINKLET_DEMO_DECK_EDITING: edit mode)
            if let Some(slot) = std::env::var("WINKLET_DEMO_DECK_EDIT").ok().and_then(|v| v.parse::<i32>().ok()) {
                slint::Timer::single_shot(std::time::Duration::from_millis(900), move || {
                    app::with_app(|s| s.borrow_mut().deck_edit(slot))
                });
            }
            if std::env::var_os("WINKLET_DEMO_DECK_EDITING").is_some() {
                a.ui().global::<Bridge>().set_deck_editing(true);
            }
            // WINKLET_DEMO_CLIP_FIND=word types it into the Clipboard preview's find bar (WINKLET_DEMO_CLIP_SEL=id picks the item)
            if let Ok(q) = std::env::var("WINKLET_DEMO_CLIP_FIND") {
                slint::Timer::single_shot(std::time::Duration::from_millis(900), move || {
                    app::with_app(|s| {
                        let mut a = s.borrow_mut();
                        if let Ok(id) = std::env::var("WINKLET_DEMO_CLIP_SEL") {
                            a.clip_select(&id);
                        }
                        a.ui().global::<Bridge>().set_clip_find_query(q.as_str().into());
                        a.clip_find(&q);
                    })
                });
            }
            if std::env::var_os("WINKLET_DEMO_HISTORY").is_some() {
                a.ui().global::<Bridge>().set_health_view(3);
            }
            if std::env::var_os("WINKLET_DEMO_PORTS").is_some() {
                a.ui().global::<Bridge>().set_dev_view(1);
            }
            if let Some(m) = std::env::var("WINKLET_DEMO_DASH").ok().and_then(|m| m.parse::<i32>().ok()) {
                a.ui().global::<Bridge>().set_dev_view(2);
                a.ui().global::<Bridge>().set_perf_mode(m);
            }
            a.expand();
        }
        // WINKLET_DEMO_CYCLE=ms walks through every tab and opens/closes the island over and over
        // (a soak test for memory leaks)
        if let Some(ms) = std::env::var("WINKLET_DEMO_CYCLE").ok().and_then(|v| v.parse::<u64>().ok()) {
            let n = std::rc::Rc::new(std::cell::Cell::new(0i32));
            let t = Box::leak(Box::new(Timer::default()));
            t.start(TimerMode::Repeated, Duration::from_millis(ms), move || {
                let i = n.get();
                n.set(i + 1);
                with_app(|s| {
                    let mut a = s.borrow_mut();
                    let ui = a.ui();
                    let b = ui.global::<Bridge>();
                    match i % 19 {
                        17 => a.collapse(),
                        18 => a.expand(),
                        tab => {
                            b.set_tab(tab);
                            a.refresh_tab(tab);
                        }
                    }
                });
            });
        }
        // WINKLET_DEMO_TIP=text shows the tooltip under the 5th tab (screenshots)
        if let Ok(text) = std::env::var("WINKLET_DEMO_TIP") {
            let ui = a.ui();
            let tip = ui.global::<Tip>();
            tip.set_text(s(text));
            tip.set_ax(14.0 + 4.0 * 36.0 + 20.0);
            tip.set_ay(10.0);
            tip.set_aw(34.0);
            tip.set_ah(30.0);
            tip.set_shown(true);
        }
        // WINKLET_DEMO_MOOD=n holds an eye mood (screenshots)
        if let Some(m) = std::env::var("WINKLET_DEMO_MOOD").ok().and_then(|t| t.parse::<i32>().ok()) {
            a.eye_end = Some(Instant::now() + Duration::from_secs(3600));
            Timer::single_shot(Duration::from_millis(600), move || {
                with_app(|s| s.borrow_mut().eye_event(m, 3600.0));
            });
        }
        // WINKLET_DEMO_STAND=mins fires a stand-up reminder; WINKLET_DEMO_GUIDE runs the stretch guide
        if let Some(m) = std::env::var("WINKLET_DEMO_STAND").ok().and_then(|t| t.parse::<u32>().ok()) {
            a.well.session_s = m * 60;
            a.fire_stand_reminder();
        }
        if std::env::var_os("WINKLET_DEMO_GUIDE").is_some() {
            a.start_guide();
        }
        if std::env::var_os("WINKLET_DEMO_PEEK").is_some() {
            a.peek(3, "Time for some water", "750 / 2500 ml today", 30);
        }
    });
}
