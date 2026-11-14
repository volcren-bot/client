use std::{
    ffi::c_void,
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use ipc::{
    input::SYNTHETIC,
    state::{
        act_command, act_status, model_status, text, ActRequest, ActState, MenuEntryInfo,
        ACT_BLOCKERS, ACT_OPTIONS,
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_RBUTTONDOWN},
};

use super::{
    hooks::hooked_window,
    menu::{self, Snapshot},
    model,
    state::mapping,
};
use crate::shared::memory::memory_manager;

const MK_LBUTTON: usize = 0x1;
const MK_RBUTTON: usize = 0x2;
const GONE_FRAMES: u32 = 30;
const MIN_FACE_AREA: f32 = 0.5;
const SPOT_SPREAD: f32 = 0.12;
const SPOT_TRIES: usize = 16;
const FOLLOW_LAG_MS: f32 = 40.0;
const SPEED_SMOOTHING: f32 = 0.5;

type Face = [[f32; 2]; 3];

struct Job {
    sequence: u32,
    request: ActRequest,
    started: Instant,
    frame: u32,
    missing: u32,
    spot: Option<(usize, [f32; 2])>,
    last: Option<([f32; 2], Instant)>,
    speed: f32,
    rng: u64,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

fn area(face: &Face) -> f32 {
    let [a, b, c] = face;
    ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() * 0.5
}

fn centroid(face: &Face) -> [f32; 2] {
    let [a, b, c] = face;
    [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0]
}

fn point(face: &Face, [u, v]: [f32; 2]) -> [f32; 2] {
    let [a, b, c] = face;
    let w = 1.0 - u - v;
    [
        u * a[0] + v * b[0] + w * c[0],
        u * a[1] + v * b[1] + w * c[1],
    ]
}

fn inside([x, y]: [f32; 2], [x1, y1, x2, y2]: [i32; 4]) -> bool {
    x >= x1 as f32 && x < x2 as f32 && y >= y1 as f32 && y < y2 as f32
}

impl Job {
    fn new(sequence: u32, request: ActRequest) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self {
            sequence,
            request,
            started: Instant::now(),
            frame: 0,
            missing: 0,
            spot: None,
            last: None,
            speed: 0.0,
            rng: nanos | 1,
        }
    }

    fn random(&mut self) -> f32 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u64 << 24) as f32
    }

    fn gauss(&mut self, mean: f32, deviation: f32) -> f32 {
        let u = self.random().max(f32::EPSILON);
        let v = self.random();
        mean + deviation * (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }

    fn visible(&self, p: [f32; 2]) -> bool {
        let count = (self.request.blocker_count as usize).min(ACT_BLOCKERS);
        inside(p, self.request.viewport)
            && !self.request.blockers[..count]
                .iter()
                .any(|&blocker| inside(p, blocker))
    }

    fn weights(&self, faces: &[Face]) -> Vec<(usize, f32)> {
        let usable: Vec<(usize, f32, [f32; 2])> = faces
            .iter()
            .enumerate()
            .map(|(i, f)| (i, area(f), centroid(f)))
            .filter(|&(_, a, c)| a >= MIN_FACE_AREA && self.visible(c))
            .collect();
        let big: Vec<_> = usable
            .iter()
            .copied()
            .filter(|&(_, a, _)| a >= self.request.min_area)
            .collect();
        let pool = if big.is_empty() { usable } else { big };
        let total: f32 = pool.iter().map(|&(_, a, _)| a).sum();
        if total <= 0.0 {
            return Vec::new();
        }
        let mean = pool.iter().fold([0.0, 0.0], |m, &(_, a, c)| {
            [m[0] + a * c[0] / total, m[1] + a * c[1] / total]
        });
        let var = pool.iter().fold([0.0, 0.0], |v, &(_, a, c)| {
            [
                v[0] + a * (c[0] - mean[0]).powi(2) / total,
                v[1] + a * (c[1] - mean[1]).powi(2) / total,
            ]
        });
        let spread = self.request.spread.max(0.05);
        let sx = var[0].sqrt().max(1.0) * spread;
        let sy = var[1].sqrt().max(1.0) * spread;
        pool.iter()
            .map(|&(i, a, c)| {
                let d = ((c[0] - mean[0]) / sx).powi(2) + ((c[1] - mean[1]) / sy).powi(2);
                (i, a * (-0.5 * d).exp())
            })
            .collect()
    }

    fn pick(&mut self, faces: &[Face]) -> Option<(usize, [f32; 2])> {
        let weights = self.weights(faces);
        let total: f32 = weights.iter().map(|&(_, w)| w).sum();
        if total <= 0.0 {
            return None;
        }
        let mut roll = self.random() * total;
        let index = weights
            .iter()
            .find(|&&(_, w)| {
                roll -= w;
                roll <= 0.0
            })
            .or(weights.last())
            .map(|&(i, _)| i)?;
        for _ in 0..SPOT_TRIES {
            let (u, v) = (
                self.gauss(1.0 / 3.0, SPOT_SPREAD),
                self.gauss(1.0 / 3.0, SPOT_SPREAD),
            );
            if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
                return Some((index, [u, v]));
            }
        }
        Some((index, [1.0 / 3.0, 1.0 / 3.0]))
    }

    fn spot(&mut self, faces: &[Face]) -> Option<[f32; 2]> {
        let kept = self.spot.and_then(|(i, weights)| {
            let face = faces.get(i)?;
            let p = point(face, weights);
            (area(face) >= MIN_FACE_AREA && self.visible(p)).then_some(p)
        });
        if kept.is_some() {
            return kept;
        }
        self.spot = self.pick(faces);
        self.spot.map(|(i, weights)| point(&faces[i], weights))
    }

    fn observe(&mut self, spot: [f32; 2]) {
        let now = Instant::now();
        if let Some((last, at)) = self.last {
            let ms = now.duration_since(at).as_secs_f32() * 1000.0;
            if ms > 0.0 {
                let speed = (spot[0] - last[0]).hypot(spot[1] - last[1]) / ms;
                self.speed += (speed - self.speed) * SPEED_SMOOTHING;
            }
        }
        self.last = Some((spot, now));
    }

    fn near(&self) -> f32 {
        self.request.near_px + self.speed * FOLLOW_LAG_MS
    }

    fn targets(&self, entry: &MenuEntryInfo) -> bool {
        let r = &self.request;
        (r.entry_kind < 0 || entry.kind as i32 == r.entry_kind)
            && (r.entry_id < 0 || entry.id == r.entry_id)
            && (r.entry_index < 0 || entry.index == r.entry_index)
    }

    fn offers(&self, entry: &MenuEntryInfo) -> bool {
        let count = (self.request.option_count as usize).min(ACT_OPTIONS);
        self.targets(entry)
            && self.request.options[..count]
                .iter()
                .any(|option| text(option).eq_ignore_ascii_case(text(&entry.option)))
    }
}

