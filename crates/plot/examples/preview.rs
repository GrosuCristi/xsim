//! Opens the waveform viewer on generated data, for manual checks of the
//! look and feel and of large-data responsiveness (Q140).
//!
//! ```text
//! cargo run -p xsim-plot --example preview              # 200 000-sample transient
//! cargo run -p xsim-plot --example preview -- --ac      # Bode plot
//! cargo run -p xsim-plot --example preview -- --op      # operating point table
//! cargo run -p xsim-plot --example preview -- --dark    # dark theme
//! cargo run -p xsim-plot --example preview -- --cursors # cursors and measurements
//! cargo run -p xsim-plot --example preview -- --large   # 3 000 000 samples
//! cargo run -p xsim-plot --example preview -- --scale   # 125 % UI scale
//! cargo run -p xsim-plot --example preview -- --panes   # V(in) in a second pane
//! ```

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;
use std::f64::consts::TAU;
use std::sync::Arc;
use xsim_plot::WaveformViewer;
use xsim_project::{PlotPane, PlotSetup, Trace};
use xsim_spice_engine::{
    AnalysisKind, Axis, Quantity, Signal, SignalData, SignalOrigin, SimulationResult,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let (result, traces) = if flag("--ac") {
        (low_pass(), vec!["V(out)", "V(in)"])
    } else if flag("--op") {
        (operating_point(), vec![])
    } else {
        let samples = if flag("--large") { 3_000_000 } else { 200_000 };
        (ringing(samples), vec!["V(out)", "V(in)", "I(L1)*10"])
    };
    let cursors = flag("--cursors");
    // Two panes, to try dragging a legend chip from one to the other (D14).
    let (setup, traces) = if flag("--panes") && !flag("--ac") && !flag("--op") {
        let pane = |expressions: &[&str]| PlotPane {
            traces: expressions.iter().map(|e| Trace::new(*e)).collect(),
            ..PlotPane::default()
        };
        let panes = vec![pane(&["V(out)", "I(L1)*10"]), pane(&["V(in)"])];
        (
            PlotSetup {
                panes,
                active_pane: 0,
            },
            Vec::new(),
        )
    } else {
        (PlotSetup::default(), traces)
    };
    let scale = if flag("--scale") { 1.25 } else { 1.0 };
    let mode = if flag("--dark") {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(mode, None, cx);
            // The kit's root sets the rem size from the theme font size (Q30).
            Theme::update(cx, |theme| {
                theme.font_size *= scale;
                theme.mono_font_size *= scale;
            });
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(760.)),
                    cx,
                ))),
                ..WindowOptions::default()
            };
            gpui_kit::open_window(options, cx, |_, cx| {
                cx.new(|cx| {
                    let mut viewer = WaveformViewer::new(cx);
                    viewer.set_result(Some(Arc::new(result)), setup, cx);
                    for expression in traces {
                        viewer.add_trace(expression, cx);
                    }
                    viewer.set_cursors_visible(cursors, cx);
                    viewer.set_measurements_visible(cursors, cx);
                    viewer
                })
            })
            .expect("open the preview window");
            cx.activate(true);
        });
}

fn signal(name: &str, quantity: Quantity, data: SignalData) -> Signal {
    let origin = match quantity {
        Quantity::Voltage => SignalOrigin::Node(1),
        Quantity::Current => SignalOrigin::TerminalCurrent {
            element: 0,
            terminal: 0,
        },
    };
    Signal {
        name: name.into(),
        quantity,
        origin,
        data,
    }
}

fn result(
    analysis: AnalysisKind,
    name: &str,
    unit: &str,
    values: Vec<f64>,
    signals: Vec<Signal>,
) -> SimulationResult {
    SimulationResult {
        analysis,
        axis: Axis {
            name: name.into(),
            unit: unit.into(),
            values,
        },
        signals,
    }
}

/// A 1 kHz square wave driving a ringing LC filter, `samples` long.
fn ringing(samples: usize) -> SimulationResult {
    let t: Vec<f64> = (0..samples)
        .map(|i| i as f64 * 10e-3 / samples as f64)
        .collect();
    let input: Vec<f64> = t
        .iter()
        .map(|t| if (t * 1e3).fract() < 0.5 { 1.0 } else { 0.0 })
        .collect();
    let out: Vec<f64> = t
        .iter()
        .map(|t| {
            let phase = (t * 1e3).fract() * 1e-3;
            let step = if (t * 1e3).fract() < 0.5 { 1.0 } else { -1.0 };
            let base = if step > 0.0 { 0.0 } else { 1.0 };
            let local = if step > 0.0 { phase } else { phase - 0.5e-3 };
            base + step * (1.0 - (-local / 8e-5).exp() * (TAU * 2e4 * local).cos())
        })
        .collect();
    let current: Vec<f64> = out
        .windows(2)
        .map(|w| (w[1] - w[0]) * 1e-1)
        .chain([0.0])
        .collect();
    result(
        AnalysisKind::Transient,
        "time",
        "s",
        t,
        vec![
            signal("V(in)", Quantity::Voltage, SignalData::Real(input)),
            signal("V(out)", Quantity::Voltage, SignalData::Real(out)),
            signal("I(L1)", Quantity::Current, SignalData::Real(current)),
        ],
    )
}

/// A second-order low-pass with a resonant peak at 10 kHz.
fn low_pass() -> SimulationResult {
    let f: Vec<f64> = (0..=500)
        .map(|i| 10f64.powf(f64::from(i) / 100.0))
        .collect();
    let (re, im): (Vec<f64>, Vec<f64>) = f
        .iter()
        .map(|f| {
            let x = f / 1e4;
            let (a, b) = (1.0 - x * x, x / 2.0);
            let d = a * a + b * b;
            (a / d, -b / d)
        })
        .unzip();
    let n = f.len();
    result(
        AnalysisKind::Ac,
        "frequency",
        "Hz",
        f,
        vec![
            signal(
                "V(in)",
                Quantity::Voltage,
                SignalData::Complex {
                    re: vec![1.0; n],
                    im: vec![0.0; n],
                },
            ),
            signal("V(out)", Quantity::Voltage, SignalData::Complex { re, im }),
        ],
    )
}

fn operating_point() -> SimulationResult {
    result(
        AnalysisKind::OperatingPoint,
        "point",
        "",
        vec![0.0],
        vec![
            signal("V(in)", Quantity::Voltage, SignalData::Real(vec![5.0])),
            signal("V(out)", Quantity::Voltage, SignalData::Real(vec![2.5])),
            signal("I(R1)", Quantity::Current, SignalData::Real(vec![2.5e-3])),
        ],
    )
}
