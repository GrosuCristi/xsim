//! Full-simulation tests of the current-controlled switch (SPICE `W` with an
//! LTspice `CSW` model, Q87, D17): the sign of the control current, on/off
//! resistances, hysteresis in DC sweeps (exactly at the thresholds, and a
//! band containing 0 A), the smooth mode and its small-signal model, a
//! current ramp and a relaxation oscillator in transients, and validation of
//! the control. The LTspice 24.1.9 references are the decks
//! `tests/reference/devices_current_switch_*.cir`.

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

fn real<'a>(r: &'a SimulationResult, name: &str) -> &'a [f64] {
    r.signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .real()
        .unwrap_or_else(|| panic!("{name} is not real"))
}

fn model(ron: f64, roff: f64, it: f64, ih: f64) -> CurrentSwitchModel {
    CurrentSwitchModel {
        name: "CSW1".into(),
        ron,
        roff,
        it,
        ih,
    }
}

fn switch(control: &str, model: CurrentSwitchModel) -> Device {
    Device::CurrentSwitch {
        control: control.into(),
        model,
    }
}

fn dc(volts: f64) -> Device {
    Device::VoltageSource(Source::dc(volts))
}

fn dc_sweep(source: &str, start: f64, stop: f64, step: f64) -> Analysis {
    Analysis::DcSweep(DcSweep {
        source: source.into(),
        start,
        stop,
        step,
    })
}

// ------------------------------------------------ control-current sign

#[test]
fn the_control_is_the_source_current_from_its_positive_terminal() {
    // `tests/reference/devices_current_switch_sign.cir`. LTspice 24.1.9:
    // with R1 drawing 1 mA out of V1's positive terminal, I(V1) = -1 mA and
    // both W1 (It = 0.5 mA) and W2 (It = -0.5 mA) are off (I = 1e-6 A);
    // with I1 pushing 1 mA into it, I(V1) = 1 mA, W1 and W2 are on (1 A)
    // and W3 (It = 2 mA) is off.
    for (pushed, expected) in [(false, [1e-6, 1e-6, 1e-6]), (true, [1.0, 1.0, 1e-6])] {
        let mut c = Circuit::new();
        let a = c.node("a");
        // The switches come before their control source: the branch is
        // resolved after every element is set up.
        for (k, it) in [5e-4, -5e-4, 2e-3].into_iter().enumerate() {
            let out = c.node(&format!("o{k}"));
            c.add(Element::new(format!("VO{k}"), [out, GROUND], dc(1.0)));
            c.add(Element::new(
                format!("W{}", k + 1),
                [out, GROUND],
                switch("V1", model(1.0, 1e6, it, 0.0)),
            ));
        }
        if pushed {
            c.add(Element::new(
                "I1",
                [GROUND, a],
                Device::CurrentSource(Source::dc(1e-3)),
            ));
            c.add(Element::new("V1", [a, GROUND], dc(0.0)));
        } else {
            c.add(Element::new("V1", [a, GROUND], dc(1.0)));
            c.add(Element::new("R1", [a, GROUND], Device::resistor(1e3)));
        }
        let r = run(&c, &Analysis::OperatingPoint);
        let control = real(&r, "I(V1)")[0];
        assert_close(control, if pushed { 1e-3 } else { -1e-3 }, 1e-12, "I(V1)");
        for (k, expected) in expected.into_iter().enumerate() {
            let name = format!("I(W{})", k + 1);
            assert_close(real(&r, &name)[0], expected, 1e-12, &name);
        }
    }
}

// ----------------------------------------- hysteresis in DC sweeps

/// `tests/reference/devices_current_switch_thresholds.cir`: `V1 (5 V) → R1
/// (1 kΩ) → o`, switch from `o` to ground controlled by the current that
/// `I1` pushes through the 0 V source `VS`. With `it = 2⁻¹⁰` A and `ih =
/// 2⁻¹² A` the thresholds `it ± ih` and the sweep points `k·2⁻¹⁴` A are
/// exact binary numbers, so the sweeps land exactly on the thresholds.
fn divider(model: CurrentSwitchModel) -> Circuit {
    let mut c = Circuit::new();
    let (ctl, a, o) = (c.node("c"), c.node("a"), c.node("o"));
    c.add(Element::new(
        "I1",
        [GROUND, ctl],
        Device::CurrentSource(Source::dc(0.0)),
    ));
    c.add(Element::new("VS", [ctl, GROUND], dc(0.0)));
    c.add(Element::new("V1", [a, GROUND], dc(5.0)));
    c.add(Element::new("R1", [a, o], Device::resistor(1e3)));
    c.add(Element::new("W1", [o, GROUND], switch("VS", model)));
    c
}

/// V(o) of [`divider`] with the switch off and on.
const OFF: f64 = 5.0 * 1e6 / (1e6 + 1e3);
const ON: f64 = 5.0 * 10.0 / (10.0 + 1e3);

