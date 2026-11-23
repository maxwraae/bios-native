//! THE RING AS A FLYWHEEL. A toy, and a pure one: no drawing, no time source. Ported from Bios
//! `editor/overview/flywheel.ts`. The map feeds it wheel events and frames, each with the time it
//! happened, and paints what it says.
//!
//! The story it tells. Scrolling the ring turns it, exactly as before, until you have kept at it
//! for a couple of seconds and a full turn. Then the ring is yours: it keeps turning on its own,
//! every further scroll adds to its speed (or brakes it), friction is slow, and a readout counts
//! the rpm. The ramp is in stages, and each takes more: past `sparks_rpm` the odd spark; past
//! `setting_off_rpm` it starts setting off, sparks pour and the ring blurs; past `fire_rpm` it is
//! ablaze, flames round the rim; past `redline_rpm` it breaks loose and rolls off the side of the
//! map in the direction it is turning, burning; a button rolls it back in. A hand on a spinning
//! ring (a press) stops it dead. Air drag grows with the square of the speed, so the top of the
//! ramp is hard: a steady scroll settles in the fire, and only a hard one sends it off.
//!
//! Units: time in SECONDS, speed in TURNS PER SECOND of the viewport's `center`. Sign follows the
//! projection: a rising `center` turns the drawing COUNTER-clockwise, so omega < 0 is clockwise on
//! screen, and a clockwise wheel rolls to the RIGHT (`screen_dir` = +1). Geometry is unit-free:
//! sparks live on a unit ring (radius 1, y down, the same angle convention as `ring_point`) and
//! the ring's exit is a signed fraction (0 home, ±1 clear of the map); the painter scales both.
//!
//! Integration is exact per step (the closed form of exponential decay), so the physics does not
//! depend on the frame rate: two 8 ms frames turn the ring exactly as far as one 16 ms frame.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpinPhase {
    Idle,
    Windup,
    Spinning,
    Leaving,
    Gone,
    Returning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Quiet,
    Hot,
    Ablaze,
    Redline,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spark {
    /// Where it left the rim, unit-ring coordinates.
    pub x: f64,
    pub y: f64,
    /// Unit radii per second: along the tangent in the spin direction, plus a little outward drift.
    pub vx: f64,
    pub vy: f64,
    pub born: f64,
    /// 0 amber, 1 orange.
    pub hue: u8,
    /// The ring's exit offset when this was thrown, so it leaves the rim where the rim IS.
    pub ox: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub dir: i8,
    pub started_at: f64,
    pub last_at: f64,
    pub turns: f64,
    /// The last `SPEED_WINDOW` seconds of input, for the handover speed: (time, turns).
    pub recent: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flywheel {
    pub phase: SpinPhase,
    /// Turns per second of `center`.
    pub omega: f64,
    pub run: Option<Run>,
    /// When leaving / returning began.
    pub phase_at: f64,
    /// The screen side it left toward: +1 right, -1 left.
    pub side: f64,
    /// Signed exit fraction: 0 home, ±1 fully off the map.
    pub offset: f64,
    /// After a press, wheel input is swallowed until this time (the trackpad's momentum tail).
    pub quiet_until: f64,
    pub sparks: Vec<Spark>,
    /// Fractional spawns carried between frames.
    pub spark_debt: f64,
    /// PRNG state (mulberry32), advanced only by spawning a spark.
    pub seed: u32,
}

/// Every knob in one place (`SPIN`). Seconds, turns, rpm, pixels.
pub mod spin {
    /// Sustained one-way scrolling before the ring is handed over.
    pub const WINDUP: f64 = 2.0;
    /// A pause longer than this ends the run.
    pub const GAP: f64 = 0.3;
    /// And it must have travelled at least this far.
    pub const WINDUP_TURNS: f64 = 1.0;
    /// The handover speed is the travel in this window divided by it.
    pub const SPEED_WINDOW: f64 = 0.4;
    /// A momentum tail that has decayed below this at the two-second mark does not qualify.
    pub const MIN_HANDOVER_RPM: f64 = 20.0;
    /// omega += torque * turns scrolled, per wheel event.
    pub const TORQUE: f64 = 3.0;
    /// Friction time constant (linear).
    pub const TAU: f64 = 8.0;
    /// Air drag, per turn/s: the decay rate gains drag·|omega|, so the top of the ramp is hard.
    pub const DRAG: f64 = 0.011;
    pub const MAX_RPM: f64 = 2200.0;
    /// Below this the ring is at rest and the toy is over.
    pub const STOP_RPM: f64 = 5.0;
    pub const QUIET_AFTER_PRESS: f64 = 0.4;
    /// The stages: the odd spark, setting off (sparks pour, blur), ablaze (flames), and off.
    pub const SPARKS_RPM: f64 = 400.0;
    pub const SETTING_OFF_RPM: f64 = 700.0;
    pub const FIRE_RPM: f64 = 1000.0;
    pub const REDLINE_RPM: f64 = 1300.0;
    pub const SPARK_MAX_LIVE: usize = 90;
    pub const SPARK_LIFE: f64 = 0.6;
    /// Sparks per second per rpm above SPARKS_RPM (the odd one), and per rpm above SETTING_OFF_RPM (the pour).
    pub const SPARK_RATE_PER_RPM: f64 = 0.03;
    pub const POUR_RATE_PER_RPM: f64 = 0.15;
    /// Unit radii per second.
    pub const SPARK_SPEED: f64 = 1.6;
    /// Unit radii per second squared, downward.
    pub const SPARK_GRAVITY: f64 = 1.2;
    pub const BLUR_MAX_PX: f64 = 2.5;
    /// Seconds to roll off, and to roll back.
    pub const LEAVE: f64 = 1.2;
    pub const RETURN: f64 = 1.4;
    /// How fast it turns while rolling back in.
    pub const ROLL_RPM: f64 = 90.0;
}

const TAU: f64 = PI * 2.0;

/// At rest. A seed of 0 is taken as 1, as mulberry32 wants.
pub fn idle(seed: u32, now: f64) -> Flywheel {
    Flywheel {
        phase: SpinPhase::Idle,
        omega: 0.0,
        run: None,
        phase_at: now,
        side: 1.0,
        offset: 0.0,
        quiet_until: 0.0,
        sparks: Vec::new(),
        spark_debt: 0.0,
        seed: if seed == 0 { 1 } else { seed },
    }
}

pub fn rpm_of(s: &Flywheel) -> f64 {
    s.omega.abs() * 60.0
}

/// +1 when the drawing turns clockwise on screen (omega < 0), else -1.
pub fn screen_dir(omega: f64) -> f64 {
    if omega < 0.0 { 1.0 } else { -1.0 }
}

/// The flywheel owns the ring: it is turning by itself, leaving, gone or coming back.
pub fn active(p: SpinPhase) -> bool {
    p != SpinPhase::Idle && p != SpinPhase::Windup
}

pub fn tone(rpm: f64) -> Tone {
    if rpm >= spin::REDLINE_RPM {
        Tone::Redline
    } else if rpm >= spin::FIRE_RPM {
        Tone::Ablaze
    } else if rpm >= spin::SPARKS_RPM {
        Tone::Hot
    } else {
        Tone::Quiet
    }
}

/// 0 until it starts setting off, rising to `BLUR_MAX_PX` at the redline, in quarter-pixel steps.
pub fn blur_px(rpm: f64) -> f64 {
    if rpm < spin::SETTING_OFF_RPM {
        return 0.0;
    }
    let f = 1f64.min((rpm - spin::SETTING_OFF_RPM) / (spin::REDLINE_RPM - spin::SETTING_OFF_RPM));
    (f * spin::BLUR_MAX_PX * 4.0).round() / 4.0
}

/// How much of the rim is burning: 0 below `FIRE_RPM`, 1 at the redline (and past it, on the way out).
pub fn fire(rpm: f64) -> f64 {
    if rpm < spin::FIRE_RPM {
        return 0.0;
    }
    1f64.min((rpm - spin::FIRE_RPM) / (spin::REDLINE_RPM - spin::FIRE_RPM))
}

/// mulberry32, pure: the next number and the next state.
fn rand(seed: u32) -> (f64, u32) {
    let a = seed.wrapping_add(0x6d2b_79f5);
    let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
    t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
    (f64::from(t ^ (t >> 14)) / 4_294_967_296.0, a)
}

fn clamp_omega(w: f64) -> f64 {
    let top = spin::MAX_RPM / 60.0;
    (-top).max(top.min(w))
}

fn ease_in(u: f64) -> f64 {
    u * u * u
}

fn ease_out(u: f64) -> f64 {
    1.0 - (1.0 - u).powi(3)
}

/// A wheel event. `d_turns` is what the map would have panned by (signed turns). Returns the pan
/// to apply directly: the whole delta while the hand still owns the ring, 0 once the flywheel does.
pub fn wheel(s: &Flywheel, d_turns: f64, now: f64) -> (Flywheel, f64) {
    if d_turns == 0.0 || d_turns.is_nan() {
        return (s.clone(), 0.0);
    }
    match s.phase {
        SpinPhase::Idle | SpinPhase::Windup => {
            if now < s.quiet_until {
                return (s.clone(), 0.0);
            }
            let dir: i8 = if d_turns > 0.0 { 1 } else { -1 };
            let run = match &s.run {
                Some(r) if r.dir == dir && now - r.last_at < spin::GAP => {
                    let mut recent: Vec<(f64, f64)> = r.recent.iter().copied().filter(|&(t, _)| now - t < spin::SPEED_WINDOW).collect();
                    recent.push((now, d_turns));
                    Run { dir, started_at: r.started_at, last_at: now, turns: r.turns + d_turns, recent }
                }
                _ => Run { dir, started_at: now, last_at: now, turns: d_turns, recent: vec![(now, d_turns)] },
            };
            let recent_turns: f64 = run.recent.iter().fold(0.0, |a, &(_, d)| a + d);
            let recent_omega = recent_turns / spin::SPEED_WINDOW;
            let qualifies =
                now - run.started_at >= spin::WINDUP && run.turns.abs() >= spin::WINDUP_TURNS && recent_omega.abs() * 60.0 >= spin::MIN_HANDOVER_RPM;
            if qualifies {
                let next = Flywheel {
                    phase: SpinPhase::Spinning,
                    omega: clamp_omega(recent_omega),
                    run: None,
                    sparks: Vec::new(),
                    spark_debt: 0.0,
                    ..s.clone()
                };
                return (next, d_turns);
            }
            (Flywheel { phase: SpinPhase::Windup, run: Some(run), ..s.clone() }, d_turns)
        }
        SpinPhase::Spinning | SpinPhase::Leaving => (Flywheel { omega: clamp_omega(s.omega + spin::TORQUE * d_turns), ..s.clone() }, 0.0),
        _ => (s.clone(), 0.0),
    }
}

/// Friction and drag over one step, exactly. d(omega)/dt = -omega/tau - drag·omega², which has the
/// closed form omega(t) = omega0·e^(-t/tau) / (1 + drag·tau·omega0·(1 - e^(-t/tau))), and the turn
/// over the step is ln(1 + drag·tau·omega0·(1 - e^(-t/tau))) / drag. Solved on |omega|, sign restored.
fn coast(omega: f64, dt: f64) -> (f64, f64) {
    let sign = if omega < 0.0 { -1.0 } else { 1.0 };
    let w0 = omega.abs();
    let e = (-dt / spin::TAU).exp();
    let g = spin::DRAG * spin::TAU * w0 * (1.0 - e);
    ((sign * (w0 * e)) / (1.0 + g), (sign * g.ln_1p()) / spin::DRAG)
}

fn throw_sparks(s: &Flywheel, omega: f64, dt: f64, now: f64) -> (Vec<Spark>, f64, u32) {
    let rpm = omega.abs() * 60.0;
    let mut live: Vec<Spark> = s.sparks.iter().copied().filter(|p| now - p.born < spin::SPARK_LIFE).collect();
    let rate = spin::SPARK_RATE_PER_RPM * 0f64.max(rpm - spin::SPARKS_RPM) + spin::POUR_RATE_PER_RPM * 0f64.max(rpm - spin::SETTING_OFF_RPM);
    let mut debt = s.spark_debt + rate * dt;
    let mut seed = s.seed;
    let dir = screen_dir(omega);
    while debt >= 1.0 && live.len() < spin::SPARK_MAX_LIVE {
        debt -= 1.0;
        let (a, s1) = rand(seed);
        let (b, s2) = rand(s1);
        let (c, s3) = rand(s2);
        seed = s3;
        let theta = a * TAU;
        let (px, py) = (theta.sin(), -theta.cos());
        let (tx, ty) = (theta.cos(), theta.sin());
        let drift = 0.25 * (0.6 + 0.8 * b);
        live.push(Spark {
            x: px,
            y: py,
            vx: tx * dir * spin::SPARK_SPEED + px * drift,
            vy: ty * dir * spin::SPARK_SPEED + py * drift,
            born: now,
            hue: if c < 0.5 { 0 } else { 1 },
            ox: s.offset,
        });
    }
    if rpm <= spin::SPARKS_RPM {
        debt = 0.0; // no debt carried into quieter frames
    }
    (live, debt, seed)
}

/// Where a spark is now, and how much of it is left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SparkNow {
    pub x: f64,
    pub y: f64,
    pub alpha: f64,
}

