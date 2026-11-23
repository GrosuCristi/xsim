//! Full-simulation tests of the voltage-controlled switch: on/off
//! resistances, hysteresis in DC sweeps and transients (exactly at the
//! thresholds, and a relaxation oscillator), LTspice-style smooth mode and
//! the small-signal model.

use xsim_spice_engine::*;

fn run(circuit: &Circuit, analysis: &Analysis) -> SimulationResult {
    simulate(
        circuit,
        analysis,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .expect("simulation failed")
}

fn assert_close(actual: f64, expected: f64, rel: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= rel * expected.abs(),
        "{what}: got {actual:e}, expected {expected:e}"
    );
}

fn model(vt: f64, vh: f64) -> SwitchModel {
    SwitchModel {
        name: "SW".into(),
        ron: 10.0,
        roff: 1e7,
        vt,
        vh,
    }
}

/// `V1 (1 V) → switch → ground`, controlled by source `VC`.
fn switched(control: Source, model: SwitchModel) -> Circuit {
    let mut c = Circuit::new();
    let a = c.node("a");
    let ctl = c.node("ctl");
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new(
        "VC",
        [ctl, GROUND],
        Device::VoltageSource(control),
    ));
    c.add(Element::new(
        "S1",
        [a, GROUND, ctl, GROUND],
        Device::VoltageSwitch { model },
    ));
    c
}

#[test]
fn switch_conducts_through_ron_or_roff() {
    for (control, resistance) in [(0.0, 1e7), (2.0, 10.0)] {
        let r = run(
            &switched(Source::dc(control), model(1.0, 0.0)),
            &Analysis::OperatingPoint,
        );
        let current = r.signal("I(S1)").unwrap().real().unwrap()[0];
        assert_close(
            current,
            1.0 / resistance,
            1e-12,
            &format!("current at control {control} V"),
        );
    }
}

/// Switch current along a DC sweep of the control voltage.
fn sweep(model: SwitchModel, start: f64, stop: f64, step: f64) -> (Vec<f64>, Vec<f64>) {
    let analysis = Analysis::DcSweep(DcSweep {
        source: "VC".into(),
        start,
        stop,
        step,
    });
    let r = run(&switched(Source::dc(0.0), model), &analysis);
    (
        r.axis.values.clone(),
        r.signal("I(S1)").unwrap().real().unwrap().to_vec(),
    )
}

/// First control voltage of the sweep at which the switch state flipped.
fn transition(control: &[f64], current: &[f64], on_now: bool) -> f64 {
    let threshold = 1e-3; // between 1/roff = 1e-7 A and 1/ron = 0.1 A
    control
        .iter()
        .zip(current)
        .find(|&(_, &i)| (i > threshold) == on_now)
        .map(|(&v, _)| v)
        .expect("switch never changed state")
}

#[test]
fn dc_sweeps_show_hysteresis() {
    let m = model(1.0, 0.2);
    let (up, i_up) = sweep(m.clone(), 0.0, 2.0, 0.01);
    let (down, i_down) = sweep(m, 2.0, 0.0, -0.01);
    // Rising, the switch stays off through 1.2 V and turns on just above it;
    // falling, it stays on through 0.8 V and turns off just below it.
    let on_at = transition(&up, &i_up, true);
    assert!(on_at > 1.2 && on_at < 1.22, "turned on at {on_at} V");
    let off_at = transition(&down, &i_down, false);
    assert!(off_at < 0.8 && off_at > 0.78, "turned off at {off_at} V");
}

#[test]
fn zero_hysteresis_switches_at_the_threshold_both_ways() {
    let m = model(1.0, 0.0);
    let (up, i_up) = sweep(m.clone(), 0.0, 2.0, 0.01);
    let (down, i_down) = sweep(m, 2.0, 0.0, -0.01);
    let on_at = transition(&up, &i_up, true);
    let off_at = transition(&down, &i_down, false);
    assert!(
        (on_at - 1.0).abs() < 0.011 && (off_at - 1.0).abs() < 0.011,
        "{on_at} / {off_at}"
    );
}

#[test]
fn smooth_mode_interpolates_resistance_logarithmically() {
    // Negative vh: no hysteresis, transition over vt ± |vh|; the midpoint is
    // the geometric mean of the two resistances.
    let r = run(
        &switched(Source::dc(1.0), model(1.0, -0.5)),
        &Analysis::OperatingPoint,
    );
    let current = r.signal("I(S1)").unwrap().real().unwrap()[0];
    assert_close(
        current,
        1.0 / (10.0f64 * 1e7).sqrt(),
        1e-9,
        "midpoint conductance",
    );
}

/// `V3 (3 V, AC 1) → R3 (1 kΩ) → o3`, a smooth-mode switch from `o3` to
/// ground controlled by `o3` itself, and `V4 (3 V) → R4 (1 kΩ) → o4`, a
/// smooth-mode switch from `o4` to ground controlled by `VC4 (AC 1)`.
fn smooth_pair(v3: f64, vc4: f64) -> Circuit {
    let smooth = SwitchModel {
        name: "SWS".into(),
        ron: 100.0,
        roff: 100e3,
        vt: 1.0,
        vh: -0.5,
    };
    let mut c = Circuit::new();
    let (a3, o3, a4, o4, c4) = (
        c.node("a3"),
        c.node("o3"),
        c.node("a4"),
        c.node("o4"),
        c.node("c4"),
    );
    c.add(Element::new(
        "V3",
        [a3, GROUND],
        Device::VoltageSource(Source::ac(v3, 1.0, 0.0)),
    ));
    c.add(Element::new("R3", [a3, o3], Device::resistor(1e3)));
    c.add(Element::new(
        "S3",
        [o3, GROUND, o3, GROUND],
        Device::VoltageSwitch {
            model: smooth.clone(),
        },
    ));
    c.add(Element::new(
        "V4",
        [a4, GROUND],
        Device::VoltageSource(Source::dc(3.0)),
    ));
    c.add(Element::new("R4", [a4, o4], Device::resistor(1e3)));
    c.add(Element::new(
        "S4",
        [o4, GROUND, c4, GROUND],
        Device::VoltageSwitch { model: smooth },
    ));
    c.add(Element::new(
        "VC4",
        [c4, GROUND],
        Device::VoltageSource(Source::ac(vc4, 1.0, 0.0)),
    ));
    c
}