#[test]
fn dc_sweeps_keep_the_state_exactly_at_the_thresholds() {
    // LTspice 24.1.9, `.dc I1 0 0.001953125 6.103515625e-5` and back:
    // V(o) = 4.995004995005 (off) or 0.0495049504950495 (on). Rising, the
    // switch is still off at exactly it + ih = 0.001220703125 A (point 20)
    // and on from point 21; falling, still on at exactly it − ih =
    // 0.000732421875 A (point 12) and off from point 11.
    let (it, ih, step) = (2f64.powi(-10), 2f64.powi(-12), 2f64.powi(-14));
    let circuit = divider(model(10.0, 1e6, it, ih));
    for (start, stop, step, on) in [
        (
            0.0,
            32.0 * step,
            step,
            &(|k: usize| k >= 21) as &dyn Fn(usize) -> bool,
        ),
        (32.0 * step, 0.0, -step, &|k: usize| k >= 12),
    ] {
        let r = run(&circuit, &dc_sweep("I1", start, stop, step));
        let vo = real(&r, "V(o)");
        let control = real(&r, "I(VS)");
        assert_eq!(vo.len(), 33);
        let mut at_threshold = 0;
        for (j, &v) in vo.iter().enumerate() {
            // Sweep point `j` carries k·2⁻¹⁴ A, through VS exactly.
            let k = if step > 0.0 { j } else { 32 - j };
            assert_eq!(control[j], k as f64 * step.abs(), "I(VS) at point {j}");
            at_threshold += usize::from(k == 20 || k == 12);
            let expected = if on(k) { ON } else { OFF };
            assert_close(v, expected, 1e-12, &format!("V(o) at {k}·2⁻¹⁴ A"));
        }
        assert_eq!(at_threshold, 2);
    }
}

#[test]
fn zero_hysteresis_switches_at_the_threshold_both_ways() {
    let it = 2f64.powi(-10);
    let step = 2f64.powi(-14);
    let circuit = divider(model(10.0, 1e6, it, 0.0));
    let up = run(&circuit, &dc_sweep("I1", 0.0, 32.0 * step, step));
    let down = run(&circuit, &dc_sweep("I1", 32.0 * step, 0.0, -step));
    // Point 16 is exactly it: it keeps the state it arrives with.
    for (j, &v) in real(&up, "V(o)").iter().enumerate() {
        assert_close(v, if j > 16 { ON } else { OFF }, 1e-12, "rising");
    }
    for (j, &v) in real(&down, "V(o)").iter().enumerate() {
        assert_close(v, if 32 - j >= 16 { ON } else { OFF }, 1e-12, "falling");
    }
}

#[test]
fn a_latch_whose_band_contains_zero_amperes_starts_off() {
    // LTspice 24.1.9, the divider with CSW(Ron=10 Roff=1e6 It=-1m Ih=0.5m)
    // and I1 = -1.2 mA: the operating point of a `.tran` gives V(o) =
    // 4.995004995005 (off). `.dc I1 -1.25m 0.25m 0.25m` is off up to and
    // including -0.5 mA and on above; `.dc I1 0.25m -1.75m -0.25m` is on
    // down to and including -1.5 mA and off below. The starting guess of a
    // fresh solve, 0 A and so above it + ih, must not turn it on.
    let mut circuit = divider(model(10.0, 1e6, -1e-3, 0.5e-3));
    for element in &mut circuit.elements {
        if element.name == "I1" {
            element.device = Device::CurrentSource(Source::dc(-1.2e-3));
        }
    }
    let op = run(&circuit, &Analysis::OperatingPoint);
    assert_close(real(&op, "V(o)")[0], OFF, 1e-12, ".op");
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
    for &v in real(&tran, "V(o)") {
        assert_close(v, OFF, 1e-12, ".tran from the operating point");
    }
    let rising = |i: f64| if i > -0.5e-3 { ON } else { OFF };
    let falling = |i: f64| if i < -1.5e-3 { OFF } else { ON };
    for (start, stop, step, points, expected) in [
        (
            -1.25e-3,
            0.25e-3,
            0.25e-3,
            7,
            &rising as &dyn Fn(f64) -> f64,
        ),
        (0.25e-3, -1.75e-3, -0.25e-3, 9, &falling),
    ] {
        let r = run(&circuit, &dc_sweep("I1", start, stop, step));
        let vo = real(&r, "V(o)");
        assert_eq!(vo.len(), points);
        for (&i, &v) in r.axis.values.iter().zip(vo) {
            assert_close(v, expected(i), 1e-12, &format!("V(o) at I1 = {i}"));
        }
    }
}

#[test]
fn switch_conducts_through_ron_or_roff() {
    for (control, resistance) in [(0.0, 1e6), (2e-3, 10.0)] {
        let mut circuit = divider(model(10.0, 1e6, 1e-3, 0.0));
        circuit.elements[0].device = Device::CurrentSource(Source::dc(control));
        let r = run(&circuit, &Analysis::OperatingPoint);
        let expected = 5.0 / (1e3 + resistance);
        assert_close(real(&r, "I(W1)")[0], expected, 1e-12, "I(W1)");
        assert_close(real(&r, "I(R1)")[0], expected, 1e-12, "I(R1)");
    }
}