pub fn spark_at(p: &Spark, now: f64) -> SparkNow {
    let age = now - p.born;
    SparkNow { x: p.x + p.vx * age, y: p.y + p.vy * age + 0.5 * spin::SPARK_GRAVITY * age * age, alpha: 0f64.max(1.0 - age / spin::SPARK_LIFE) }
}

/// One frame. `dt` in seconds (the caller clamps it). Returns the next state and how far to turn
/// `center`.
pub fn step(s: &Flywheel, dt: f64, now: f64) -> (Flywheel, f64) {
    match s.phase {
        SpinPhase::Spinning => {
            let (omega, turned) = coast(s.omega, dt);
            let (sparks, spark_debt, seed) = throw_sparks(s, omega, dt, now);
            let mut next = Flywheel { omega, sparks, spark_debt, seed, ..s.clone() };
            let rpm = rpm_of(&next);
            if rpm >= spin::REDLINE_RPM {
                next = Flywheel { phase: SpinPhase::Leaving, phase_at: now, side: screen_dir(omega), offset: 0.0, ..next };
            } else if rpm < spin::STOP_RPM {
                next = Flywheel { phase: SpinPhase::Idle, omega: 0.0, run: None, sparks: Vec::new(), spark_debt: 0.0, ..next };
            }
            (next, turned)
        }
        SpinPhase::Leaving => {
            let (omega, turned) = coast(s.omega, dt);
            let u = (now - s.phase_at) / spin::LEAVE;
            if u >= 1.0 {
                return (Flywheel { phase: SpinPhase::Gone, omega: 0.0, offset: s.side, sparks: Vec::new(), spark_debt: 0.0, ..s.clone() }, turned);
            }
            let (sparks, spark_debt, seed) = throw_sparks(s, omega, dt, now);
            (Flywheel { omega, offset: s.side * ease_in(u), sparks, spark_debt, seed, ..s.clone() }, turned)
        }
        SpinPhase::Returning => {
            let u = (now - s.phase_at) / spin::RETURN;
            if u >= 1.0 {
                return (Flywheel { phase: SpinPhase::Idle, omega: 0.0, offset: 0.0, run: None, ..s.clone() }, 0.0);
            }
            // Rolls the way it travels: coming in from the left it moves right, which is clockwise.
            let omega = -s.side * (spin::ROLL_RPM / 60.0) * (1.0 - u);
            (Flywheel { omega, offset: -s.side * (1.0 - ease_out(u)), ..s.clone() }, omega * dt)
        }
        _ => (s.clone(), 0.0),
    }
}

