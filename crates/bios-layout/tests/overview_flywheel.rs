//! Parity with Bios `editor/overview/flywheel.test.ts`: one test here per test there, in the same
//! order, with the same assertions and the same numbers.
//!
//! `idle()` in the original defaults its seed to 1 and its clock to 0; here both are passed.

use bios_layout::overview::flywheel::{
    Flywheel, SpinPhase, Tone, active, blur_px, fetch, fire, idle, press, rpm_of, screen_dir, spark_at, spin, step, tone, wheel,
};

/// A trackpad: `hz` events a second, each `turns` of travel (signed), for `seconds`, from `t0`.
fn scroll_hz(mut s: Flywheel, turns: f64, seconds: f64, t0: f64, hz: f64) -> (Flywheel, Vec<f64>, f64) {
    let mut pans = Vec::new();
    let mut t = t0;
    let n = (seconds * hz).round() as usize;
    for i in 0..n {
        t = t0 + (i + 1) as f64 / hz;
        let (next, pan) = wheel(&s, turns, t);
        s = next;
        pans.push(pan);
    }
    (s, pans, t)
}

fn scroll(s: Flywheel, turns: f64, seconds: f64, t0: f64) -> (Flywheel, Vec<f64>, f64) {
    scroll_hz(s, turns, seconds, t0, 60.0)
}

/// Frames: `dt` seconds each, for `seconds`, from `t0`. Sums the turn.
fn run(mut s: Flywheel, seconds: f64, dt: f64, t0: f64) -> (Flywheel, f64, f64) {
    let mut turned = 0.0;
    let mut t = t0;
    let n = (seconds / dt).round() as usize;
    for i in 0..n {
        t = t0 + (i + 1) as f64 * dt;
        let (next, d) = step(&s, dt, t);
        s = next;
        turned += d;
    }
    (s, turned, t)
}

/// A person at the trackpad: one wheel event and one frame every 1/60 s, for `seconds`.
fn drive(mut s: Flywheel, turns: f64, seconds: f64, t0: f64) -> (Flywheel, f64) {
    let mut t = t0;
    let n = (seconds * 60.0).round() as usize;
    for i in 0..n {
        t = t0 + (i + 1) as f64 / 60.0;
        s = wheel(&s, turns, t).0;
        s = step(&s, 1.0 / 60.0, t).0;
    }
    (s, t)
}

/// The closed form of coasting from omega0 (turns/s) for t seconds: the turn made, and the speed left.
fn coast_turn(omega0: f64, t: f64) -> f64 {
    (omega0.signum() * (spin::DRAG * spin::TAU * omega0.abs() * (1.0 - (-t / spin::TAU).exp())).ln_1p()) / spin::DRAG
}

fn coast_omega(omega0: f64, t: f64) -> f64 {
    (omega0 * (-t / spin::TAU).exp()) / (1.0 + spin::DRAG * spin::TAU * omega0.abs() * (1.0 - (-t / spin::TAU).exp()))
}

/// Two seconds at a turn a second is comfortably a windup: 0.03 turns per event, 60 a second.
fn wind_up(t0: f64) -> (Flywheel, Vec<f64>, f64) {
    scroll(idle(7, 0.0), 0.03, 2.2, t0)
}

fn spinning_at(rpm: f64, t0: f64) -> Flywheel {
    Flywheel { phase: SpinPhase::Spinning, omega: -rpm / 60.0, phase_at: t0, ..idle(7, 0.0) }
}

fn approx(a: f64, b: f64, eps: f64) {
    assert!((a - b).abs() <= eps, "{a} ≉ {b}");
}

#[test]
fn a_scroll_before_the_windup_is_a_direct_pan_and_the_phase_is_only_ever_idle_or_windup() {
    let (state, pans, _) = scroll(idle(1, 0.0), 0.03, 1.0, 0.0);
    assert!(pans.iter().all(|&p| p == 0.03));
    assert_eq!(state.phase, SpinPhase::Windup);
    assert!(!active(state.phase));
}

#[test]
fn two_seconds_one_way_with_at_least_a_turn_hands_the_ring_to_the_flywheel() {
    let (state, pans, _) = wind_up(0.0);
    assert_eq!(state.phase, SpinPhase::Spinning);
    assert!(pans.iter().all(|&p| p == 0.03 || p == 0.0), "the qualifying event is still panned, the rest are the flywheel's");
    assert_eq!(pans.last().copied(), Some(0.0), "once spinning, wheel input is torque, not pan");
}