// --------------------------------------------------------- smooth mode

#[test]
fn smooth_mode_follows_ltspice_along_a_sweep() {
    // LTspice 24.1.9: I1 → VS (0 V), W1 from b (V2 = 1 V) to ground with
    // CSW(Ron=10 Roff=1e7 It=1m Ih=-0.5m), `.dc I1 0 2m 0.125m`.
    let ltspice = [
        1e-7,
        1e-7,
        1e-7,
        1e-7,
        1e-7,
        1.810_558_243_027_12e-7,
        8.659_643_233_600_63e-7,
        7.914_755_439_411_16e-6,
        9.999_999_999_999_99e-5,
        1.263_462_917_654_47e-3,
        1.154_781_984_689_46e-2,
        5.523_158_417_307_09e-2,
        0.1,
        0.1,
        0.1,
        0.1,
        0.1,
    ];
    let mut c = Circuit::new();
    let (ctl, b) = (c.node("c"), c.node("b"));
    c.add(Element::new(
        "I1",
        [GROUND, ctl],
        Device::CurrentSource(Source::dc(0.0)),
    ));
    c.add(Element::new("VS", [ctl, GROUND], dc(0.0)));
    c.add(Element::new("V2", [b, GROUND], dc(1.0)));
    c.add(Element::new(
        "W1",
        [b, GROUND],
        switch("VS", model(10.0, 1e7, 1e-3, -0.5e-3)),
    ));
    let r = run(&c, &dc_sweep("I1", 0.0, 2e-3, 0.125e-3));
    let current = real(&r, "I(W1)");
    assert_eq!(current.len(), ltspice.len());
    for (k, (&i, &expected)) in current.iter().zip(&ltspice).enumerate() {
        assert_close(i, expected, 1e-9, &format!("I(W1) at {k}·0.125 mA"));
    }
    // The midpoint is the geometric mean of the two conductances.
    assert_close(current[8], 1.0 / (10.0f64 * 1e7).sqrt(), 1e-12, "midpoint");
}

/// `tests/reference/devices_current_switch_smooth.cir`: `V3 (3 V, AC 1) →
/// R3 (1 kΩ) → o3`, a smooth switch from `o3` to ground controlled by the
/// current through `R5 (1 kΩ)` and `VS3` from `o3` to ground (so by its own
/// output voltage, through a branch current), and `V4 (3 V) → R4 (1 kΩ) →
/// o4`, a smooth switch from `o4` to ground controlled by `I4 (AC 1)`
/// pushed through `VC4`.
fn smooth_pair(v3: f64, i4: f64) -> Circuit {
    let mut c = Circuit::new();
    let (a3, o3, s3, a4, o4, c4) = (
        c.node("a3"),
        c.node("o3"),
        c.node("s3"),
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
        "W3",
        [o3, GROUND],
        switch("VS3", model(1e3, 10e3, 1e-3, -1e-3)),
    ));
    c.add(Element::new("R5", [o3, s3], Device::resistor(1e3)));
    c.add(Element::new("VS3", [s3, GROUND], dc(0.0)));
    c.add(Element::new("V4", [a4, GROUND], dc(3.0)));
    c.add(Element::new("R4", [a4, o4], Device::resistor(1e3)));
    c.add(Element::new(
        "W4",
        [o4, GROUND],
        switch("VC4", model(100.0, 100e3, 1e-3, -0.5e-3)),
    ));
    c.add(Element::new(
        "I4",
        [GROUND, c4],
        Device::CurrentSource(Source::ac(i4, 1.0, 0.0)),
    ));
    c.add(Element::new("VC4", [c4, GROUND], dc(0.0)));
    c
}