/// A hand on the wheel: a spinning ring stops dead. Once it has broken loose you cannot catch it.
pub fn press(s: &Flywheel, now: f64) -> Flywheel {
    if s.phase != SpinPhase::Spinning {
        return s.clone();
    }
    Flywheel {
        phase: SpinPhase::Idle,
        omega: 0.0,
        run: None,
        sparks: Vec::new(),
        spark_debt: 0.0,
        quiet_until: now + spin::QUIET_AFTER_PRESS,
        ..s.clone()
    }
}

/// The button: roll it back in. Only from gone.
pub fn fetch(s: &Flywheel, now: f64) -> Flywheel {
    if s.phase != SpinPhase::Gone {
        return s.clone();
    }
    Flywheel { phase: SpinPhase::Returning, phase_at: now, offset: -s.side, ..s.clone() }
}

/// The flywheel caught at one moment of its story, at time `now`, for a picture of it. `stage` is
/// `spin` (turning at `value` rpm, below the redline), `leaving` (`value` of the way off the map),
/// `gone`, or `returning` (`value` of the way back in). A positive value turns clockwise and
/// leaves to the right, a negative one the other way. The sparks are the ones that would be in the
/// air, and a ring on its way out has coasted there: the physics is run up to `now`.
pub fn pose(stage: &str, value: f64, now: f64) -> Option<Flywheel> {
    if !value.is_finite() {
        return None;
    }
    let sign = if value < 0.0 { -1.0 } else { 1.0 };
    let warm = |rpm: f64, until: f64| {
        let omega = -sign * rpm.abs() / 60.0;
        let mut s = Flywheel { phase: SpinPhase::Spinning, omega, ..idle(7, until) };
        let frames = (spin::SPARK_LIFE * 60.0).round() as usize;
        for k in 0..frames {
            let t = until - spin::SPARK_LIFE + (k + 1) as f64 / 60.0;
            s = Flywheel { phase: SpinPhase::Spinning, omega, ..throw(&s, omega, t) };
        }
        s
    };
    let u = value.abs();
    match stage {
        "spin" if u < spin::REDLINE_RPM => Some(warm(value, now)),
        "leaving" if u < 1.0 => {
            // Broken loose at the redline, then a frame at a time to `now`, coasting and throwing sparks.
            let start = now - u * spin::LEAVE - 1.0 / 60.0;
            let mut s = warm(spin::REDLINE_RPM + 10.0, start);
            let frames = ((now - start) * 60.0).round() as usize;
            for k in 0..frames {
                s = step(&s, 1.0 / 60.0, start + (k + 1) as f64 / 60.0).0;
            }
            Some(s)
        }
        "gone" => Some(Flywheel { phase: SpinPhase::Gone, side: sign, offset: sign, ..idle(7, now) }),
        "returning" if u < 1.0 => {
            let s = Flywheel { phase: SpinPhase::Gone, side: sign, offset: sign, ..idle(7, now) };
            let back = fetch(&s, now - u * spin::RETURN);
            Some(step(&back, 0.0, now).0)
        }
        _ => None,
    }
}