struct Outcome {
    status: i32,
    faces: u32,
    spot: Option<[f32; 2]>,
    mouse: [i32; 2],
    entry: MenuEntryInfo,
}

impl Outcome {
    fn of(status: i32) -> Self {
        Self {
            status,
            faces: 0,
            spot: None,
            mouse: [0; 2],
            entry: MenuEntryInfo::default(),
        }
    }

    fn done(&self) -> bool {
        self.status != act_status::TRACKING
    }
}

fn lparam(x: i32, y: i32) -> LPARAM {
    LPARAM(((y as u16 as u32) << 16 | x as u16 as u32) as isize)
}

unsafe fn press(raw: (i32, i32), down: u32, held: usize) {
    let window = HWND(hooked_window() as *mut c_void);
    let at = lparam(raw.0, raw.1);
    let synthetic = SYNTHETIC as usize;
    let _ = PostMessageW(Some(window), WM_MOUSEMOVE, WPARAM(synthetic), at);
    let _ = PostMessageW(Some(window), down, WPARAM(held | synthetic), at);
}

fn step(job: &mut Job, offset: (i32, i32)) -> Outcome {
    job.frame += 1;
    if job.started.elapsed().as_millis() as u64 > job.request.timeout_ms as u64 {
        return Outcome::of(act_status::TIMEOUT);
    }
    let faces = match model::faces(&job.request.model) {
        Ok(faces) => faces,
        Err(model_status::NO_MODEL | model_status::NO_NPC) => Vec::new(),
        Err(model_status::OTHER_THREAD) => return Outcome::of(act_status::OTHER_THREAD),
        Err(model_status::NOT_IN_GAME) => return Outcome::of(act_status::NOT_IN_GAME),
        Err(_) => return Outcome::of(act_status::NOT_FOUND),
    };
    let Some(spot) = job.spot(&faces) else {
        if faces.is_empty() {
            job.missing += 1;
        }
        return Outcome {
            faces: faces.len() as u32,
            ..Outcome::of(if job.missing > GONE_FRAMES {
                act_status::GONE
            } else {
                act_status::TRACKING
            })
        };
    };
    job.missing = 0;
    job.observe(spot);
    let mut outcome = Outcome {
        faces: faces.len() as u32,
        spot: Some(spot),
        ..Outcome::of(act_status::TRACKING)
    };
    let entries = match menu::snapshot() {
        Some(Snapshot::Closed(entries)) => entries,
        Some(Snapshot::Open) => {
            outcome.status = act_status::MENU_OPEN;
            return outcome;
        }
        None => return outcome,
    };
    let raw = unsafe { memory_manager().get_mouse_position() };
    let mouse = [raw.0 - offset.0, raw.1 - offset.1];
    outcome.mouse = mouse;
    let near = (mouse[0] as f32 - spot[0]).hypot(mouse[1] as f32 - spot[1]) <= job.near();
    if !near || !entries.iter().any(|e| job.targets(e)) {
        return outcome;
    }
    if let Some(left) = entries.first().filter(|left| job.offers(left)) {
        outcome.entry = *left;
        outcome.status = act_status::LEFT;
        unsafe { press(raw, WM_LBUTTONDOWN, MK_LBUTTON) };
    } else if let Some(entry) = entries.iter().find(|e| job.offers(e)) {
        if job.request.right != 0 {
            outcome.entry = *entry;
            outcome.status = act_status::RIGHT;
            unsafe { press(raw, WM_RBUTTONDOWN, MK_RBUTTON) };
        }
    } else {
        outcome.status = act_status::NO_OPTION;
    }
    outcome
}