#[test]
fn smooth_mode_small_signal_is_the_slope_of_the_dc_curve() {
    // LTspice 24.1.9 batch runs of the reference deck with reltol=1e-12
    // abstol=1e-18 vntol=1e-15: V(o3) = 1.21950958143819 / 1.21980543720808
    // / 1.22010122571417 V at V3 = 2.999 / 3 / 3.001 V, so dV(o3)/dV3 =
    // 0.29582213799; V(o4) = 0.928923182551501 / 0.923345515526751 /
    // 0.917797058790221 V at I4 = 1.199 / 1.2 / 1.201 mA, so dV(o4)/dI4 =
    // -5563.06188064 V/A. LTspice's own `.ac` leaves out the dependence on
    // the control current (V(o4) is 0 there), so it is not the reference.
    let options = SimOptions {
        reltol: 1e-12,
        abstol: 1e-18,
        vntol: 1e-15,
        ..SimOptions::default()
    };
    let voltage = |c: &Circuit, name: &str| {
        let r = simulate(c, &Analysis::OperatingPoint, &options, &SimControl::new()).unwrap();
        real(&r, name)[0]
    };
    let nominal = smooth_pair(3.0, 1.2e-3);
    assert_close(
        voltage(&nominal, "V(o3)"),
        1.219_805_437_208_08,
        1e-9,
        "V(o3)",
    );
    assert_close(
        voltage(&nominal, "V(o4)"),
        0.923_345_515_526_751,
        1e-9,
        "V(o4)",
    );
    let slope3 = (voltage(&smooth_pair(3.001, 1.2e-3), "V(o3)")
        - voltage(&smooth_pair(2.999, 1.2e-3), "V(o3)"))
        / 0.002;
    let slope4 = (voltage(&smooth_pair(3.0, 1.201e-3), "V(o4)")
        - voltage(&smooth_pair(3.0, 1.199e-3), "V(o4)"))
        / 2e-6;
    let ac = simulate(
        &nominal,
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
    // I4 has no effect on o3 and V3 none on o4, so both carry AC 1 at once.
    for (name, slope, ltspice) in [
        ("V(o3)", slope3, 0.295_822_137_99),
        ("V(o4)", slope4, -5_563.061_880_64),
    ] {
        let SignalData::Complex { re, im } = &ac.signal(name).unwrap().data else {
            panic!("{name} is not complex");
        };
        assert_eq!(im[0], 0.0, "{name}");
        // A central difference is exact to its O(h²) term.
        assert_close(re[0], slope, 1e-5, &format!("{name}: AC vs DC slope"));
        assert_close(re[0], ltspice, 1e-5, &format!("{name}: AC vs LTspice"));
        // LTspice prints 15 digits, which leaves its slopes good to ~1e-9.
        assert_close(slope, ltspice, 1e-6, &format!("{name}: DC vs LTspice"));
    }
}

#[test]
fn a_latched_switch_is_a_resistor_in_ac() {
    // `tests/reference/devices_current_switch_ac.cir`: on (control 2 mA >
    // it + ih) the switch is Ron, off (control 0) Roff; a latch's
    // conductance does not depend on the control at the operating point, so
    // the AC control current has no effect. LTspice 24.1.9: I(W) = -20 dB
    // and -120 dB at 0°, 0.1 A and 1 µA.
    for (control, resistance) in [(2e-3, 10.0), (0.0, 1e6)] {
        let mut c = Circuit::new();
        let (a, ctl) = (c.node("a"), c.node("c"));
        c.add(Element::new(
            "V1",
            [a, GROUND],
            Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
        ));
        c.add(Element::new(
            "I1",
            [GROUND, ctl],
            Device::CurrentSource(Source::ac(control, 1.0, 0.0)),
        ));
        c.add(Element::new("VS", [ctl, GROUND], dc(0.0)));
        c.add(Element::new(
            "W1",
            [a, GROUND],
            switch("VS", model(10.0, 1e6, 1e-3, 0.1e-3)),
        ));
        let r = run(
            &c,
            &Analysis::Ac(AcSweep {
                scale: SweepScale::Linear,
                points: 1,
                start: 1e3,
                stop: 1e3,
            }),
        );
        let SignalData::Complex { re, im } = &r.signal("I(W1)").unwrap().data else {
            panic!("AC result must be complex");
        };
        assert_close(re[0], 1.0 / resistance, 1e-12, "AC conductance");
        assert_eq!(im[0], 0.0);
    }
}

// ------------------------------------------------- transient: current ramp

/// `values` at `time`, linearly interpolated.
fn sample(times: &[f64], values: &[f64], time: f64) -> f64 {
    let j = times
        .partition_point(|&t| t < time)
        .clamp(1, times.len() - 1);
    let (t0, t1) = (times[j - 1], times[j]);
    values[j - 1] + (time - t0) * (values[j] - values[j - 1]) / (t1 - t0)
}

/// Crossings of `level`, linearly interpolated: rising if `rising`, else
/// falling.
fn crossings(t: &[f64], v: &[f64], level: f64, rising: bool) -> Vec<f64> {
    (1..v.len())
        .filter(|&j| {
            if rising {
                v[j - 1] < level && v[j] >= level
            } else {
                v[j - 1] > level && v[j] <= level
            }
        })
        .map(|j| t[j - 1] + (level - v[j - 1]) * (t[j] - t[j - 1]) / (v[j] - v[j - 1]))
        .collect()
}

#[test]
fn a_triangle_control_current_switches_at_the_hysteresis_thresholds() {
    // `tests/reference/devices_current_switch_ramp.cir`: I1 pushes a
    // 0 → 2 mA → 0 triangle through VS over 2 ms, so the switch (it = 1 mA,
    // ih = 0.2 mA) turns on at 0.6 ms (1.2 mA) and off at 1.6 ms (0.8 mA).
    // It loads `V1 (5 V) → R1 (1 kΩ) → o` with C1 = 10 nF: V(o) falls
    // through 2.5 V τ_on·ln(4.9455/2.4505) = 69.52 ns after the switch
    // turns on and rises through it τ_off·ln(4.9455/2.4950) = 6.835 µs
    // after it turns off. LTspice 24.1.9 batch run of the deck (0.1 ns
    // maximum step): falling crossing 0.600069481481252 ms, rising crossing
    // 1.60683501550817 ms, V(o) = 4.99500499500501 V at 0.3 ms,
    // 0.0495049504950846 V at 1 ms and 4.99500499481299 V at 1.9 ms,
    // I(W1) = 4.95049504950846 mA at 1 ms. LTspice places a switching
    // instant to about half its maximum step: its crossings are 0.04 ns
    // and 0.02 ns before the closed form here, 0.6 ns and 0.4 ns with a
    // 1 ns maximum step.
    let mut c = Circuit::new();
    let (ctl, a, o) = (c.node("c"), c.node("a"), c.node("o"));
    let triangle = Waveform::Pwl {
        points: vec![(0.0, 0.0), (1e-3, 2e-3), (2e-3, 0.0)],
        repeat: false,
    };
    c.add(Element::new(
        "I1",
        [GROUND, ctl],
        Device::CurrentSource(Source::with_waveform(triangle)),
    ));
    c.add(Element::new("VS", [ctl, GROUND], dc(0.0)));
    c.add(Element::new("V1", [a, GROUND], dc(5.0)));
    c.add(Element::new("R1", [a, o], Device::resistor(1e3)));
    c.add(Element::new("C1", [o, GROUND], Device::capacitor(10e-9)));
    c.add(Element::new(
        "W1",
        [o, GROUND],
        switch("VS", model(10.0, 1e6, 1e-3, 0.2e-3)),
    ));
    let (r_on, r_off) = (1e3 * 10.0 / 1010.0, 1e3 * 1e6 / (1e3 + 1e6));
    let fall = 0.6e-3 + r_on * 10e-9 * ((OFF - ON) / (2.5 - ON)).ln();
    let rise = 1.6e-3 + r_off * 10e-9 * ((OFF - ON) / (OFF - 2.5)).ln();
    assert_close(fall, 0.600_069_523_565_9e-3, 1e-12, "closed form");
    // The switch changes state at its located threshold crossing, within
    // its 1.2 nA resolution of the 1.2 mA and 0.8 mA thresholds (0.6 ns of
    // the 2 A/s ramp), whatever step the truncation-error test would allow:
    // measured 5e-7 (falling) and 1.9e-7 (rising) from the closed form at
    // both tolerances. A switch that changed state wherever a step ended
    // was 2.7e-6 late at the default reltol, where an 8.8 ns step crossed
    // the turn-off threshold (C1 sits at a steady state when it opens).
    for (reltol, rising_tolerance) in [(1e-6, 1e-6), (1e-7, 1e-6)] {
        let options = SimOptions {
            reltol,
            ..SimOptions::default()
        };
        let r = simulate(
            &c,
            &Analysis::Transient(Transient {
                stop: 2e-3,
                step: 1e-6,
                start_save: 0.0,
                max_step: Some(1e-6),
                initial_state: InitialState::OperatingPoint,
            }),
            &options,
            &SimControl::new(),
        )
        .expect("simulation failed");
        let t = &r.axis.values;
        let v = real(&r, "V(o)");
        let i = real(&r, "I(W1)");
        let falls = crossings(t, v, 2.5, false);
        let rises = crossings(t, v, 2.5, true);
        assert_eq!((falls.len(), rises.len()), (1, 1), "{falls:?} {rises:?}");
        // Between switchings V(o) relaxes exponentially towards ON or OFF
        // with a known time constant, so the crossing follows exactly from
        // the first sample past it; linear interpolation between the sparse
        // samples of the slow rise (τ_off ≈ 10 µs) would misplace it by up
        // to h²/8τ, 4 ns.
        let exact_crossing = |crossing: f64, asymptote: f64, tau: f64| {
            let j = t.partition_point(|&time| time < crossing);
            t[j] - tau * ((asymptote - 2.5) / (asymptote - v[j])).ln()
        };
        for (what, actual, closed, ltspice, tolerance) in [
            (
                "falling",
                exact_crossing(falls[0], ON, r_on * 10e-9),
                fall,
                0.600_069_481_481_252e-3,
                1e-6,
            ),
            (
                "rising",
                exact_crossing(rises[0], OFF, r_off * 10e-9),
                rise,
                1.606_835_015_508_17e-3,
                rising_tolerance,
            ),
        ] {
            let what = format!("reltol {reltol}: {what} crossing");
            assert_close(actual, closed, tolerance, &format!("{what} vs closed form"));
            assert_close(actual, ltspice, tolerance, &format!("{what} vs LTspice"));
        }
        for (time, expected) in [
            (0.3e-3, 4.995_004_995_005_01),
            (1e-3, 0.049_504_950_495_084_6),
            (1.9e-3, 4.995_004_994_812_99),
        ] {
            let what = format!("reltol {reltol}: V(o) at {time} s vs LTspice");
            assert_close(sample(t, v, time), expected, 1e-6, &what);
        }
        let what = format!("reltol {reltol}: I(W1) at 1 ms vs LTspice");
        assert_close(sample(t, i, 1e-3), 4.950_495_049_508_46e-3, 1e-6, &what);
    }
}

// ---------------------------------------- relaxation oscillator

/// `tests/reference/devices_current_switch_oscillator.cir`: C1 charges
/// through R1; the switch senses the current V(c)/100 kΩ through R2 and VS
/// and turns on above 30 µA (3 V), discharging C1 to 20 µA (2 V).
fn oscillator() -> Circuit {
    let mut c = Circuit::new();
    let (vcc, n, s) = (c.node("vcc"), c.node("c"), c.node("s"));
    c.add(Element::new("VCC", [vcc, GROUND], dc(5.0)));
    c.add(Element::new("R1", [vcc, n], Device::resistor(10e3)));
    c.add(Element::new("C1", [n, GROUND], Device::capacitor(100e-9)));
    c.add(Element::new("R2", [n, s], Device::resistor(100e3)));
    c.add(Element::new("VS", [s, GROUND], dc(0.0)));
    c.add(Element::new(
        "W1",
        [n, GROUND],
        switch("VS", model(10.0, 1e9, 25e-6, 5e-6)),
    ));
    c
}

#[test]
fn a_current_sensing_switch_discharging_its_capacitor_oscillates() {
    // Closed form: charging 2 → 3 V through R1 against R2 ‖ Roff, then
    // discharging 3 → 2 V through Ron ‖ R1 ‖ R2.
    let par = |a: f64, b: f64| a * b / (a + b);
    let (r1, r2, c, ron, roff) = (10e3, 100e3, 100e-9, 10.0, 1e9);
    let phase = |load: f64, from: f64, to: f64| {
        let vinf = 5.0 * load / (r1 + load);
        par(r1, load) * c * ((vinf - from) / (vinf - to)).ln()
    };
    let first = phase(par(r2, roff), 0.0, 2.5);
    let period = phase(par(r2, roff), 2.0, 3.0) + phase(par(r2, ron), 3.0, 2.0);
    assert_close(period, 0.454_039_611_108e-3, 1e-11, "closed form");
    // LTspice 24.1.9 batch run of the reference deck (1 ns maximum step):
    // first rise through 2.5 V at 0.725919589318672 ms, RISE=2 at
    // 1.18004538589822 ms and RISE=10 at 4.81305286475512 ms, a period of
    // 0.4541259 ms, 1.9e-4 above the closed form (with a 0.1 µs maximum
    // step its periods scatter by 2 %).
    //
    // Measured (default options): every period within 3.4e-6 of the closed
    // form (6.5e-5 with backward Euler at reltol 1e-4), the switch turning
    // on and off within 1.6 µV of 3 V and 2 V (its resolution is 3e-11 A,
    // 3 µV of V(c)). The mean period is 1.85e-4 below LTspice's, which is
    // LTspice's own distance from the closed form.
    let ltspice = (4.813_052_864_755_12e-3 - 1.180_045_385_898_22e-3) / 8.0;
    assert_close(first, 0.725_919_589_318_672e-3, 1e-9, "first rise: LTspice");
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
        let v = real(&result, "V(c)");
        let rises = crossings(t, v, 2.5, true);
        assert!(
            rises.len() >= 10,
            "{method:?}: {} rising edges",
            rises.len()
        );
        let what = format!("{method:?}: first rise");
        assert_close(rises[0], first, tolerance, &what);
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
            (low - 2.0).abs() < 5e-6 && (high - 3.0).abs() < 5e-6,
            "{method:?}: V(c) between {low} and {high}"
        );
    }
}