/// Sparks thrown over one 60th of a second at `omega`, the rest left as it is.
fn throw(s: &Flywheel, omega: f64, now: f64) -> Flywheel {
    let (sparks, spark_debt, seed) = throw_sparks(s, omega, 1.0 / 60.0, now);
    Flywheel { sparks, spark_debt, seed, ..s.clone() }
}

// ── WHAT THE MAP DRAWS OF IT (`PlasmidOverview.tsx`: the carriage, `Flames`, `RpmReadout`) ──────

/// How far past the map's edge the carriage travels before the ring counts as gone: clears the
/// labels, the blur halo and the last sparks.
pub const EXIT_PAD: f64 = 48.0;

/// How far the ring travels, in pixels, between home and fully off the map: half the map, the
/// ring's radius and [`EXIT_PAD`]. The exit offset is a fraction of this.
pub fn exit_px(w: f64, r: f64) -> f64 {
    w / 2.0 + r + EXIT_PAD
}

/// A spark as the map draws it: a short streak from its head back along where it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SparkStreak {
    pub head: (f64, f64),
    pub tail: (f64, f64),
    pub alpha: f64,
    /// 0 amber, 1 orange.
    pub hue: u8,
}

/// Where spark `p` is drawn at `now`, for a ring centred at (`cx`, `cy`) of radius `r` in a map
/// `exit` pixels from home to gone. Thrown into MAP space from wherever the rim was, and left
/// behind as the ring goes.
pub fn spark_streak(p: &Spark, now: f64, cx: f64, cy: f64, r: f64, exit: f64) -> SparkStreak {
    let at = spark_at(p, now);
    let x = cx + p.ox * exit + r * at.x;
    let y = cy + r * at.y;
    let sp = p.vx.hypot(p.vy);
    let sp = if sp == 0.0 { 1.0 } else { sp };
    let len = 0.04 * r;
    SparkStreak { head: (x, y), tail: (x - (p.vx / sp) * len, y - (p.vy / sp) * len), alpha: at.alpha, hue: p.hue }
}

