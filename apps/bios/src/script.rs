//! `--input <script>` and `--record <dir>@<seconds>`: pictures of what the window does over time.
//!
//! The script feeds pointer, button, wheel and pinch events into the window's own input, the
//! path a real mouse takes, at set times after the window opens; the recorder saves a picture of
//! the window every so often while the script runs, then closes the window. Together they show
//! what a picture of one moment cannot: a drag, a scroll, a hand-off between views, and how hover
//! and motion unfold.
//!
//! A script is steps separated by `;`, each `<seconds> <what>`:
//! - `move x,y`: the pointer jumps there (window points)
//! - `glide x,y <seconds>`: the pointer moves there in a straight line over that long
//! - `down` / `up`: the primary button
//! - `wheel dx,dy`: a scroll of that many points; `cmdwheel dy` with ⌘ held
//! - `zoom f`: a pinch by that factor (above 1 zooms in)
//! - `key <chord>`: a key press and release, e.g. `key cmd+f`, `key shift+right`, `key escape`
//! - `type <text>`: typed text, no spaces
//! - `window <n>`: from here on the main window draws the nth Bios window (1 is itself, 2 the
//!   second), and the steps act in it: the system cannot take a picture of a second window
//! - `end`: the recording stops here

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, TouchPhase, pos2, vec2};

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Move(Pos2),
    Button(bool),
    Wheel(egui::Vec2, Modifiers),
    Zoom(f32),
    Key(egui::Key, Modifiers),
    Text(String),
    Window(usize),
    End,
}

/// The pointer moves this often during a glide.
const GLIDE_STEP: f64 = 1.0 / 60.0;

/// `cmd+shift+f`: the key and its modifiers.
fn chord(s: &str) -> Option<(egui::Key, Modifiers)> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in s.split('+') {
        match part.to_ascii_lowercase().as_str() {
            "cmd" => mods |= Modifiers::COMMAND,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "opt" => mods |= Modifiers::ALT,
            "ctrl" => mods |= Modifiers::CTRL,
            "right" => key = Some(egui::Key::ArrowRight),
            "left" => key = Some(egui::Key::ArrowLeft),
            "up" => key = Some(egui::Key::ArrowUp),
            "down" => key = Some(egui::Key::ArrowDown),
            other => {
                let capital: String = other.chars().take(1).flat_map(char::to_uppercase).chain(other.chars().skip(1)).collect();
                key = egui::Key::from_name(other)
                    .or_else(|| egui::Key::from_name(&capital))
                    .or_else(|| egui::Key::from_name(&other.to_ascii_uppercase()));
            }
        }
    }
    Some((key?, mods))
}