#[test]
fn smooth_mode_small_signal_is_the_slope_of_the_dc_curve() {
    // LTspice 24.1.9 batch runs of the same circuit,
    //   V3 a3 0 DC 3 / R3 a3 o3 1k / S3 o3 0 o3 0 SWS
    //   V4 a4 0 DC 3 / R4 a4 o4 1k / S4 o4 0 c4 0 SWS / VC4 c4 0 DC 1.2
    //   .model SWS SW(Ron=100 Roff=100k Vt=1 Vh=-0.5)
    //   .options reltol=1e-12 abstol=1e-18 vntol=1e-15 gmin=1e-12 numdgt=15
    // with `.dc V3 2.999 3.001 0.001` and `.dc VC4 1.199 1.201 0.001`,
    // central differences over ±1 mV: V(o3) = 1.1610114074 / 1.16111313125 V
    // at V3 = 2.999 / 3.001 V, so dV(o3)/dV3 = 0.050861925; V(o4) =
    // 0.928923182264 / 0.917797058509 V at VC4 = 1.199 / 1.201 V, so
    // dV(o4)/dVC4 = -5.5630618775.
    // (LTspice's own `.ac` gives 0.0513076 and -5.50744, about 1 % off its
    // DC slopes, so it is not the reference here.)
    let options = SimOptions {
        reltol: 1e-12,
        abstol: 1e-18,
        vntol: 1e-15,
        ..SimOptions::default()
    };
    let voltage = |c: &Circuit, name: &str| {
        let r = simulate(c, &Analysis::OperatingPoint, &options, &SimControl::new()).unwrap();
        r.signal(name).unwrap().real().unwrap()[0]
    };
    let slope3 = (voltage(&smooth_pair(3.001, 1.2), "V(o3)")
        - voltage(&smooth_pair(2.999, 1.2), "V(o3)"))
        / 0.002;
    let slope4 = (voltage(&smooth_pair(3.0, 1.201), "V(o4)")
        - voltage(&smooth_pair(3.0, 1.199), "V(o4)"))
        / 0.002;
    let ac = simulate(
        &smooth_pair(3.0, 1.2),
        &Analysis::Ac(AcSweep {
            scale: SweepScale::Linear,
            points: 1,
            start: 1e3,
            stop: 1e3,
        }),
        &options,
        &SimControl::new(),
    )
    .unwrap();
    // VC4 has no effect on o3 and V3 none on o4, so both sources can carry
    // AC 1 at once.
    for (name, slope, ltspice) in [
        ("V(o3)", slope3, 0.050_861_925),
        ("V(o4)", slope4, -5.563_061_877_5),
    ] {
        let SignalData::Complex { re, im } = &ac.signal(name).unwrap().data else {
            panic!("{name} is not complex");
        };
        assert_eq!(im[0], 0.0, "{name}");
        // A central difference over ±1 mV is exact to its O(h²) term.
        assert_close(re[0], slope, 1e-5, &format!("{name}: AC vs DC slope"));
        assert_close(
            re[0],
            ltspice,
            1e-5,
            &format!("{name}: AC vs LTspice DC slope"),
        );
        // LTspice prints 12 digits, which leaves its slopes good to ~1e-7.
        assert_close(
            slope,
            ltspice,
            1e-6,
            &format!("{name}: DC slope vs LTspice"),
        );
    }
}

#[test]
fn transient_triangle_switches_at_hysteresis_thresholds() {
    let triangle = Waveform::Pwl {
        points: vec![(0.0, 0.0), (1e-3, 2.0), (2e-3, 0.0)],
        repeat: false,
    };
    let c = switched(Source::with_waveform(triangle), model(1.0, 0.2));
    let spec = Analysis::Transient(Transient {
        stop: 2e-3,
        step: 1e-6,
        start_save: 0.0,
        max_step: Some(1e-6),
        initial_state: InitialState::OperatingPoint,
    });
    let r = run(&c, &spec);
    let t = &r.axis.values;
    let i = r.signal("I(S1)").unwrap().real().unwrap();
    let on = t
        .iter()
        .zip(i)
        .find(|&(_, &i)| i > 1e-3)
        .map(|(&t, _)| t)
        .unwrap();
    let off = t
        .iter()
        .zip(i)
        .skip_while(|&(&t, _)| t < 1e-3)
        .find(|&(_, &i)| i < 1e-3)
        .map(|(&t, _)| t)
        .unwrap();
    // The control ramps at 2 V/ms: 1.2 V at 0.6 ms rising, 0.8 V at 1.6 ms falling.
    assert!((on - 0.6e-3).abs() < 2e-6, "turned on at {on}");
    assert!((off - 1.6e-3).abs() < 2e-6, "turned off at {off}");
}