/// Tongues of flame round the rim.
pub const FLAMES: usize = 28;

/// One layer of the flames: how tall and wide against the outermost, its colour (0 scarlet,
/// 1 orange, 2 amber) and its opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlameLayer {
    pub h: f64,
    pub w: f64,
    pub color: u8,
    pub opacity: f64,
}

/// Three layers deep: scarlet, orange, an amber tip.
pub const FLAME_LAYERS: [FlameLayer; 3] = [
    FlameLayer { h: 1.0, w: 1.0, color: 0, opacity: 0.55 },
    FlameLayer { h: 0.68, w: 0.66, color: 1, opacity: 0.8 },
    FlameLayer { h: 0.38, w: 0.36, color: 2, opacity: 0.95 },
];

/// One tongue: two quadratic curves, from `a` through control `c1` to the `tip`, and from the tip
/// through `c2` back to `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tongue {
    pub layer: usize,
    pub a: (f64, f64),
    pub c1: (f64, f64),
    pub tip: (f64, f64),
    pub c2: (f64, f64),
    pub b: (f64, f64),
}

/// THE RIM ON FIRE (past `FIRE_RPM`). Tongues stand off the rim and trail against the spin,
/// flickering on the model's clock so they are deterministic per frame. `intensity` (0..1 from
/// `FIRE_RPM` to the redline) sets their height and reach; `dir` is [`screen_dir`].
pub fn flame_tongues(cx: f64, cy: f64, r: f64, intensity: f64, dir: f64, now: f64) -> Vec<Tongue> {
    let mut tongues = Vec::with_capacity(FLAMES * FLAME_LAYERS.len());
    let base = r + 3.0;
    for i in 0..FLAMES {
        let fi = i as f64;
        let th = (fi / FLAMES as f64) * PI * 2.0 + 0.12 * (now * 0.9 + fi * 1.3).sin();
        let flick = 0.5 + 0.5 * ((now * 9.0 + fi * 2.1).sin() * (now * 6.1 + fi * 0.7).sin()).abs();
        let height = r * (0.09 + 0.24 * intensity) * (0.55 + 0.45 * flick);
        let half = r * (0.032 + 0.02 * intensity);
        let sweep = r * (0.06 + 0.16 * intensity) * dir * (0.7 + 0.3 * flick);
        let (px, py) = (th.sin(), -th.cos()); // outward
        let (tx, ty) = (th.cos(), th.sin()); // clockwise tangent
        for (li, l) in FLAME_LAYERS.iter().enumerate() {
            let (h, w, sw) = (height * l.h, half * l.w, sweep * l.h);
            let a = (cx + px * base - tx * w, cy + py * base - ty * w);
            let b = (cx + px * base + tx * w, cy + py * base + ty * w);
            let tip = (cx + px * (base + h) - tx * sw, cy + py * (base + h) - ty * sw);
            let c1 = (a.0 + px * h * 0.55 - tx * sw * 0.25, a.1 + py * h * 0.55 - ty * sw * 0.25);
            let c2 = (b.0 + px * h * 0.55 - tx * sw * 0.25, b.1 + py * h * 0.55 - ty * sw * 0.25);
            tongues.push(Tongue { layer: li, a, c1, tip, c2, b });
        }
    }
    tongues
}

