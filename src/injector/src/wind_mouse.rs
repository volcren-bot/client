// Author: Raymond van Venetië and Merlijn Wajer
// Project: Simba (https://github.com/MerlijnWajer/Simba)
// License: GNU General Public License (https://www.gnu.org/licenses/gpl-3.0)
// --------------------------------------------------------------------------
// Move the mouse in a "human like" way.
// Author: BenLand100
//   https://github.com/BenLand100/SMART/blob/master/src/EventNazi.java#L201
//   https://ben.land/post/2021/04/25/windmouse-human-mouse-movement
//
// Modified over time by Olly for Villavu/Simba (https://github.com/Villavu/Simba)

use std::{
    f64::consts::PI,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

const HESITATE_CHANCE: f64 = 0.03;
const GUST_CHANCE: f64 = 0.05;
const SUBSTEP_CHANCE: f64 = 0.02;

pub const MAX_TIMEOUT_MS: u32 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Options {
    pub speed: f64,
    pub wind: f64,
    pub gravity: f64,
    pub timeout_ms: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self { speed: 12.0, wind: 6.0, gravity: 9.0, timeout_ms: 10_000 }
    }
}

pub fn run(
    start: Option<(f64, f64)>,
    dest: (i32, i32),
    options: &Options,
    post: impl FnMut(i32, i32) -> Result<(), String>,
) -> Result<(), String> {
    run_with(start, dest, options, post, |duration| {
        thread::sleep(duration);
        true
    })
}

// `sleep` returns false when the move was cancelled: the cursor stays where it is and nothing more is posted.
pub fn run_with(
    start: Option<(f64, f64)>,
    dest: (i32, i32),
    options: &Options,
    mut post: impl FnMut(i32, i32) -> Result<(), String>,
    sleep: impl FnMut(Duration) -> bool,
) -> Result<(), String> {
    let Some(start) = start else {
        return post(dest.0, dest.1);
    };
    WindMouse::new(start, dest, options, post, sleep).run()
}

struct Rng(u64);

impl Rng {
    fn seeded() -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let mut z = nanos ^ (std::process::id() as u64) << 32;
        z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Self((z ^ (z >> 31)) | 1)
    }

    fn random(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.random() * (hi - lo)
    }

    fn skew(&mut self, near: f64, far: f64) -> f64 {
        near + (far - near) * self.random() * self.random()
    }
}

fn smooth_step(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    hi.min(lo.max(v))
}

fn sqr(v: f64) -> f64 {
    v * v
}

struct WindMouse<P, S> {
    post: P,
    sleep: S,
    cancelled: bool,
    rng: Rng,
    due: Instant,

    dest_x: f64,
    dest_y: f64,
    speed: f64,
    wind: f64,
    gravity: f64,
    timeout_ms: u32,

    x: f64,
    y: f64,
    move_x: f64,
    move_y: f64,
    total_dist: f64,
    remaining_dist: f64,
    progress: f64,

    max_step: f64,
    end_step: f64,
    gap: f64,
    decel_rate: f64,
    inertia: f64,
    launch_scale: f64,
    arc_scale: f64,
    arc_amp: f64,
    arc_decay: f64,
    arc_freq: f64,
    arc_skew: f64,
    hesitate_at: f64,
    coasting: bool,
    overshot: bool,
}