#[test]
fn closed_switch_is_a_resistor_in_ac() {
    let mut c = Circuit::new();
    let a = c.node("a");
    let ctl = c.node("ctl");
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
    ));
    c.add(Element::new(
        "VC",
        [ctl, GROUND],
        Device::VoltageSource(Source::dc(5.0)),
    ));
    c.add(Element::new(
        "S1",
        [a, GROUND, ctl, GROUND],
        Device::VoltageSwitch {
            model: model(1.0, 0.1),
        },
    ));
    let spec = AcSweep {
        scale: SweepScale::Linear,
        points: 1,
        start: 1e3,
        stop: 1e3,
    };
    let r = run(&c, &Analysis::Ac(spec));
    match &r.signal("I(S1)").unwrap().data {
        SignalData::Complex { re, im } => {
            assert_close(re[0], 0.1, 1e-12, "on-state AC conductance");
            assert_eq!(im[0], 0.0);
        }
        SignalData::Real(_) => panic!("AC result must be complex"),
    }
}

// ------------------------------------- exactly at the thresholds (DCAC-4)

/// `V1 (5 V) → R1 (1 kΩ) → o`, switch from `o` to ground controlled by `VC`
/// (`ron` 10 Ω, `roff` 1 MΩ, `vt` 1 V).
fn divider(vh: f64) -> Circuit {
    let mut c = Circuit::new();
    let (ctl, a, o) = (c.node("ctl"), c.node("a"), c.node("o"));
    c.add(Element::new(
        "VC",
        [ctl, GROUND],
        Device::VoltageSource(Source::dc(0.0)),
    ));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(5.0)),
    ));
    c.add(Element::new("R1", [a, o], Device::resistor(1e3)));
    c.add(Element::new(
        "S1",
        [o, GROUND, ctl, GROUND],
        Device::VoltageSwitch {
            model: SwitchModel {
                name: "SWM".into(),
                ron: 10.0,
                roff: 1e6,
                vt: 1.0,
                vh,
            },
        },
    ));
    c
}

#[test]
fn dc_sweeps_keep_the_state_exactly_at_the_thresholds() {
    // LTspice 24.1.9, V1 a 0 5 / R1 a o 1k / S1 o 0 ctl 0 SWM /
    // .model SWM SW(Ron=10 Roff=1e6 Vt=1 Vh=0.2) (and Vh=0), `.dc VC` over
    // each range: V(o) = 4.99500499001 (off) or 0.0495049504946 (on).
    // Rising, the switch is still off at exactly 1.2 V and on just above;
    // falling, still on at exactly 0.8 V and off just below. Without
    // hysteresis it keeps its state exactly at 1 V.
    let (off, on) = (5.0 * 1e6 / (1e6 + 1e3), 5.0 * 10.0 / (10.0 + 1e3));
    for (vh, start, stop, step) in [
        (0.2, 0.0, 2.0, 0.01),
        (0.2, 2.0, 0.0, -0.01),
        (0.2, 1.1, 2.0, 0.1),
        (0.0, 0.0, 2.0, 0.01),
        (0.0, 2.0, 0.0, -0.01),
    ] {
        let analysis = Analysis::DcSweep(DcSweep {
            source: "VC".into(),
            start,
            stop,
            step,
        });
        let r = run(&divider(vh), &analysis);
        let vo = r.signal("V(o)").unwrap().real().unwrap();
        let mut state = false;
        let mut at_threshold = 0;
        for (&vc, &v) in r.axis.values.iter().zip(vo) {
            let (up, down) = (1.0 + vh, 1.0 - vh);
            at_threshold += usize::from(vc == up || vc == down);
            state = if vc > up {
                true
            } else if vc < down {
                false
            } else {
                state
            };
            let expected = if state { on } else { off };
            assert_close(v, expected, 1e-9, &format!("vh={vh}: V(o) at VC = {vc:?}"));
        }
        // Every sweep but the coarse one lands exactly on a threshold.
        assert!(at_threshold > 0 || step == 0.1, "vh={vh}, {start} → {stop}");
    }
}

/// `divider` with `vt = −1 V` and `vh = 0.5 V`, a hysteresis band from
/// −1.5 V to −0.5 V that contains 0 V, and the control at `vc`.
fn negative_band_divider(vc: f64) -> Circuit {
    let mut c = divider(0.5);
    for element in &mut c.elements {
        match &mut element.device {
            Device::VoltageSwitch { model } => model.vt = -1.0,
            Device::VoltageSource(source) if element.name == "VC" => *source = Source::dc(vc),
            _ => {}
        }
    }
    c
}

#[test]
fn a_latch_whose_band_contains_zero_volts_starts_off() {
    // LTspice 24.1.9, the `divider` deck with .model SWM SW(Ron=10 Roff=1e6
    // Vt=-1 Vh=0.5): with VC = -1.2 V, `.op` and the operating point of a
    // `.tran` give V(o) = 4.99500499001 (off). `.dc VC -1.25 0.25 0.25` is
    // off up to and including -0.5 V and on above; `.dc VC 0.25 -1.75 -0.25`
    // is on down to and including -1.5 V and off below. The starting guess
    // of a fresh solve, 0 V and so above vt + vh, must not turn it on.
    let (off, on) = (5.0 * 1e6 / (1e6 + 1e3), 5.0 * 10.0 / (10.0 + 1e3));
    let circuit = negative_band_divider(-1.2);
    let r = run(&circuit, &Analysis::OperatingPoint);
    assert_close(
        r.signal("V(o)").unwrap().real().unwrap()[0],
        off,
        1e-9,
        ".op",
    );
    let tran = run(
        &circuit,
        &Analysis::Transient(Transient {
            stop: 1e-6,
            step: 1e-7,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::OperatingPoint,
        }),
    );
    for &v in tran.signal("V(o)").unwrap().real().unwrap() {
        assert_close(v, off, 1e-9, ".tran from the operating point");
    }
    let rising = |vc: f64| if vc > -0.5 { on } else { off };
    let falling = |vc: f64| if vc < -1.5 { off } else { on };
    for (start, stop, step, expected) in [
        (-1.25, 0.25, 0.25, &rising as &dyn Fn(f64) -> f64),
        (0.25, -1.75, -0.25, &falling),
    ] {
        let analysis = Analysis::DcSweep(DcSweep {
            source: "VC".into(),
            start,
            stop,
            step,
        });
        let r = run(&circuit, &analysis);
        let vo = r.signal("V(o)").unwrap().real().unwrap();
        assert_eq!(vo.len(), if step > 0.0 { 7 } else { 9 });
        for (&vc, &v) in r.axis.values.iter().zip(vo) {
            assert_close(v, expected(vc), 1e-9, &format!("V(o) at VC = {vc}"));
        }
    }
}