/// The soft glow behind the flames, a radial gradient: each stop's distance from the ring's centre
/// in pixels, its colour (0 scarlet, 1 orange) and its opacity. Past the last stop it is clear.
///
/// The web writes the stops as if offset 0.5 were the glow circle's edge, but an SVG gradient's
/// offset 1 is its edge, so what a browser draws is a warm band INSIDE the rim, peaking at half the
/// radius, behind the dial. That is the picture people see, and the one ported.
pub fn glow_stops(r: f64, intensity: f64) -> [(f64, u8, f64); 4] {
    let glow_r = r * 1.45;
    let rim = r / glow_r / 2.0; // meant as the rim; a fraction of the glow's radius, it lands at half of it
    let at = |offset: f64| offset * glow_r;
    [(at(rim - 0.06), 0, 0.0), (at(rim), 1, 0.35 * intensity), (at(rim + 0.1), 0, 0.12 * intensity), (at(0.5), 0, 0.0)]
}

#[cfg(test)]
mod marks {
    use super::*;

    #[test]
    fn a_spark_streak_trails_behind_its_head_and_rides_with_the_ring() {
        let p = Spark { x: 0.0, y: -1.0, vx: 1.6, vy: 0.0, born: 0.0, hue: 1, ox: 0.5 };
        let s = spark_streak(&p, 0.0, 500.0, 300.0, 100.0, 700.0);
        assert_eq!(s.head, (500.0 + 350.0, 200.0));
        assert!((s.tail.0 - (s.head.0 - 4.0)).abs() < 1e-9 && s.tail.1 == s.head.1);
        assert_eq!(s.alpha, 1.0);
    }