#[test]
fn the_oscillator_has_no_operating_point() {
    // No DC state satisfies the latch. LTspice's direct Newton iteration,
    // gmin stepping and source stepping fail on it as well (its
    // pseudo-transient fallback then starts a `.tran` from a point between
    // the thresholds); the reference deck uses `uic`.
    let error = simulate(
        &oscillator(),
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap_err();
    assert!(matches!(error, SimError::NoConvergence { .. }), "{error}");
}

// ------------------------------------------------------------ validation

fn validation_error(control: &str, extra: Option<Element>) -> String {
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    c.add(Element::new("V1", [a, GROUND], dc(1.0)));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new(
        "W1",
        [b, GROUND],
        switch(control, CurrentSwitchModel::default()),
    ));
    if let Some(element) = extra {
        c.add(element);
    }
    let validated = c.validate().err().map(|e| e.message);
    let simulated = simulate(
        &c,
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &SimControl::new(),
    );
    match (validated, simulated) {
        (Some(message), Err(SimError::InvalidCircuit(e))) => {
            assert_eq!(e.message, message);
            message
        }
        (None, Ok(_)) => String::new(),
        (v, s) => panic!("validate {v:?} but simulate {s:?}"),
    }
}

#[test]
fn the_control_must_name_an_independent_voltage_source() {
    assert_eq!(validation_error("V1", None), "");
    let current_source = Element::new(
        "I9",
        [GROUND, GROUND],
        Device::CurrentSource(Source::dc(0.0)),
    );
    for (control, extra, problem) in [
        ("V9", None, "its control V9 does not exist"),
        ("R1", None, "its control R1 is not one"),
        ("W1", None, "its control W1 is not one"),
        ("v1", None, "its control v1 does not exist"),
        ("I9", Some(current_source), "its control I9 is not one"),
        ("", None, "no control is given"),
    ] {
        // The whole message, so a stray run of spaces from a broken string
        // continuation shows up.
        assert_eq!(
            validation_error(control, extra),
            format!(
                "W1: a current-controlled switch senses the current through an independent \
                 voltage source, but {problem}"
            ),
            "{control:?}"
        );
    }
}