// --------------------------------------- relaxation oscillator (T1)

/// `tests/reference/devices_switch_oscillator.cir`: C1 charges through R1
/// until the switch turns on above 3 V, which discharges it to 2 V.
fn oscillator() -> Circuit {
    let mut c = Circuit::new();
    let (vcc, n) = (c.node("vcc"), c.node("c"));
    c.add(Element::new(
        "VCC",
        [vcc, GROUND],
        Device::VoltageSource(Source::dc(5.0)),
    ));
    c.add(Element::new("R1", [vcc, n], Device::resistor(10e3)));
    c.add(Element::new("C1", [n, GROUND], Device::capacitor(100e-9)));
    c.add(Element::new(
        "S1",
        [n, GROUND, n, GROUND],
        Device::VoltageSwitch {
            model: SwitchModel {
                name: "SW1".into(),
                ron: 10.0,
                roff: 1e9,
                vt: 2.5,
                vh: 0.5,
            },
        },
    ));
    c
}

/// Rising crossings of `level`, linearly interpolated.
fn rising_crossings(t: &[f64], v: &[f64], level: f64) -> Vec<f64> {
    (1..v.len())
        .filter(|&j| v[j - 1] < level && v[j] >= level)
        .map(|j| t[j - 1] + (level - v[j - 1]) * (t[j] - t[j - 1]) / (v[j] - v[j - 1]))
        .collect()
}

#[test]
fn hysteretic_switch_discharging_its_own_control_oscillates() {
    // Closed form (with Roff = 1 GΩ in parallel while charging): charging
    // 2 → 3 V takes Rth·C·ln((V∞ − 2)/(V∞ − 3)), discharging through
    // Ron ‖ R1 takes (Ron ‖ R1)·C·ln((3 − V∞')/(2 − V∞')).
    let (r, c, ron, roff): (f64, f64, f64, f64) = (10e3, 100e-9, 10.0, 1e9);
    let charge = {
        let (rth, vinf) = (r * roff / (r + roff), 5.0 * roff / (r + roff));
        rth * c * ((vinf - 2.0) / (vinf - 3.0)).ln()
    };
    let discharge = {
        let (rth, vinf) = (r * ron / (r + ron), 5.0 * ron / (r + ron));
        rth * c * ((3.0 - vinf) / (2.0 - vinf)).ln()
    };
    let period = charge + discharge;
    assert!((period - 0.405_875_28e-3).abs() < 1e-11);
    // LTspice 24.1.9 batch run of the reference deck (0.1 µs maximum step):
    // t(RISE=2) = 1.09902422983972 ms, t(RISE=7) = 3.1281718304203 ms, a
    // period of 0.4058295 ms, 1.1e-4 below the closed form; V(c) between
    // 1.99963519 V and 2.99999984 V.
    let ltspice = (3.128_171_830_420_3e-3 - 1.099_024_229_839_72e-3) / 5.0;
    // Every cycle agrees with the closed form to about 1.3e-6 (6e-5 with
    // backward Euler at reltol 1e-4), and the swing stays within 0.6 µV of
    // the thresholds: the switch changes state at its located threshold
    // crossing, so no step can carry it past the threshold.
    for (method, reltol, tolerance) in [
        (IntegrationMethod::Trapezoidal, 1e-6, 1e-5),
        (IntegrationMethod::Gear2, 1e-6, 1e-5),
        (IntegrationMethod::ModifiedTrapezoidal, 1e-6, 1e-5),
        (IntegrationMethod::BackwardEuler, 1e-4, 2e-4),
    ] {
        let spec = Analysis::Transient(Transient {
            stop: 5e-3,
            step: 1e-6,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::ZeroState,
        });
        let options = SimOptions {
            method,
            reltol,
            ..SimOptions::default()
        };
        let result = simulate(&oscillator(), &spec, &options, &SimControl::new())
            .unwrap_or_else(|e| panic!("{method:?}: {e}"));
        let t = &result.axis.values;
        let v = result.signal("V(c)").unwrap().real().unwrap();
        let rises = rising_crossings(t, v, 2.5);
        assert!(
            rises.len() >= 11,
            "{method:?}: {} rising edges",
            rises.len()
        );
        // The first rise from 0 V: RC·ln(5/2.5) with Roff in parallel.
        assert_close(rises[0], 0.693_150_249e-3, tolerance, "first rise");
        let periods: Vec<f64> = rises.windows(2).skip(1).map(|w| w[1] - w[0]).collect();
        for (k, &p) in periods.iter().enumerate() {
            let what = format!("{method:?}: period {}", k + 1);
            assert_close(p, period, tolerance, &what);
        }
        let mean = (rises[rises.len() - 1] - rises[1]) / (rises.len() - 2) as f64;
        assert_close(mean, ltspice, 2e-4, &format!("{method:?}: vs LTspice"));
        // The swing stays between the thresholds.
        let late = t.iter().zip(v).filter(|&(&time, _)| time > 2e-3);
        let (low, high) = late.fold((f64::MAX, f64::MIN), |(lo, hi), (_, &x)| {
            (lo.min(x), hi.max(x))
        });
        assert!(
            (low - 2.0).abs() < 2e-6 && (high - 3.0).abs() < 2e-6,
            "{method:?}: V(c) between {low} and {high}"
        );
    }
}