#[test]
fn a_gap_over_300_ms_restarts_the_windup() {
    let (a, _, at) = scroll(idle(1, 0.0), 0.03, 1.5, 0.0);
    let (b, _, _) = scroll(a, 0.03, 1.5, at + 0.5);
    assert_eq!(b.phase, SpinPhase::Windup);
    assert_eq!(b.run.as_ref().map(|r| r.started_at), Some(at + 0.5 + 1.0 / 60.0));
}

#[test]
fn a_change_of_direction_restarts_the_windup() {
    let (a, _, at) = scroll(idle(1, 0.0), 0.03, 1.5, 0.0);
    let (b, _, _) = scroll(a, -0.03, 1.5, at);
    assert_eq!(b.phase, SpinPhase::Windup);
    let r = b.run.as_ref().map(|r| (r.dir, r.turns));
    assert_eq!(r.map(|r| r.0), Some(-1));
    assert!(r.is_some_and(|r| r.1.abs() < 1.5 * 60.0 * 0.03 + 1e-9));
}

#[test]
fn a_flick_a_turn_in_under_two_seconds_stays_a_pan() {
    let (state, _, _) = scroll(idle(1, 0.0), 0.1, 1.5, 0.0);
    assert_eq!(state.phase, SpinPhase::Windup);
}

#[test]
fn two_seconds_of_tiny_deltas_do_not_qualify_without_a_full_turn() {
    let (state, _, _) = scroll(idle(1, 0.0), 0.005, 2.5, 0.0);
    assert_eq!(state.phase, SpinPhase::Windup);
}

#[test]
fn a_momentum_tail_below_the_handover_floor_at_the_two_second_mark_does_not_qualify() {
    // A real scroll for 1.6 s, then a decaying tail: 0.0002 turns an event is 0.7 rpm.
    let (a, _, at) = scroll(idle(1, 0.0), 0.03, 1.6, 0.0);
    let (b, _, _) = scroll(a, 0.0002, 1.0, at);
    assert_eq!(b.phase, SpinPhase::Windup);
}

#[test]
fn the_handover_speed_is_the_recent_scroll_rate() {
    // 121 events at 60 Hz: the 121st is the first past the two-second mark, and the one that hands
    // over. The window then holds 25 events of 0.03 turns over 0.4 s: 1.875 turns/s, 112.5 rpm.
    let (state, _, _) = scroll(idle(7, 0.0), 0.03, 121.0 / 60.0, 0.0);
    assert_eq!(state.phase, SpinPhase::Spinning);
    approx(rpm_of(&state), (25.0 * 0.03) / 0.4 * 60.0, 1e-6);
    assert_eq!(screen_dir(state.omega), -1.0, "scrolling down (positive delta) raises center: counter-clockwise");
}

#[test]
fn step_is_frame_rate_independent_50_ms_frames_and_10_ms_frames_turn_the_ring_exactly_as_far() {
    let s = spinning_at(300.0, 0.0);
    let coarse = run(s.clone(), 2.0, 0.05, 0.0);
    let fine = run(s.clone(), 2.0, 0.01, 0.0);
    approx(coarse.1, coast_turn(s.omega, 2.0), 1e-9);
    approx(fine.1, coast_turn(s.omega, 2.0), 1e-9);
    approx(coarse.0.omega, coast_omega(s.omega, 2.0), 1e-9);
    approx(fine.0.omega, coast_omega(s.omega, 2.0), 1e-9);
}

#[test]
fn coasting_follows_the_closed_form_friction_alone_at_low_speed_and_drag_on_top_of_it_at_high_speed() {
    let slow = run(spinning_at(60.0, 0.0), 4.0, 0.01, 0.0);
    approx(slow.0.omega, coast_omega(-1.0, slow.2), 1e-9);
    approx(rpm_of(&slow.0), 60.0 * (-slow.2 / spin::TAU).exp(), 1.5); // drag barely shows at 60 rpm: 3%
    let fast = run(spinning_at(1200.0, 0.0), 1.0, 0.01, 0.0);
    approx(fast.0.omega, coast_omega(-20.0, fast.2), 1e-9);
    assert!(rpm_of(&fast.0) < 1200.0 * (-fast.2 / spin::TAU).exp() - 50.0, "drag takes more than friction alone would");
}

#[test]
fn harder_at_the_top_a_steady_scroll_settles_in_the_fire_short_of_the_redline_and_only_a_hard_one_sends_it_off() {
    let steady = drive(wind_up(0.0).0, 0.03, 25.0, 2.2).0;
    assert_eq!(steady.phase, SpinPhase::Spinning);
    let rpm = rpm_of(&steady);
    assert!((spin::FIRE_RPM..spin::REDLINE_RPM).contains(&rpm), "settled at {rpm:.0} rpm");
    let hard = drive(wind_up(0.0).0, 0.06, 25.0, 2.2).0;
    assert!(hard.phase == SpinPhase::Leaving || hard.phase == SpinPhase::Gone, "a hard scroll broke it loose ({:?})", hard.phase);
}