impl<P: FnMut(i32, i32) -> Result<(), String>, S: FnMut(Duration) -> bool> WindMouse<P, S> {
    fn new(start: (f64, f64), dest: (i32, i32), options: &Options, post: P, sleep: S) -> Self {
        Self {
            post,
            sleep,
            cancelled: false,
            rng: Rng::seeded(),
            due: Instant::now(),
            dest_x: dest.0 as f64,
            dest_y: dest.1 as f64,
            speed: clamp(options.speed, 2.0, 30.0),
            wind: clamp(options.wind, 0.0, 15.0),
            gravity: clamp(options.gravity, 1.0, 30.0),
            timeout_ms: options.timeout_ms.min(MAX_TIMEOUT_MS),
            x: start.0,
            y: start.1,
            move_x: 0.0,
            move_y: 0.0,
            total_dist: 0.0,
            remaining_dist: 0.0,
            progress: 0.0,
            max_step: 0.0,
            end_step: 0.0,
            gap: 0.0,
            decel_rate: 0.0,
            inertia: 0.0,
            launch_scale: 0.0,
            arc_scale: 0.0,
            arc_amp: 0.0,
            arc_decay: 0.0,
            arc_freq: 0.0,
            arc_skew: 0.0,
            hesitate_at: 0.0,
            coasting: false,
            overshot: false,
        }
    }

    fn teleport(&mut self, x: f64, y: f64, sleep_ms: Option<f64>) -> Result<(), String> {
        if self.cancelled {
            return Ok(());
        }
        (self.post)(x.round() as i32, y.round() as i32)?;

        let ms = match sleep_ms {
            Some(ms) => ms,
            None => {
                let mut ms = self.gap * self.rng.range(0.925, 1.075);
                if self.rng.random() < 0.015 {
                    ms *= self.rng.range(1.5, 2.5);
                }
                ms
            }
        };

        let due = self.due + Duration::from_secs_f64(ms.max(0.0) / 1000.0);
        let now = Instant::now();
        if due > now {
            self.cancelled = !(self.sleep)(due - now);
            self.due = due;
        } else {
            self.due = now;
        }
        Ok(())
    }

    fn flick(&mut self, scale: f64) {
        let mag = (sqr(self.rng.random()) * 6.5 * self.arc_scale).min(self.max_step) * scale;
        let angle = self.rng.random() * 2.0 * PI;
        self.move_x += angle.cos() * mag;
        self.move_y += angle.sin() * mag;
    }

    fn plan(&mut self, from_x: f64, from_y: f64) {
        self.total_dist = (from_x - self.dest_x).hypot(from_y - self.dest_y).max(1.0);

        let move_time = (100.0 + 14.0 * self.total_dist.sqrt() / (self.speed / 10.0)) * self.rng.range(0.78, 1.22);
        self.gap = self.rng.range(6.0, 9.0);
        let n = (move_time / self.gap).round().max(3.0);
        self.max_step = (self.total_dist / n / 0.80).min(self.total_dist * 0.5);

        self.arc_scale = clamp(2.8 * self.wind / self.gravity, 0.4, 2.5);
        if self.rng.random() < GUST_CHANCE {
            self.arc_scale *= 1.4 + self.rng.random() * 0.5;
        }

        self.launch_scale = self.rng.range(0.15, 0.30);
        self.decel_rate = self.rng.range(0.42, 0.65);
        if self.rng.random() < 0.22 {
            self.decel_rate = self.rng.range(0.7, 0.88);
        }
        self.inertia = 0.31 + 0.17 * sqr(self.rng.random());
        self.end_step = (2.0 + sqr(self.rng.random()) * 2.5).min(self.max_step);

        self.hesitate_at = 2.0;
        if self.rng.random() < HESITATE_CHANCE * clamp((self.total_dist - 120.0) / 130.0, 0.0, 1.0) {
            self.hesitate_at = self.rng.range(0.20, 0.75);
        }

        self.coasting = self.rng.random() < 0.06 && self.total_dist > 3.0;
        self.overshot = false;

        self.arc_amp = ((0.030 + sqr(self.rng.random()) * 0.130) * self.arc_scale).min(0.45) * self.total_dist;
        self.arc_decay = 1.45 * clamp(self.gravity / 12.0, 0.3, 3.0);
        self.arc_skew = 0.50 + sqr(self.rng.random()) * 0.58;
        if self.rng.random() >= 0.5 {
            self.arc_amp = -self.arc_amp;
        }

        if self.rng.random() < 0.80 {
            self.arc_freq = 1.0;
        } else {
            self.arc_freq = 2.0;
            self.arc_amp *= 0.6;
        }
    }