fn publish(sequence: u32, frame: u32, face: i32, outcome: &Outcome) {
    let Some(mapping) = mapping() else { return };
    mapping.shared().write_act_state(&ActState {
        request: sequence,
        status: outcome.status,
        frame,
        has_spot: outcome.spot.is_some() as u32,
        spot: outcome.spot.unwrap_or_default(),
        face,
        face_count: outcome.faces,
        mouse: outcome.mouse,
        entry: outcome.entry,
    });
}

pub fn on_request() {
    let Some(mapping) = mapping() else { return };
    let Some((sequence, request)) = mapping.shared().read_act_request() else {
        return;
    };
    let Ok(mut job) = JOB.lock() else { return };
    let status = if request.command == act_command::ARM {
        *job = Some(Job::new(sequence, request));
        act_status::TRACKING
    } else {
        *job = None;
        act_status::CANCELLED
    };
    publish(sequence, 0, -1, &Outcome::of(status));
}

pub fn on_frame(offset: (i32, i32)) {
    let Ok(mut guard) = JOB.try_lock() else {
        return;
    };
    let Some(job) = guard.as_mut() else { return };
    let outcome = step(job, offset);
    let face = job.spot.map_or(-1, |(i, _)| i as i32);
    publish(job.sequence, job.frame, face, &outcome);
    if outcome.done() {
        *guard = None;
    }
}