#[test]
fn hysteretic_oscillator_has_no_operating_point() {
    // As in LTspice, which needs `uic` for the reference deck: no DC state
    // satisfies the latch, and the solver reports that instead of returning
    // a point between the thresholds.
    let spec = Analysis::Transient(Transient {
        stop: 1e-3,
        step: 1e-6,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::OperatingPoint,
    });
    let error = simulate(
        &oscillator(),
        &spec,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap_err();
    assert!(matches!(error, SimError::NoConvergence { .. }), "{error}");
}

/// The closed-form period of [`oscillator`] with `ron` and `c` (R1 = 10 kΩ,
/// Roff = 1 GΩ, 5 V, switching at 3 V and 2 V).
fn oscillator_period(ron: f64, c: f64) -> f64 {
    let (r, roff) = (10e3, 1e9);
    let phase = |load: f64, from: f64, to: f64| {
        let (rth, vinf) = (r * load / (r + load), 5.0 * load / (r + load));
        rth * c * ((vinf - from) / (vinf - to)).ln()
    };
    phase(roff, 2.0, 3.0) + phase(ron, 3.0, 2.0)
}

#[test]
fn a_near_ideal_switch_oscillator_keeps_the_closed_form_period_with_every_method() {
    // Ron = 10 mΩ and C1 = 1 µF discharge from 3 V to 2 V in 4 ns at 300 A.
    // With the default trapezoidal rule this used to be silently wrong: a
    // step that the 300 A peak let through crossed the whole switching
    // transition, the trapezoidal rule rang on it, and the 50 ms run gave 7
    // rises with periods 84.5 % too long and V(c) dipping to 0.775 V (Gear-2
    // and TR-BDF2 were 1.3e-3 off). The switch now changes state at its
    // located threshold crossing and the integration restarts there, as at
    // a breakpoint. Measured: every cycle within 1.6e-6 of the closed form
    // (6e-5 with backward Euler at reltol 1e-4) for Ron from 10 mΩ to 100 Ω,
    // also when the output step leaves the step control alone (1 ms here),
    // where the recharge's tolerance must not stay relative to the
    // discharge current (3e-3 off before).
    let methods = [
        (IntegrationMethod::Trapezoidal, 1e-6, 1e-5),
        (IntegrationMethod::Gear2, 1e-6, 1e-5),
        (IntegrationMethod::ModifiedTrapezoidal, 1e-6, 1e-5),
        (IntegrationMethod::BackwardEuler, 1e-4, 1e-4),
    ];
    let cases = [
        (0.01, 10e-6, &methods[..]),
        (1.0, 10e-6, &methods[..3]),
        (100.0, 10e-6, &methods[..]),
        (0.01, 1e-3, &methods[..1]),
    ];
    for (ron, step, methods) in cases {
        let mut circuit = oscillator();
        for element in &mut circuit.elements {
            match &mut element.device {
                Device::Capacitor { capacitance, .. } => *capacitance = 1e-6,
                Device::VoltageSwitch { model } => model.ron = ron,
                _ => {}
            }
        }
        let period = oscillator_period(ron, 1e-6);
        for &(method, reltol, tolerance) in methods {
            let spec = Analysis::Transient(Transient {
                stop: 50e-3,
                step,
                start_save: 0.0,
                max_step: None,
                initial_state: InitialState::ZeroState,
            });
            let options = SimOptions {
                method,
                reltol,
                ..SimOptions::default()
            };
            let what = format!("Ron {ron} Ω, step {step} s, {method:?}");
            let result = simulate(&circuit, &spec, &options, &SimControl::new())
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            let t = &result.axis.values;
            let v = result.signal("V(c)").unwrap().real().unwrap();
            let rises = rising_crossings(t, v, 2.5);
            assert_eq!(rises.len(), 11, "{what}: rising edges");
            for (k, w) in rises.windows(2).skip(1).enumerate() {
                let p = w[1] - w[0];
                assert_close(p, period, tolerance, &format!("{what}: period {}", k + 1));
            }
            let late = t.iter().zip(v).filter(|&(&time, _)| time > 10e-3);
            let (low, high) = late.fold((f64::MAX, f64::MIN), |(lo, hi), (_, &x)| {
                (lo.min(x), hi.max(x))
            });
            assert!(
                (low - 2.0).abs() < 2e-6 && (high - 3.0).abs() < 2e-6,
                "{what}: V(c) between {low} and {high}"
            );
        }
    }
}

// ------------------------------------------------ fast gate edge (buck)

/// `tests/reference/devices_switch_buck.cir`: a 10 mΩ switch connects 12 V
/// to a Schottky-clamped switch node for one 4 µs gate pulse with 10 ns
/// edges at t = 0; 10 µH into 10 µF ‖ 5 Ω.
fn buck_stage() -> Circuit {
    let mut c = Circuit::new();
    let (vin, sw, out, g) = (c.node("vin"), c.node("sw"), c.node("out"), c.node("g"));
    c.add(Element::new(
        "VIN",
        [vin, GROUND],
        Device::VoltageSource(Source::dc(12.0)),
    ));
    c.add(Element::new(
        "S1",
        [vin, sw, g, GROUND],
        Device::VoltageSwitch {
            model: SwitchModel {
                name: "SWB".into(),
                ron: 0.01,
                roff: 1e6,
                vt: 2.5,
                vh: 0.0,
            },
        },
    ));
    let schottky = DiodeModel {
        is: 2.3e-6,
        n: 1.05,
        rs: 0.1,
        cjo: 150e-12,
        vj: 0.5,
        m: 0.5,
        bv: 40.0,
        ibv: 0.5e-3,
        eg: 0.69,
        xti: 2.0,
        ..DiodeModel::default()
    };
    c.add(Element::new("D1", [GROUND, sw], Device::diode(schottky)));
    c.add(Element::new("L1", [sw, out], Device::inductor(10e-6)));
    c.add(Element::new("C1", [out, GROUND], Device::capacitor(10e-6)));
    c.add(Element::new("RL", [out, GROUND], Device::resistor(5.0)));
    let gate = Waveform::Pulse {
        initial: 0.0,
        pulsed: 5.0,
        delay: 0.0,
        rise: 10e-9,
        fall: 10e-9,
        width: 4e-6,
        period: 10e-6,
        cycles: Some(1),
    };
    c.add(Element::new(
        "VG",
        [g, GROUND],
        Device::VoltageSource(Source::with_waveform(gate)),
    ));
    c
}

/// `values` at `time`, linearly interpolated.
fn sample(times: &[f64], values: &[f64], time: f64) -> f64 {
    let j = times
        .partition_point(|&t| t < time)
        .clamp(1, times.len() - 1);
    let (t0, t1) = (times[j - 1], times[j]);
    values[j - 1] + (time - t0) * (values[j] - values[j - 1]) / (t1 - t0)
}

/// `(I(L1), V(out))` of [`buck_stage`] at `time` while the switch is on,
/// from the closed-form solution of `L·i' = 12 − ron·i − v`,
/// `C·v' = i − v/RL` from rest at the threshold crossing (5 ns).
fn buck_on_phase(time: f64) -> (f64, f64) {
    use num_complex::Complex64;
    let (l, c, rl, ron, vs) = (10e-6, 10e-6, 5.0, 0.01, 12.0);
    let a = [[-ron / l, -1.0 / l], [1.0 / c, -1.0 / (rl * c)]];
    // Steady state, then e^(A·t) by Sylvester's formula.
    let steady = [vs / (ron + rl), vs * rl / (ron + rl)];
    let (trace, det) = (a[0][0] + a[1][1], a[0][0] * a[1][1] - a[0][1] * a[1][0]);
    let root = Complex64::new(trace * trace / 4.0 - det, 0.0).sqrt();
    let (l1, l2) = (trace / 2.0 + root, trace / 2.0 - root);
    let t = time - 5e-9;
    let (e1, e2) = ((l1 * t).exp(), (l2 * t).exp());
    let exp = |r: usize, k: usize| {
        let identity = if r == k { 1.0 } else { 0.0 };
        ((e1 * (a[r][k] - l2 * identity) - e2 * (a[r][k] - l1 * identity)) / (l1 - l2)).re
    };
    let state = |r: usize| steady[r] - exp(r, 0) * steady[0] - exp(r, 1) * steady[1];
    (state(0), state(1))
}

#[test]
fn a_fast_gate_edge_early_in_a_run_switches() {
    // The gate crosses the threshold at 5e8 V/s at t = 5 ns: the control
    // moves through the switch's 1 µV resolution in 2 fs. A switch that
    // resolved its change of state over that time stopped with "Timestep
    // too small at t = 5 ns" at the minimum timestep used here (3e-18 s,
    // that of a 3 ms run at the default `min_step_ratio`; a 1 ms buck run
    // failed the same way). The switch now changes state at the located
    // crossing, at an accepted point, whatever the edge rate.
    // While the switch is on the circuit is linear: the closed form is the
    // oracle. LTspice 24.1.9 batch run of the reference deck: I(L1) =
    // 3.53547405961 A at 3 µs and 4.65972904113 A at 4 µs, V(out) =
    // 0.918975520557 V at 4 µs; after the switch opens, I(L1) =
    // 4.47943017037 A and V(out) = 1.35444636368 V at 5 µs. LTspice turns on
    // about 0.5 ns late (1.5e-4 to 2e-4 below the closed form).
    let result = simulate(
        &buck_stage(),
        &Analysis::Transient(Transient {
            stop: 5e-6,
            step: 50e-9,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::ZeroState,
        }),
        &SimOptions {
            min_step_ratio: 6e-13,
            ..SimOptions::default()
        },
        &SimControl::new(),
    )
    .expect("simulation failed");
    let t = &result.axis.values;
    let v = result.signal("V(out)").unwrap().real().unwrap();
    let il = result.signal("I(L1)").unwrap().real().unwrap();
    for time in [1e-6, 3e-6, 4e-6] {
        let (current, voltage) = buck_on_phase(time);
        assert_close(
            sample(t, il, time),
            current,
            1e-6,
            &format!("I(L1) at {time} s"),
        );
        assert_close(
            sample(t, v, time),
            voltage,
            1e-6,
            &format!("V(out) at {time} s"),
        );
    }
    assert_close(
        buck_on_phase(4e-6).0,
        4.660_658_833_749,
        1e-12,
        "closed form",
    );
    for (signal, values, time, ltspice) in [
        ("I(L1)", il, 3e-6, 3.535_474_059_61),
        ("I(L1)", il, 4e-6, 4.659_729_041_13),
        ("V(out)", v, 4e-6, 0.918_975_520_557),
        ("I(L1)", il, 5e-6, 4.479_430_170_37),
        ("V(out)", v, 5e-6, 1.354_446_363_68),
    ] {
        let what = format!("{signal} at {time} s vs LTspice");
        assert_close(sample(t, values, time), ltspice, 5e-4, &what);
    }
}

// ------------------------------------------- ideal and late gate edges

/// `VIN` (1 V) through S1 (1 Ω on, vt = 2.5 V, no hysteresis) into
/// R2 = 1 kΩ ‖ C2 = 1 nF, gated by a 0 → 5 V pulse at `delay` lasting
/// `width` with `rise` edges.
fn gated_rc(delay: f64, rise: f64, width: f64) -> Circuit {
    let mut c = Circuit::new();
    let (vin, o, g) = (c.node("vin"), c.node("o"), c.node("g"));
    c.add(Element::new(
        "VIN",
        [vin, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new(
        "S1",
        [vin, o, g, GROUND],
        Device::VoltageSwitch {
            model: SwitchModel {
                name: "SWG".into(),
                ron: 1.0,
                roff: 1e9,
                vt: 2.5,
                vh: 0.0,
            },
        },
    ));
    c.add(Element::new("R2", [o, GROUND], Device::resistor(1e3)));
    c.add(Element::new("C2", [o, GROUND], Device::capacitor(1e-9)));
    let gate = Waveform::Pulse {
        initial: 0.0,
        pulsed: 5.0,
        delay,
        rise,
        fall: rise,
        width,
        period: 0.0,
        cycles: None,
    };
    c.add(Element::new(
        "VG",
        [g, GROUND],
        Device::VoltageSource(Source::with_waveform(gate)),
    ));
    c
}

/// Time constant and final value of [`gated_rc`] while the switch is on.
const GATED_TAU: f64 = 1e3 / 1001.0 * 1e-9;
const GATED_ON: f64 = 1000.0 / 1001.0;

#[test]
fn an_ideal_gate_edge_switches_at_the_edge() {
    // A zero-rise gate jumps from 0 to 5 V at 1 µs and back at 3 µs: the
    // control is the same at the end of every shorter retry, so the switch
    // changes state right after the edge instead of halving the step down
    // to the minimum. Closed form: V(o) charges with τ = (1 Ω ‖ 1 kΩ)·1 nF
    // from 1 µs and decays with R2·C2 = 1 µs from 3 µs. Measured: within
    // 1e-6 with every method (backward Euler at reltol 1e-4: 5e-4).
    let tau_off: f64 = 1e3 * 1e-9 * (1e9 / (1e9 + 1e3));
    for (method, reltol, tolerance) in [
        (IntegrationMethod::Trapezoidal, 1e-6, 1e-5),
        (IntegrationMethod::Gear2, 1e-6, 1e-5),
        (IntegrationMethod::ModifiedTrapezoidal, 1e-6, 1e-5),
        (IntegrationMethod::BackwardEuler, 1e-4, 2e-3),
    ] {
        let options = SimOptions {
            method,
            reltol,
            ..SimOptions::default()
        };
        let spec = Analysis::Transient(Transient {
            stop: 5e-6,
            step: 10e-9,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::OperatingPoint,
        });
        let r = simulate(
            &gated_rc(1e-6, 0.0, 2e-6),
            &spec,
            &options,
            &SimControl::new(),
        )
        .unwrap_or_else(|e| panic!("{method:?}: {e}"));
        let t = &r.axis.values;
        let v = r.signal("V(o)").unwrap().real().unwrap();
        for k in [0.5_f64, 1.0, 2.0, 5.0, 20.0] {
            let time = 1e-6 + k * GATED_TAU;
            let exact = -GATED_ON * (-k).exp_m1();
            let what = format!("{method:?}: V(o) {k} τ after the rising edge");
            assert!((sample(t, v, time) - exact).abs() < tolerance, "{what}");
        }
        for time in [3.1e-6_f64, 3.5e-6, 4.9e-6] {
            let exact = GATED_ON * (-(time - 3e-6) / tau_off).exp();
            let what = format!("{method:?}: V(o) at {time} s after the falling edge");
            assert!((sample(t, v, time) - exact).abs() < tolerance, "{what}");
        }
    }
}

#[test]
fn a_gate_edge_late_in_a_long_run_switches_where_time_resolves_it() {
    // A 10 ns gate edge at t = 100 s: the time axis there resolves 14 fs,
    // in which the edge moves 7 µV, more than the switch's 1 µV resolution.
    // Locating the crossing more closely asked for retries that rounded to
    // the very step they shortened, and the run never finished; the switch
    // now changes state where such a step ends. Measured: within 2.1e-6 of
    // the closed form with the second-order methods. The 1 ns edge crosses
    // 2.5 V exactly at the restart step's midpoint, an accepted point, so
    // the switch turns on one minimum step (0.1 ps, 1e-4 τ) later: 6.2e-5
    // off at 0.5 τ, the time resolution of a 100 s run.
    let methods = [
        IntegrationMethod::Trapezoidal,
        IntegrationMethod::Gear2,
        IntegrationMethod::ModifiedTrapezoidal,
    ];
    let cases = [(10e-9, 1e-5), (1e-9, 1e-4)];
    for ((rise, tolerance), method) in cases.into_iter().flat_map(|c| methods.map(|m| (c, m))) {
        let circuit = gated_rc(100.0, rise, 1.0);
        let control = std::sync::Arc::new(SimControl::new());
        let shared = std::sync::Arc::clone(&control);
        let handle = std::thread::spawn(move || {
            let options = SimOptions {
                method,
                ..SimOptions::default()
            };
            let spec = Analysis::Transient(Transient {
                stop: 100.00001,
                step: 1.0,
                start_save: 0.0,
                max_step: None,
                initial_state: InitialState::OperatingPoint,
            });
            simulate(&circuit, &spec, &options, &shared)
        });
        let started = std::time::Instant::now();
        while !handle.is_finished() && started.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        control.cancel();
        let what = format!("{rise} s edge, {method:?}");
        let r = handle
            .join()
            .unwrap()
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        let t = &r.axis.values;
        let v = r.signal("V(o)").unwrap().real().unwrap();
        let on = 100.0 + rise / 2.0;
        for k in [0.5_f64, 1.0, 2.0, 5.0, 20.0] {
            let exact = -GATED_ON * (-k).exp_m1();
            let value = sample(t, v, on + k * GATED_TAU);
            assert!(
                (value - exact).abs() < tolerance,
                "{what}: {value} at {k} τ, exact {exact}"
            );
        }
    }
}

#[test]
fn a_gate_edge_shorter_than_two_minimum_steps_switches_at_its_end() {
    // Gate edges of 1.9, 1.5 and 1.2 minimum steps (fs on a 1 s run): from
    // the corner where such an edge starts, event landing stretches every
    // step shorter than the edge back to the corner where it ends, so the
    // located retry repeated the very step it shortened. With 1.9 fs the run
    // never finished; with 1.5 and 1.2 fs the retry rounded below the
    // minimum step and the run stopped with "Timestep too small" at the
    // edge. A step no shorter step exists for is no longer located: the
    // switch changes state where it ends, less than two minimum steps after
    // the crossing. The restart step behind the edge (0.2 ms after 0.5 s)
    // also used to pass over the 1 ns decay that follows, leaving V(o)
    // ramping linearly between its samples (0.999 V off); see
    // `transient_bounds.rs`. Measured: within 9.3e-7 of the closed form with
    // the second-order methods.
    let methods = [
        (IntegrationMethod::Trapezoidal, 1e-6, 2e-6),
        (IntegrationMethod::Gear2, 1e-6, 2e-6),
        (IntegrationMethod::ModifiedTrapezoidal, 1e-6, 2e-6),
        (IntegrationMethod::BackwardEuler, 1e-4, 3e-4),
    ];
    for (delay, minimum_steps) in [(1e-3, 1.9), (0.5, 1.5), (0.25, 1.2)] {
        let rise = minimum_steps * 1e-15;
        for (method, reltol, tolerance) in methods {
            let circuit = gated_rc(delay, rise, 0.25);
            let control = std::sync::Arc::new(SimControl::new());
            let shared = std::sync::Arc::clone(&control);
            let handle = std::thread::spawn(move || {
                let options = SimOptions {
                    method,
                    reltol,
                    ..SimOptions::default()
                };
                let spec = Analysis::Transient(Transient {
                    stop: 1.0,
                    step: 0.1,
                    start_save: 0.0,
                    max_step: None,
                    initial_state: InitialState::OperatingPoint,
                });
                simulate(&circuit, &spec, &options, &shared)
            });
            let started = std::time::Instant::now();
            while !handle.is_finished() && started.elapsed().as_secs() < 60 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            control.cancel();
            let what = format!("{minimum_steps} minimum steps at {delay} s, {method:?}");
            let r = handle
                .join()
                .unwrap()
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            let t = &r.axis.values;
            let v = r.signal("V(o)").unwrap().real().unwrap();
            for k in [0.5_f64, 1.0, 2.0, 5.0, 20.0] {
                let exact = -GATED_ON * (-k).exp_m1();
                let value = sample(t, v, delay + rise / 2.0 + k * GATED_TAU);
                assert!(
                    (value - exact).abs() < tolerance,
                    "{what}: {value} at {k} τ, exact {exact}"
                );
            }
        }
    }
}

#[test]
fn a_latch_without_hysteresis_that_drives_its_own_control_chatters_at_the_threshold() {
    // Ron discharges C1 the moment V(c) passes vt = 2.5 V and Roff lets it
    // recharge the moment it falls below: no motion is well defined, and the
    // switch chatters within its 1 µV resolution of the threshold. This used
    // to stop with a timestep too small at the first arrival (0.69 ms).
    let mut circuit = oscillator();
    for element in &mut circuit.elements {
        if let Device::VoltageSwitch { model } = &mut element.device {
            model.vh = 0.0;
        }
    }
    let spec = Analysis::Transient(Transient {
        stop: 0.72e-3,
        step: 1e-6,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::ZeroState,
    });
    let r = simulate(&circuit, &spec, &SimOptions::default(), &SimControl::new())
        .expect("simulation failed");
    let t = &r.axis.values;
    let v = r.signal("V(c)").unwrap().real().unwrap();
    // RC·ln(2) with Roff in parallel, as in the oscillator's first rise.
    let arrival = 0.693_150_249e-3;
    let late = t.iter().zip(v).filter(|&(&time, _)| time > arrival + 1e-6);
    let (low, high) = late.fold((f64::MAX, f64::MIN), |(lo, hi), (_, &x)| {
        (lo.min(x), hi.max(x))
    });
    assert!(
        (low - 2.5).abs() < 2e-6 && (high - 2.5).abs() < 2e-6,
        "V(c) between {low} and {high}"
    );
}