    fn hesitation_step(&mut self) -> Result<bool, String> {
        if !(self.hesitate_at <= 1.0 && self.progress >= self.hesitate_at) {
            return Ok(false);
        }

        self.move_x *= 1.0 - self.inertia;
        self.move_y *= 1.0 - self.inertia;
        self.x += self.move_x;
        self.y += self.move_y;
        if self.move_x.hypot(self.move_y) < 0.75 {
            self.hesitate_at = 2.0;
            if self.rng.random() < 0.60 {
                let scale = 1.0 + sqr(self.rng.random()) * 5.0;
                self.flick(scale);
            }
            let pause = self.rng.skew(80.0, 600.0);
            self.teleport(self.x, self.y, Some(pause))?;
        } else {
            self.teleport(self.x, self.y, None)?;
        }
        Ok(true)
    }

    fn step(&mut self) -> Result<(), String> {
        if self.hesitation_step()? {
            return Ok(());
        }

        let remaining = self.remaining_dist.max(f64::EPSILON);
        let dir_x = (self.dest_x - self.x) / remaining;
        let dir_y = (self.dest_y - self.y) / remaining;

        if self.overshot || self.remaining_dist * self.decel_rate <= self.max_step {
            let mut move_len = (self.remaining_dist * self.decel_rate * self.rng.range(0.7, 1.3)).max(self.end_step);
            if self.coasting && move_len >= self.remaining_dist - 0.5 {
                move_len = self.remaining_dist + clamp(self.max_step * 0.3, 3.0, 8.0);
                self.coasting = false;
                self.overshot = true;
            } else {
                move_len = move_len.min(self.remaining_dist);
            }
            self.move_x = dir_x * move_len;
            self.move_y = dir_y * move_len;
        } else {
            let envelope = smooth_step(clamp(self.progress / 0.012, 0.0, 1.0)).max(self.launch_scale);
            let cur_step = self.max_step * envelope;
            let mut arc_offset = self.arc_amp
                * (-self.arc_decay * self.progress).exp()
                * (self.arc_freq * PI * self.progress.powf(self.arc_skew)).sin();
            arc_offset *= clamp((self.remaining_dist - self.max_step) / self.max_step, 0.0, 1.0);
            arc_offset = clamp(arc_offset, -0.5 * self.remaining_dist, 0.5 * self.remaining_dist);
            let aim_x = (self.dest_x - dir_y * arc_offset) - self.x;
            let aim_y = (self.dest_y + dir_x * arc_offset) - self.y;
            let aim_len = aim_x.hypot(aim_y).max(1.0);
            self.move_x += ((aim_x / aim_len) * cur_step - self.move_x) * self.inertia;
            self.move_y += ((aim_y / aim_len) * cur_step - self.move_y) * self.inertia;
        }

        self.x += self.move_x;
        self.y += self.move_y;

        if self.rng.random() < SUBSTEP_CHANCE && self.move_x.hypot(self.move_y) > 3.0 {
            let split = self.rng.range(0.35, 0.70);
            let pause = self.gap * self.rng.range(0.3, 0.6);
            self.teleport(self.x - self.move_x * split, self.y - self.move_y * split, Some(pause))?;
        }

        self.teleport(self.x, self.y, None)
    }

    fn run(mut self) -> Result<(), String> {
        let timeout_ms = self.timeout_ms;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);

        let (start_x, start_y) = (self.x, self.y);
        self.plan(start_x, start_y);

        self.move_y = (self.launch_scale * self.max_step) * (self.dest_y - start_y) / self.total_dist;
        self.move_x = (self.launch_scale * self.max_step) * (self.dest_x - start_x) / self.total_dist;
        if self.rng.random() < 0.50 {
            self.flick(1.0);
        }