#[test]
fn a_scroll_with_the_spin_adds_torque_against_it_brakes_and_omega_never_exceeds_max_rpm() {
    let s = spinning_at(300.0, 0.0);
    let faster = wheel(&s, -0.03, 1.0).0; // negative delta = clockwise, the way it spins
    assert!(rpm_of(&faster) > 300.0);
    let slower = wheel(&s, 0.03, 1.0).0;
    assert!(rpm_of(&slower) < 300.0);
    let flat = scroll(s, -0.1, 5.0, 1.0).0;
    assert!(rpm_of(&flat) <= spin::MAX_RPM + 1e-9);
}

#[test]
fn below_stop_rpm_the_phase_returns_to_idle() {
    let s = spinning_at(6.0, 0.0);
    let (state, _, _) = run(s, 5.0, 0.016, 0.0);
    assert_eq!(state.phase, SpinPhase::Idle);
    assert_eq!(state.omega, 0.0);
}

#[test]
fn a_press_stops_a_spinning_ring_dead_and_swallows_wheel_input_for_the_quiet_period() {
    let s = press(&spinning_at(500.0, 0.0), 10.0);
    assert_eq!(s.phase, SpinPhase::Idle);
    assert_eq!(s.omega, 0.0);
    assert_eq!(s.sparks.len(), 0);
    assert_eq!(wheel(&s, 0.03, 10.2).1, 0.0);
    assert_eq!(wheel(&s, 0.03, 10.5).1, 0.03);
}

#[test]
fn no_sparks_below_sparks_rpm_above_it_the_rate_rises_with_rpm_and_the_live_count_never_exceeds_the_cap() {
    let cold = run(spinning_at(300.0, 0.0), 1.0, 0.016, 0.0).0;
    assert_eq!(cold.sparks.len(), 0);
    let warm = run(spinning_at(600.0, 0.0), 0.5, 0.016, 0.0).0;
    let pouring = run(spinning_at(900.0, 0.0), 0.5, 0.016, 0.0).0;
    assert!(!warm.sparks.is_empty(), "the odd spark");
    assert!(pouring.sparks.len() > 3 * warm.sparks.len(), "setting off: sparks pour");
    assert!(pouring.sparks.len() <= spin::SPARK_MAX_LIVE);
    let long = run(spinning_at(1290.0, 0.0), 3.0, 0.016, 0.0).0;
    assert!(long.sparks.len() <= spin::SPARK_MAX_LIVE);
}

#[test]
fn sparks_are_deterministic_per_seed_and_a_spark_dies_after_spark_life() {
    let a = run(Flywheel { seed: 42, ..spinning_at(900.0, 0.0) }, 0.5, 0.016, 0.0).0;
    let b = run(Flywheel { seed: 42, ..spinning_at(900.0, 0.0) }, 0.5, 0.016, 0.0).0;
    assert_eq!(a.sparks, b.sparks);
    let c = run(Flywheel { seed: 43, ..spinning_at(900.0, 0.0) }, 0.5, 0.016, 0.0).0;
    assert_ne!(a.sparks, c.sparks);
    let Some(p) = a.sparks.first() else { panic!("no sparks at 900 rpm") };
    assert_eq!(spark_at(p, p.born + spin::SPARK_LIFE).alpha, 0.0);
    assert!(spark_at(p, p.born + 0.1).alpha > 0.8);
}

#[test]
fn a_spark_leaves_the_rim_on_the_tangent_in_the_spin_direction() {
    for rpm in [900.0, -900.0] {
        let s = Flywheel { phase: SpinPhase::Spinning, omega: rpm / 60.0, ..idle(9, 0.0) };
        let (state, _, _) = run(s.clone(), 0.2, 0.016, 0.0);
        assert!(!state.sparks.is_empty());
        for p in &state.sparks {
            let tangent = [-p.y, p.x]; // clockwise tangent at (x, y) on a y-down ring
            let along = p.vx * tangent[0] + p.vy * tangent[1];
            assert_eq!(
                along.signum(),
                screen_dir(s.omega),
                "spark should fly {}",
                if screen_dir(s.omega) > 0.0 { "clockwise" } else { "counter-clockwise" }
            );
            approx(p.x.hypot(p.y), 1.0, 1e-9);
        }
    }
}