    #[test]
    fn flames_stand_off_the_rim_and_trail_against_the_spin() {
        let t = flame_tongues(0.0, 0.0, 100.0, 1.0, 1.0, 0.0);
        assert_eq!(t.len(), FLAMES * 3);
        let Some(top) = t.first() else { panic!("no tongues") };
        assert!((top.a.1 + 103.0).abs() < 1e-9, "the base sits 3px off the rim");
        assert!(top.tip.1 < top.a.1, "the tip stands outward");
        assert!(top.tip.0 < 0.0, "a clockwise spin trails the tip counter-clockwise");
    }

    #[test]
    fn a_pose_is_the_stage_asked_for_with_its_sparks_in_the_air() {
        let Some(hot) = pose("spin", 900.0, 10.0) else { panic!("no pose") };
        assert_eq!((hot.phase, rpm_of(&hot)), (SpinPhase::Spinning, 900.0));
        assert!(!hot.sparks.is_empty() && hot.sparks.iter().all(|p| p.born <= 10.0 && 10.0 - p.born < spin::SPARK_LIFE));
        let Some(off) = pose("leaving", 0.5, 10.0) else { panic!("no pose") };
        assert_eq!(off.phase, SpinPhase::Leaving);
        assert!((off.offset - 0.125).abs() < 0.01 && rpm_of(&off) < spin::REDLINE_RPM, "it has coasted on its way out");
        assert_eq!(pose("leaving", -0.5, 10.0).map(|s| s.side), Some(-1.0));
        assert_eq!(pose("gone", 0.0, 10.0).map(|s| s.phase), Some(SpinPhase::Gone));
        let Some(back) = pose("returning", 0.5, 10.0) else { panic!("no pose") };
        assert_eq!(back.phase, SpinPhase::Returning);
        assert!(back.offset < 0.0 && back.offset > -1.0);
        assert!(pose("spin", 1400.0, 10.0).is_none() && pose("sideways", 0.0, 0.0).is_none());
    }

    #[test]
    fn the_glow_peaks_inside_the_rim_at_half_the_radius_as_a_browser_draws_it() {
        let g = glow_stops(100.0, 1.0);
        assert!((g[1].0 - 50.0).abs() < 1e-9);
        assert!((g[3].0 - 72.5).abs() < 1e-9);
        assert_eq!(g[1].2, 0.35);
    }
}