// ------------------------------- reversed sense source, self-sensing

/// `tests/reference/devices_current_switch_reversed.cir`: `IX` pushes its
/// current into `VS`'s `+` terminal (so `I(VS) = IX`) and on into `VS2`'s
/// `−` terminal (`I(VS2) = −IX`). `W1` (negative threshold) senses `VS`
/// and switches `V1 (5 V) → R1 (1 kΩ) → p → n → R2 (100 Ω)`; `W2` senses
/// `VS2` and switches `V3 (5 V) → R3 (1 kΩ) → q` to ground.
fn reversed_sense() -> Circuit {
    let mut c = Circuit::new();
    let (x, y, z) = (c.node("x"), c.node("y"), c.node("z"));
    let (a, p, n, b, q) = (
        c.node("a"),
        c.node("p"),
        c.node("n"),
        c.node("b"),
        c.node("q"),
    );
    let (it, ih) = (2f64.powi(-10), 2f64.powi(-12));
    c.add(Element::new(
        "IX",
        [GROUND, x],
        Device::CurrentSource(Source::dc(0.0)),
    ));
    c.add(Element::new("VS", [x, y], dc(0.2)));
    c.add(Element::new("VS2", [z, y], dc(0.0)));
    c.add(Element::new("RY", [z, GROUND], Device::resistor(100.0)));
    c.add(Element::new("V1", [a, GROUND], dc(5.0)));
    c.add(Element::new("R1", [a, p], Device::resistor(1e3)));
    c.add(Element::new(
        "W1",
        [p, n],
        switch("VS", model(10.0, 1e6, -it, ih)),
    ));
    c.add(Element::new("R2", [n, GROUND], Device::resistor(100.0)));
    c.add(Element::new("V3", [b, GROUND], dc(5.0)));
    c.add(Element::new("R3", [b, q], Device::resistor(1e3)));
    c.add(Element::new(
        "W2",
        [q, GROUND],
        switch("VS2", model(10.0, 1e6, it, ih)),
    ));
    c
}