#[test]
fn the_stages_quiet_hot_ablaze_redline_blur_from_setting_off_to_the_redline_fire_from_fire_rpm_to_the_redline() {
    assert_eq!(tone(100.0), Tone::Quiet);
    assert_eq!(tone(spin::SPARKS_RPM), Tone::Hot);
    assert_eq!(tone(spin::FIRE_RPM), Tone::Ablaze);
    assert_eq!(tone(spin::REDLINE_RPM), Tone::Redline);
    assert_eq!(blur_px(spin::SETTING_OFF_RPM - 1.0), 0.0);
    assert_eq!(blur_px(spin::REDLINE_RPM), spin::BLUR_MAX_PX);
    assert_eq!(blur_px(900.0) % 0.25, 0.0);
    assert_eq!(fire(spin::FIRE_RPM - 1.0), 0.0);
    assert_eq!(fire(spin::REDLINE_RPM), 1.0);
    assert!(fire(1150.0) > 0.0 && fire(1150.0) < 1.0);
}

#[test]
fn past_the_redline_the_ring_leaves_toward_the_spin_side_and_is_gone_after_leave_still_turning_on_the_way() {
    let s = spinning_at(1310.0, 0.0); // clockwise (omega < 0)
    let first = step(&s, 0.016, 0.016);
    assert_eq!(first.0.phase, SpinPhase::Leaving);
    assert_eq!(first.0.side, 1.0);
    let half = run(first.0, spin::LEAVE / 2.0, 0.016, 0.016);
    assert_eq!(half.0.phase, SpinPhase::Leaving);
    assert!(half.0.offset > 0.0 && half.0.offset < 1.0);
    assert!(half.1 < 0.0, "still turning clockwise on the way out");
    let done = run(half.0, spin::LEAVE / 2.0 + 0.05, 0.016, half.2);
    assert_eq!(done.0.phase, SpinPhase::Gone);
    assert_eq!(done.0.offset, 1.0);
    assert_eq!(done.0.omega, 0.0);
    assert_eq!(done.0.sparks.len(), 0);
}

#[test]
fn a_clockwise_spin_leaves_to_the_right_a_counter_clockwise_one_to_the_left() {
    let cw = step(&spinning_at(1310.0, 0.0), 0.016, 0.016).0;
    let ccw = step(&Flywheel { omega: 1310.0 / 60.0, ..spinning_at(1310.0, 0.0) }, 0.016, 0.016).0;
    assert_eq!(cw.side, 1.0);
    assert_eq!(ccw.side, -1.0);
}

#[test]
fn wheel_and_press_do_nothing_while_leaving_gone_or_returning() {
    let leaving = step(&spinning_at(1310.0, 0.0), 0.016, 0.016).0;
    assert_eq!(press(&leaving, 1.0), leaving);
    let gone = run(leaving, spin::LEAVE + 0.1, 0.016, 0.016).0;
    assert_eq!(gone.phase, SpinPhase::Gone);
    assert_eq!(wheel(&gone, 0.03, 2.0).1, 0.0);
    assert_eq!(wheel(&gone, 0.03, 2.0).0, gone);
    assert_eq!(press(&gone, 2.0), gone);
    let back = fetch(&gone, 2.0);
    assert_eq!(wheel(&back, 0.03, 2.1).1, 0.0);
    assert_eq!(press(&back, 2.1), back);
}

#[test]
fn fetch_only_works_from_gone_the_return_decelerates_to_rest_and_ends_idle_at_home() {
    assert_eq!(fetch(&spinning_at(300.0, 0.0), 1.0).phase, SpinPhase::Spinning);
    let leaving = step(&spinning_at(1310.0, 0.0), 0.016, 0.016).0;
    let gone = run(leaving, spin::LEAVE + 0.1, 0.016, 0.016).0;
    let back = fetch(&gone, 3.0);
    assert_eq!(back.phase, SpinPhase::Returning);
    assert_eq!(back.offset, -1.0, "comes in from the side it did not leave by");
    let early = step(&back, 0.016, 3.016);
    let late = step(&run(back.clone(), spin::RETURN * 0.8, 0.016, 3.0).0, 0.016, 3.0 + spin::RETURN * 0.8 + 0.016);
    assert!(early.1.abs() > late.1.abs(), "slowing down");
    assert!(early.1 < 0.0, "moving right is clockwise");
    let home = run(back, spin::RETURN + 0.05, 0.016, 3.0).0;
    assert_eq!(home.phase, SpinPhase::Idle);
    assert_eq!(home.offset, 0.0);
    assert_eq!(home.omega, 0.0);
}