        while Instant::now() < deadline && !self.cancelled {
            self.remaining_dist = (self.x - self.dest_x).hypot(self.y - self.dest_y);
            if self.remaining_dist <= 2.0 && !self.coasting {
                break;
            }

            if self.remaining_dist > self.total_dist {
                self.total_dist = self.remaining_dist;
            }
            self.progress = clamp(1.0 - self.remaining_dist / self.total_dist, 0.0, 1.0);

            self.step()?;
        }

        if self.cancelled {
            return Ok(());
        }

        if Instant::now() >= deadline && (self.x - self.dest_x).hypot(self.y - self.dest_y) > self.end_step {
            return Err(format!(
                "wind_mouse timed out after {timeout_ms}ms. Start={},{} Dest={},{} XY={},{}",
                start_x.round(),
                start_y.round(),
                self.dest_x.round(),
                self.dest_y.round(),
                self.x.round(),
                self.y.round()
            ));
        }

        if self.x.round() != self.dest_x.round() || self.y.round() != self.dest_y.round() {
            self.teleport(self.dest_x, self.dest_y, None)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: Options = Options { speed: 30.0, wind: 6.0, gravity: 9.0, timeout_ms: 10_000 };

    fn points(start: Option<(f64, f64)>, dest: (i32, i32), options: &Options) -> (Vec<(i32, i32)>, Result<(), String>) {
        let mut posted = Vec::new();
        let result = run(start, dest, options, |x, y| {
            posted.push((x, y));
            Ok(())
        });
        (posted, result)
    }

    #[test]
    fn lands_on_the_destination() {
        for _ in 0..5 {
            let (posted, result) = points(Some((10.0, 20.0)), (500, 300), &FAST);
            assert!(result.is_ok(), "{result:?}");
            assert!(posted.len() >= 3, "{posted:?}");
            assert_eq!(posted.last(), Some(&(500, 300)));
            assert!(posted.iter().all(|&(x, y)| (-200..800).contains(&x) && (-200..600).contains(&y)), "{posted:?}");
        }
    }

    #[test]
    fn without_a_start_it_jumps() {
        assert_eq!(points(None, (5, 6), &FAST), (vec![(5, 6)], Ok(())));
    }

    #[test]
    fn already_there_posts_nothing() {
        assert_eq!(points(Some((40.0, 40.0)), (40, 40), &FAST), (vec![], Ok(())));
    }

    #[test]
    fn post_errors_pass_through() {
        let result = run(Some((0.0, 0.0)), (300, 300), &FAST, |_, _| Err("no window".into()));
        assert_eq!(result, Err("no window".into()));
    }

    #[test]
    fn times_out_far_from_the_destination() {
        let slow = Options { speed: 2.0, timeout_ms: 60, ..FAST };
        let (_, result) = points(Some((0.0, 0.0)), (2000, 0), &slow);
        let error = result.unwrap_err();
        assert!(error.starts_with("wind_mouse timed out after 60ms. Start=0,0 Dest=2000,0"), "{error}");
    }

    #[test]
    fn paces_by_the_gap() {
        let start = Instant::now();
        let (posted, result) = points(Some((0.0, 0.0)), (400, 400), &FAST);
        assert!(result.is_ok());
        let per_point = start.elapsed().as_secs_f64() * 1000.0 / posted.len() as f64;
        assert!(per_point >= 5.0, "{per_point} ms per point over {} points", posted.len());
    }

    #[test]
    fn cancelling_stops_posting() {
        let mut posted = 0;
        let mut sleeps = 0;
        let result = run_with(
            Some((0.0, 0.0)),
            (500, 400),
            &FAST,
            |_, _| {
                posted += 1;
                Ok(())
            },
            |_| {
                sleeps += 1;
                sleeps < 4
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(sleeps, 4);
        assert!(posted < 10, "{posted}");
    }

    #[test]
    fn random_stays_in_range() {
        let mut rng = Rng::seeded();
        for _ in 0..100_000 {
            let r = rng.random();
            assert!((0.0..1.0).contains(&r));
        }
    }
}