#[test]
fn a_reversed_sense_source_and_negative_thresholds_follow_ltspice() {
    // LTspice 24.1.9 batch runs of the reference deck, both sweep
    // directions: W1 off V(p) = 4.99500549395665 / on 0.495495495495496 V,
    // W2 on V(q) = 0.0495049504950495 / off 4.995004995005 V. Rising, W1
    // turns on and W2 off after point 20 (IX = −0.000732421875 A, exactly
    // W1's it + ih and, through VS2, W2's it − ih); falling, W1 turns off
    // and W2 on after point 12 (IX = −0.001220703125 A).
    const P_OFF: f64 = 4.995_005_493_956_65;
    const P_ON: f64 = 0.495_495_495_495_496;
    const Q_ON: f64 = 0.049_504_950_495_049_5;
    const Q_OFF: f64 = 4.995_004_995_005;
    let step = 2f64.powi(-14);
    let (low, high) = (-32.0 * step, 16.0 * step);
    let circuit = reversed_sense();
    for (start, stop, step, w1_on) in [
        (
            low,
            high,
            step,
            &(|k: usize| k >= 21) as &dyn Fn(usize) -> bool,
        ),
        (high, low, -step, &|k: usize| k >= 12),
    ] {
        let r = run(&circuit, &dc_sweep("IX", start, stop, step));
        let (vp, vq) = (real(&r, "V(p)"), real(&r, "V(q)"));
        let (sense, reversed) = (real(&r, "I(VS)"), real(&r, "I(VS2)"));
        assert_eq!(vp.len(), 49);
        for j in 0..vp.len() {
            // Sweep point `j` carries IX = low + k·2⁻¹⁴ A.
            let k = if step > 0.0 { j } else { 48 - j };
            let ix = low + k as f64 * step.abs();
            assert_eq!(sense[j], ix, "I(VS) at point {k}");
            assert_eq!(reversed[j], -ix, "I(VS2) at point {k}");
            let on = w1_on(k);
            let what = format!("point {k}, sweep step {step:e}");
            assert_close(vp[j], if on { P_ON } else { P_OFF }, 1e-12, &what);
            assert_close(vq[j], if on { Q_OFF } else { Q_ON }, 1e-12, &what);
        }
        // Both switches conduct from P to N, like LTspice's I(W1).
        let i_w1 = real(&r, "I(W1)");
        let last = if step > 0.0 { i_w1.len() - 1 } else { 0 };
        assert_close(i_w1[last], 0.004_504_504_504_504_5, 1e-12, "I(W1) on");
    }
}