fn xy(s: &str) -> Option<Pos2> {
    let (x, y) = s.split_once(',')?;
    Some(pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// The steps of a script in time order, glides spelt out as moves; `None` names the bad step.
fn parse(script: &str) -> Result<Vec<(f64, Step)>, String> {
    let mut steps = Vec::new();
    let mut at = pos2(0.0, 0.0);
    for raw in script.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let bad = || format!("--input: cannot read {raw:?}");
        let mut words = raw.split_whitespace();
        let t: f64 = words.next().and_then(|w| w.parse().ok()).ok_or_else(bad)?;
        let what = words.next().ok_or_else(bad)?;
        let arg = words.next();
        match what {
            "move" => {
                at = arg.and_then(xy).ok_or_else(bad)?;
                steps.push((t, Step::Move(at)));
            }
            "glide" => {
                let to = arg.and_then(xy).ok_or_else(bad)?;
                let over: f64 = words.next().and_then(|w| w.parse().ok()).filter(|d: &f64| *d > 0.0).ok_or_else(bad)?;
                let n = (over / GLIDE_STEP).ceil().max(1.0) as u32;
                for i in 1..=n {
                    let f = f64::from(i) / f64::from(n);
                    steps.push((t + over * f, Step::Move(at.lerp(to, f as f32))));
                }
                at = to;
            }
            "down" => steps.push((t, Step::Button(true))),
            "up" => steps.push((t, Step::Button(false))),
            "wheel" => {
                let d = arg.and_then(xy).ok_or_else(bad)?;
                steps.push((t, Step::Wheel(vec2(d.x, d.y), Modifiers::NONE)));
            }
            "cmdwheel" => {
                let dy: f32 = arg.and_then(|a| a.parse().ok()).ok_or_else(bad)?;
                steps.push((t, Step::Wheel(vec2(0.0, dy), Modifiers::COMMAND)));
            }
            "zoom" => steps.push((t, Step::Zoom(arg.and_then(|a| a.parse().ok()).filter(|f: &f32| *f > 0.0).ok_or_else(bad)?))),
            "key" => {
                let (key, mods) = arg.and_then(chord).ok_or_else(bad)?;
                steps.push((t, Step::Key(key, mods)));
            }
            "type" => steps.push((t, Step::Text(arg.ok_or_else(bad)?.to_string()))),
            "window" => steps.push((t, Step::Window(arg.and_then(|a| a.parse().ok()).filter(|n: &usize| *n > 0).ok_or_else(bad)?))),
            "end" => steps.push((t, Step::End)),
            _ => return Err(bad()),
        }
    }
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(steps)
}

/// `<dir>@<seconds>`: where the pictures go and how often one is taken.
fn parse_record(arg: &str) -> Option<(PathBuf, f64)> {
    let (dir, every) = arg.rsplit_once('@')?;
    let every: f64 = every.parse().ok().filter(|e: &f64| *e > 0.0)?;
    Some((PathBuf::from(dir), every))
}

pub struct Script {
    steps: Vec<(f64, Step)>,
    next: usize,
    pos: Pos2,
    record: Option<(PathBuf, f64)>,
    taken: u32,
    started: Instant,
    closing: bool,
    /// Which window the main one draws, from the last `window` step.
    window: Option<usize>,
}

impl Script {
    pub fn new(input: Option<&str>, record: Option<&str>) -> Result<Option<Self>, String> {
        if input.is_none() && record.is_none() {
            return Ok(None);
        }
        let steps = input.map(parse).transpose()?.unwrap_or_default();
        let record = record.map(|r| parse_record(r).ok_or_else(|| format!("--record: cannot read {r:?}; give <dir>@<seconds>"))).transpose()?;
        if let Some((dir, _)) = &record {
            std::fs::create_dir_all(dir).map_err(|e| format!("--record: {}: {e}", dir.display()))?;
        }
        Ok(Some(Self { steps, next: 0, pos: pos2(0.0, 0.0), record, taken: 0, started: Instant::now(), closing: false, window: None }))
    }

    fn end(&self) -> f64 {
        self.steps.iter().rev().find(|(_, s)| *s == Step::End).or(self.steps.last()).map_or(3.0, |(t, _)| *t)
    }

    /// Feeds the steps that are due into this frame's input.
    pub fn feed(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let now = self.started.elapsed().as_secs_f64();
        while let Some((t, step)) = self.steps.get(self.next).cloned() {
            if t > now {
                break;
            }
            self.next += 1;
            match step {
                Step::Move(p) => {
                    self.pos = p;
                    raw.events.push(Event::PointerMoved(p));
                }
                Step::Button(pressed) => {
                    raw.events.push(Event::PointerButton { pos: self.pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE })
                }
                Step::Wheel(delta, modifiers) => {
                    raw.events.push(Event::MouseWheel {
                        unit: MouseWheelUnit::Point,
                        delta,
                        phase: TouchPhase::Move,
                        source: MouseWheelSource::Unknown,
                        modifiers,
                    });
                }
                Step::Zoom(f) => raw.events.push(Event::Zoom(f)),
                Step::Key(key, modifiers) => {
                    for pressed in [true, false] {
                        raw.events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
                    }
                }
                Step::Text(text) => raw.events.push(Event::Text(text)),
                Step::Window(n) => self.window = Some(n),
                Step::End => {}
            }
        }
        ctx.request_repaint();
    }

    /// The window a `window` step asked the main one to draw, counted from 1.
    pub fn window(&self) -> Option<usize> {
        self.window
    }

    /// Takes the pictures that are due, and closes the window once the script has run.
    pub fn record(&mut self, ctx: &egui::Context) {
        let now = self.started.elapsed().as_secs_f64();
        if self.closing {
            return;
        }
        if now > self.end() + 0.3 {
            self.closing = true;
            // The last picture's callback may still be on its way; a later frame closes.
            let closer = ctx.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |_| {
                closer.send_viewport_cmd(egui::ViewportCommand::Close);
            })));
            return;
        }
        let Some((dir, every)) = &self.record else { return };
        let due = (now / every).floor() as u32;
        if due < self.taken {
            return;
        }
        self.taken = due.saturating_add(1);
        let path = dir.join(format!("{:03}-{:05}ms.png", due, (now * 1000.0) as u64));
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |image| {
            if let Err(why) = crate::save_png(&path, &image) {
                eprintln!("could not save {}: {why}", path.display());
            }
        })));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_reads_in_time_order_with_glides_spelt_out() {
        let steps = parse("1 move 10,10; 0.5 down; 1 glide 20,10 0.1; 2 wheel 0,-40; 2.5 zoom 1.5; 3 end").unwrap_or_default();
        assert_eq!(steps.first(), Some(&(0.5, Step::Button(true))));
        assert!(steps.iter().any(|(_, s)| *s == Step::Move(pos2(20.0, 10.0))));
        assert_eq!(steps.last(), Some(&(3.0, Step::End)));
        assert!(steps.windows(2).all(|w| w[0].0 <= w[1].0));
    }

    #[test]
    fn a_bad_step_is_named_not_a_crash() {
        assert!(parse("1 move ten").is_err());
        assert!(parse("x down").is_err());
        assert!(parse("1 fly 1,1").is_err());
        assert!(parse("1 window 0").is_err());
        assert_eq!(parse("1 window 2").ok().and_then(|s| s.first().cloned()), Some((1.0, Step::Window(2))));
        assert_eq!(chord("cmd+f"), Some((egui::Key::F, Modifiers::COMMAND)));
        assert_eq!(chord("shift+right"), Some((egui::Key::ArrowRight, Modifiers::SHIFT)));
        assert!(chord("cmd+nonsense").is_none());
        assert_eq!(chord("escape"), Some((egui::Key::Escape, Modifiers::NONE)));
        assert_eq!(parse_record("out@0.1").map(|r| r.1), Some(0.1));
        assert!(parse_record("out").is_none());
    }
}
