//! Developer tab: system stats, the port list and the car-style dashboard.

use super::{App, s, with_app};
use crate::{Bridge, PortItem, gpu, ports, sys};
use chrono::Local;
use slint::{ComponentHandle, ModelRc, Timer, VecModel};
use std::time::{Duration, Instant};

impl App {
    /// Car-style cluster: CPU tachometer, GPU speedometer, RAM "temperature", two small
    /// network dials (down / up), odometer = today's screen time. Updated once a second, only while visible.
    pub fn refresh_dash(&mut self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let cpu = sys::cpu_usage();
        let (gpu, vram) = gpu::usage();
        let (ram, used, total) = sys::memory();
        let (rx, tx) = sys::net_bytes();
        let now = Instant::now();
        let (down, up) = match self.net_prev {
            Some((t, prx, ptx)) => {
                let dt = t.elapsed().as_secs_f64().max(0.2);
                (rx.saturating_sub(prx) as f64 * 8.0 / 1e6 / dt, tx.saturating_sub(ptx) as f64 * 8.0 / 1e6 / dt)
            }
            None => (0.0, 0.0),
        };
        self.net_prev = Some((now, rx, tx));
        // small network dials: 0 1 10 100 1G Mbps, evenly spaced
        const KNOTS: [f64; 5] = [0.0, 1.0, 10.0, 100.0, 1000.0];
        let pos = |mbps: f64| {
            KNOTS
                .windows(2)
                .enumerate()
                .find(|(_, w)| mbps <= w[1])
                .map(|(i, w)| (i as f64 + (mbps - w[0]) / (w[1] - w[0])) / (KNOTS.len() - 1) as f64)
                .unwrap_or(1.0) as f32
        };
        let fmt = |m: f64| if m >= 100.0 { format!("{m:.0}") } else if m >= 10.0 { format!("{m:.1}") } else { format!("{m:.2}") };
        b.set_dash_cpu(cpu);
        b.set_dash_cpu_text(s(format!("{:.0}%", cpu * 100.0)));
        b.set_dash_ram(ram);
        b.set_dash_ram_text(s(format!("{:.0}%", ram * 100.0)));
        b.set_dash_ram_gb(s(format!("{used:.1} / {total:.0} GB")));
        b.set_dash_gpu(gpu);
        b.set_dash_gpu_text(s(format!("{:.0}%", gpu * 100.0)));
        b.set_dash_gpu_mem(s(if vram > 0.05 { format!("{vram:.1} GB VRAM") } else { "load".to_string() }));
        b.set_dash_net(pos(down));
        b.set_dash_up(pos(up));
        b.set_dash_net_text(s(fmt(down)));
        b.set_dash_up_text(s(fmt(up)));
        let active = self.data.health.get(&Local::now().date_naive()).map(|h| h.active_s).unwrap_or(0);
        b.set_dash_odo(s(format!("{:02}h{:02}m", active / 3600, active / 60 % 60)));
    }

    /// Needles swing to the end stop and back the first time the dash opens, like turning the key.
    pub fn ignition_sweep(&mut self) {
        sys::cpu_usage();
        gpu::usage(); // prime the rate counters
        self.net_prev = None;
        self.refresh_dash();
        if self.dash_swept {
            return;
        }
        self.dash_swept = true;
        self.ui().global::<Bridge>().set_dash_sweep(true);
        Timer::single_shot(Duration::from_millis(750), || {
            with_app(|s| s.borrow().ui().global::<Bridge>().set_dash_sweep(false))
        });
    }

    pub fn set_perf_mode(&mut self, mode: i32) {
        if !(1..=3).contains(&mode) {
            return;
        }
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_perf_mode(mode);
        b.set_has_perf_mode(true);
        if self.data.settings.perf_mode != mode {
            self.data.settings.perf_mode = mode;
            self.data.save();
        }
    }

    pub fn refresh_ports(&self) {
        let all = ports::listeners();
        let hidden = all.iter().filter(|l| l.system).count() as i32;
        let items: Vec<PortItem> = all
            .into_iter()
            .filter(|l| self.ports_system || !l.system)
            .map(|l| PortItem {
                port: l.port as i32,
                label: s(ports::label(l.port)),
                name: s(l.name),
                pid: l.pid as i32,
                addr: s(l.addr),
                system: l.system,
            })
            .collect();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_ports(ModelRc::new(VecModel::from(items)));
        b.set_ports_hidden(hidden);
        b.set_ports_system(self.ports_system);
    }

    pub fn refresh_dev(&self) {
        self.refresh_dev_live();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_ip_text(s(sys::local_ip()));
        b.set_projects_dir(s(super::folder_for(self, "projects")));
        b.set_downloads_dir(s(super::folder_for(self, "downloads")));
    }

    /// Just the live numbers (every 2 s while Developer > Tools is open). Rounded and set only
    /// when they change, since every change animates the bars.
    pub fn refresh_dev_live(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let (load, used, total) = sys::memory();
        let cpu = (sys::cpu_usage() * 100.0).round() / 100.0;
        let ram = (load * 200.0).round() / 200.0;
        if b.get_cpu() != cpu {
            b.set_cpu(cpu);
        }
        if b.get_ram() != ram {
            b.set_ram(ram);
        }
        let ram_text = format!("{used:.1} / {total:.0} GB");
        if b.get_ram_text() != ram_text.as_str() {
            b.set_ram_text(s(ram_text));
        }
        let up = sys::uptime();
        if b.get_uptime() != up.as_str() {
            b.set_uptime(s(up));
        }
    }
}