/// `tests/reference/devices_current_switch_self.cir`: switches in series
/// with their own control sources: the latch `W1` behind `R1 (1 kΩ)` from
/// `V1`, and the smooth `W3` behind `R3 (1 kΩ)` from `V3 (AC 1)`.
fn self_sensing(v3: f64) -> Circuit {
    let mut c = Circuit::new();
    let (a, b, s, e, f, g) = (
        c.node("a"),
        c.node("b"),
        c.node("s"),
        c.node("e"),
        c.node("f"),
        c.node("g"),
    );
    c.add(Element::new("V1", [a, GROUND], dc(0.0)));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new("VSW", [b, s], dc(0.0)));
    c.add(Element::new(
        "W1",
        [s, GROUND],
        switch("VSW", model(1.0, 10e3, 1e-3, 0.5e-3)),
    ));
    c.add(Element::new(
        "V3",
        [e, GROUND],
        Device::VoltageSource(Source::ac(v3, 1.0, 0.0)),
    ));
    c.add(Element::new("R3", [e, f], Device::resistor(1e3)));
    c.add(Element::new("VSW3", [f, g], dc(0.0)));
    c.add(Element::new(
        "W3",
        [g, GROUND],
        switch("VSW3", model(100.0, 10e3, 1e-3, -1e-3)),
    ));
    c
}

#[test]
fn a_switch_sensing_its_own_current_follows_ltspice() {
    // LTspice 24.1.9 batch runs of the reference deck (sweep points off the
    // exact thresholds, where the last bit of the computed current decides).
    let circuit = self_sensing(2.0);
    for (start, stop, step, expected) in [
        (
            0.0,
            20.0,
            0.4,
            [
                (16.4, 0.001_490_909_090_909_09),
                (16.8, 0.016_783_216_783_216_8),
            ],
        ),
        (
            20.0,
            0.0,
            -0.2,
            [
                (0.6, 0.000_599_400_599_400_599),
                (0.4, 3.636_363_636_364_03e-5),
            ],
        ),
    ] {
        let r = run(&circuit, &dc_sweep("V1", start, stop, step));
        for (volts, current) in expected {
            let j = r
                .axis
                .values
                .iter()
                .position(|&v| (v - volts).abs() < 1e-9)
                .expect("sweep point");
            assert_close(
                real(&r, "I(VSW)")[j],
                current,
                1e-12,
                &format!("I(VSW) at V1 = {volts} V"),
            );
        }
    }
    // The smooth W3: ".dc V3 1.999 2.001 0.001" gives V(f) = 1.79355781754769
    // / 1.79442348861482 / 1.79528908890498 V, so dV(f)/dV3 = 0.865635678645.
    // (LTspice's own `.ac` gives 0.89739: it leaves out the dependence on
    // the control current.) The small-signal model includes the switch's
    // own current through its control branch.
    let options = SimOptions {
        reltol: 1e-12,
        abstol: 1e-18,
        vntol: 1e-15,
        ..SimOptions::default()
    };
    let voltage = |v3: f64| {
        let r = simulate(
            &self_sensing(v3),
            &Analysis::OperatingPoint,
            &options,
            &SimControl::new(),
        )
        .expect("operating point");
        real(&r, "V(f)")[0]
    };
    assert_close(voltage(2.0), 1.794_423_488_614_82, 1e-9, "V(f)");
    let slope = (voltage(2.001) - voltage(1.999)) / 0.002;
    assert_close(slope, 0.865_635_678_645, 1e-6, "DC slope vs LTspice");
    let ac = simulate(
        &self_sensing(2.0),
        &Analysis::Ac(AcSweep {
            scale: SweepScale::Linear,
            points: 1,
            start: 1e3,
            stop: 1e3,
        }),
        &options,
        &SimControl::new(),
    )
    .expect("AC");
    let SignalData::Complex { re, im } = &ac.signal("V(f)").expect("V(f)").data else {
        panic!("V(f) is not complex");
    };
    assert_eq!(im[0], 0.0);
    assert_close(re[0], slope, 1e-5, "AC vs DC slope");
    assert_close(re[0], 0.865_635_678_645, 1e-5, "AC vs LTspice DC slope");
}
