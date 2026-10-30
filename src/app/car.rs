//! The car show: a scripted drift under the island when it opens.

use super::{App, Shared};
use crate::{Bridge, Puff, Skid};
use slint::{ComponentHandle, Model, TimerMode};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub const CAR_SKIDS: usize = 240;
pub const CAR_PUFFS: usize = 34;
/// when the car has left the stage (seconds)
const CAR_END: f32 = 3.25;

pub struct CarShow {
    start: Instant,
    skid_i: usize,
    /// (x, y, born) of each smoke puff
    puffs: Vec<(f32, f32, f32)>,
    frame: u32,
}

struct Pose {
    x: f32,
    y: f32,
    heading: f32,
    slip: f32,
    smoke: bool,
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Scripted drive in stage coordinates (620 x 160): race in from the left, brake
/// into a full donut with the tail hung out, then straighten and blast off to the right.
fn car_pose(t: f32) -> Option<Pose> {
    const CX: f32 = 330.0;
    const CY: f32 = 78.0;
    const R: f32 = 30.0;
    const T_IN: f32 = 0.85;
    const T_DONUT: f32 = 1.45;
    let y0 = CY - R;
    if t < T_IN {
        let u = t / T_IN;
        let e = 0.4 * u + 0.6 * u * (2.0 - u); // brakes into the corner
        // nose dips a touch as weight shifts under braking
        let wobble = 3.0 * (u * std::f32::consts::PI).sin() * u;
        return Some(Pose { x: -70.0 + (CX + 70.0) * e, y: y0, heading: -wobble, slip: 0.0, smoke: u > 0.8 });
    }
    let tb = t - T_IN;
    if tb < T_DONUT {
        let phi = (-90.0 + 360.0 * tb / T_DONUT).to_radians();
        let slip = 62.0 * smoothstep(0.0, 0.22, tb);
        let tangent = phi.to_degrees() + 90.0;
        return Some(Pose { x: CX + R * phi.cos(), y: CY + R * phi.sin(), heading: tangent + slip, slip, smoke: true });
    }
    let tc = tb - T_DONUT;
    let dur = CAR_END - T_IN - T_DONUT;
    if tc < dur {
        let s = smoothstep(0.0, 0.45, tc);
        let slip = 62.0 * (1.0 - s) - 36.0 * s * (1.0 - s); // counter-steer overshoot
        let u = tc / dur;
        return Some(Pose { x: CX + 560.0 * u * u, y: y0, heading: 360.0 + slip, slip, smoke: tc < 0.45 });
    }
    None
}

impl App {
    pub fn start_car_show(shared: &Shared) {
        let mut a = shared.borrow_mut();
        a.car = Some(CarShow { start: Instant::now(), skid_i: 0, puffs: Vec::new(), frame: 0 });
        a.last_car_show = Some(Instant::now());
        for i in 0..CAR_SKIDS {
            a.skids.set_row_data(i, Skid { x: 0.0, y: 0.0, angle: 0.0, alpha: 0.0 });
        }
        let big = a.ui().global::<Bridge>().get_expanded();
        a.resize(big);
        let ui = a.ui();
        let b = ui.global::<Bridge>();
        b.set_skid_fade(1.0);
        b.set_car_active(true);
        let weak = Rc::downgrade(shared);
        a.car_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            if let Some(s) = weak.upgrade() {
                s.borrow_mut().car_frame();
            }
        });
    }

    pub fn stop_car_show(&mut self) {
        if self.car.take().is_none() {
            return;
        }
        self.car_timer.stop();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_car_active(false);
        b.set_car_visible(false);
        for i in 0..CAR_PUFFS {
            self.puffs.set_row_data(i, Puff { x: 0.0, y: 0.0, size: 0.0, alpha: 0.0 });
        }
        let big = b.get_expanded() || b.get_peeking();
        self.resize(big);
    }

    pub fn car_frame(&mut self) {
        let Some(t) = self.car.as_ref().map(|c| c.start.elapsed().as_secs_f32()) else { return };
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        match car_pose(t) {
            Some(pose) => {
                b.set_car_visible(true);
                b.set_car_x(pose.x);
                b.set_car_y(pose.y);
                b.set_car_angle(pose.heading);
                if pose.smoke {
                    let h = pose.heading.to_radians();
                    let (f, l) = ((h.cos(), h.sin()), (-h.sin(), h.cos()));
                    let rear = (pose.x - f.0 * 22.0, pose.y - f.1 * 22.0);
                    let car = self.car.as_mut().unwrap();
                    car.frame += 1;
                    let side = if car.frame % 2 == 0 { 1.0 } else { -1.0 };
                    let wheel = (rear.0 + l.0 * 15.0 * side, rear.1 + l.1 * 15.0 * side);
                    // two skid marks per frame (both rear tyres), aligned with the direction of travel
                    let travel = pose.heading - pose.slip;
                    for sgn in [1.0f32, -1.0] {
                        let i = car.skid_i % CAR_SKIDS;
                        car.skid_i += 1;
                        self.skids.set_row_data(i, Skid {
                            x: rear.0 + l.0 * 15.0 * sgn,
                            y: rear.1 + l.1 * 15.0 * sgn,
                            angle: travel,
                            alpha: if pose.slip.abs() > 8.0 { 0.42 } else { 0.25 },
                        });
                    }
                    let jitter = (self.rand() - 0.5) * 8.0;
                    let car = self.car.as_mut().unwrap();
                    car.puffs.push((wheel.0 + jitter, wheel.1 - jitter, t));
                    if car.puffs.len() > CAR_PUFFS {
                        car.puffs.remove(0);
                    }
                }
            }
            None => {
                b.set_car_visible(false);
                b.set_skid_fade(0.0);
            }
        }
        // smoke: grows, rises a little and fades over ~1 s
        let puffs = self.car.as_ref().map(|c| c.puffs.clone()).unwrap_or_default();
        for i in 0..CAR_PUFFS {
            let p = match puffs.get(i) {
                Some(&(x, y, born)) => {
                    let age = (t - born).max(0.0);
                    let life = (1.0 - age / 1.1).max(0.0);
                    Puff { x: x - age * 10.0, y: y - age * 14.0, size: 10.0 + age * 52.0, alpha: 0.55 * life * life }
                }
                None => Puff { x: 0.0, y: 0.0, size: 0.0, alpha: 0.0 },
            };
            self.puffs.set_row_data(i, p);
        }
        if t > CAR_END + 1.1 {
            self.stop_car_show();
        }
    }
}
