//! Helpers for the headless UI tests: a private data directory, scripted
//! prompts, windows on test projects and reference circuits.

use crate::prompts::scripted::ScriptedPrompts;
use crate::workspace::{Services, Workspace};
use gpui_kit::{
    AppContext, Bounds, Entity, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, Point, TestAppContext, VisualTestContext, WindowBounds,
    WindowOptions, px, size,
};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use xsim_project::appdata::AppData;
use xsim_project::{
    Component, Id, NetLabel, Orientation, Part, PartKind, Point as GridPoint, Project, Schematic,
    Wire,
};
use xsim_spice_engine::{InitialState, Source, Transient, Waveform};

/// A temporary data directory and scripted dialogs for one test.
pub struct TestEnv {
    dir: TempDir,
    pub prompts: Rc<ScriptedPrompts>,
}

impl TestEnv {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("temporary directory"),
            prompts: Rc::new(ScriptedPrompts::default()),
        }
    }

    /// The application data directory (never the real profile).
    pub fn data_dir(&self) -> PathBuf {
        self.dir.path().join("app-data")
    }

    pub fn app_data(&self) -> AppData {
        AppData::at(self.data_dir())
    }

    /// A file path inside the test directory.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Saves `project` as `name` and returns its path.
    pub fn write_project(&self, name: &str, project: &Project) -> PathBuf {
        let path = self.path(name);
        project.save(&path).expect("save the test project");
        path
    }

    fn services(&self) -> Services {
        Services {
            app_data: self.app_data(),
            prompts: self.prompts.clone(),
        }
    }
}

/// Initializes the kit, motion and the shortcuts as the application does;
/// call once per test.
pub fn init(cx: &mut TestAppContext) {
    // Simulations finish on real threads, which wake the test executor.
    cx.executor().allow_parking();
    cx.update(crate::setup);
}

/// Opens a window on `path` (or an empty project).
pub fn open(
    cx: &mut TestAppContext,
    env: &TestEnv,
    path: Option<&Path>,
) -> (Entity<Workspace>, VisualTestContext) {
    let services = env.services();
    let path = path.map(Path::to_path_buf);
    let (window, workspace) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::default(),
                    size(px(1280.), px(800.)),
                ))),
                ..WindowOptions::default()
            },
            cx,
            |window, cx| cx.new(|cx| Workspace::new(path, services, window, cx)),
        )
        .expect("open the test window")
    });
    let visual = VisualTestContext::from_window(window, cx);
    visual.run_until_parked();
    (workspace, visual)
}

/// Closes a test window.
pub fn close(cx: &mut VisualTestContext) {
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
}

/// Runs the executor until `done` holds; background simulations run on
/// real threads, so this also waits in real time (at most 60 s).
pub fn wait_until(
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for the UI");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Double-clicks the centre of the element tagged `selector`.
pub fn double_click(selector: &'static str, cx: &mut VisualTestContext) {
    let position = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not on screen"))
        .center();
    let modifiers = Modifiers::default();
    cx.simulate_mouse_move(position, None, modifiers);
    for click_count in 1..=2 {
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count,
        });
    }
    cx.run_until_parked();
}

/// Presses and releases `key`, as a keyboard does: a focused button
/// activates on the release.
pub fn press(key: &'static str, cx: &mut VisualTestContext) {
    let keystroke = Keystroke::parse(key).unwrap_or_else(|_| panic!("{key} is not a key"));
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(KeyUpEvent { keystroke });
    cx.run_until_parked();
}

/// Clicks the centre of the element tagged `selector`.
pub fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let point = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not on screen"))
        .center();
    cx.simulate_mouse_move(point, None, Modifiers::default());
    cx.simulate_click(point, Modifiers::default());
    cx.run_until_parked();
}

fn component(id: Id, name: &str, position: (i32, i32), rotation: u8, part: Part) -> Component {
    Component {
        id,
        name: name.into(),
        position: GridPoint::new(position.0, position.1),
        orientation: Orientation {
            rotation,
            ..Orientation::default()
        },
        part,
    }
}

fn wire(id: Id, points: &[(i32, i32)]) -> Wire {
    Wire {
        id,
        points: points.iter().map(|&(x, y)| GridPoint::new(x, y)).collect(),
    }
}

/// The Q07 circuit: 1 V source, 1 kΩ, 1 µF to ground, output labelled
/// `out`; τ = 1 ms.
pub fn rc_schematic(with_ground: bool) -> Schematic {
    let mut components = vec![
        component(
            1,
            "V1",
            (0, 8),
            1,
            Part::VoltageSource {
                source: Source::dc(1.0),
            },
        ),
        component(2, "R1", (8, 0), 0, Part::Resistor { resistance: 1e3 }),
        component(
            3,
            "C1",
            (16, 8),
            1,
            Part::Capacitor {
                capacitance: 1e-6,
                initial_voltage: None,
            },
        ),
    ];
    if with_ground {
        components.push(component(4, "GND1", (8, 16), 0, Part::Ground));
    }
    Schematic {
        components,
        wires: vec![
            wire(5, &[(0, 4), (0, 0), (4, 0)]),
            wire(6, &[(12, 0), (16, 0), (16, 4)]),
            wire(7, &[(0, 12), (0, 16), (8, 16)]),
            wire(8, &[(16, 12), (16, 16), (8, 16)]),
        ],
        labels: vec![NetLabel {
            id: 9,
            name: "out".into(),
            position: GridPoint::new(16, 0),
        }],
        next_id: 10,
    }
}

/// A project with the RC circuit and a 5 ms transient from rest.
pub fn rc_project() -> Project {
    let mut project = Project::new("RC low-pass");
    let circuit = &mut project.circuits[0];
    circuit.schematic = rc_schematic(true);
    circuit.simulation.transient = Transient {
        stop: 5e-3,
        step: 1e-5,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::ZeroState,
    };
    project
}

/// The RC circuit with a transient long enough to stop it by hand.
pub fn long_run_project() -> Project {
    let mut project = rc_project();
    project.circuits[0].simulation.transient = Transient {
        stop: 1.0,
        step: 1e-6,
        start_save: 0.0,
        max_step: Some(1e-6),
        initial_state: InitialState::ZeroState,
    };
    project
}

/// A pulse from 0 to `high`, low for the first millisecond, high for the
/// second, and so on.
fn square(high: f64) -> Source {
    Source {
        dc: 0.0,
        ac_magnitude: 0.0,
        ac_phase: 0.0,
        waveform: Some(Waveform::Pulse {
            initial: 0.0,
            pulsed: high,
            delay: 1e-3,
            rise: 1e-6,
            fall: 1e-6,
            width: 1e-3,
            period: 2e-3,
            cycles: None,
        }),
    }
}

/// One part of [`switch_project`]: designator, position, quarter turns,
/// part and the nets of its pins.
type SwitchBenchPart = (&'static str, (i32, i32), u8, Part, &'static [&'static str]);

/// D17: a voltage-controlled switch S1 and a current-controlled switch W1,
/// each shunting a 1 kΩ load from the 1 V `Vdd`, with every pin joined to
/// its net by a label and a 4 ms transient:
///
/// * `Vctl` squares 0 / 1 V into S1's control (VT = 0.5 V);
/// * `Vdrv` squares 0 / 2 V through R2 (1 kΩ) into the 0 V `Vsense`, whose
///   current (0 / 2 mA into its + pin) controls W1 (IT = 1 mA).
///
/// `out` (S1) and `out2` (W1) are near 1 V while the switch is off (the
/// first and third millisecond) and near 1 mV while it is on (RON = 1 Ω,
/// ROFF = 1 MΩ).
pub fn switch_project() -> Project {
    let resistor = Part::Resistor { resistance: 1e3 };
    let current_switch = match Part::default_for(PartKind::CurrentSwitch) {
        Part::CurrentSwitch { model, .. } => Part::CurrentSwitch {
            control: "Vsense".into(),
            model,
        },
        other => other,
    };
    let parts: [SwitchBenchPart; 9] = [
        (
            "Vdd",
            (-40, -12),
            0,
            Part::VoltageSource {
                source: Source::dc(1.0),
            },
            &["vdd", "0"],
        ),
        (
            "Vctl",
            (-20, -12),
            0,
            Part::VoltageSource {
                source: square(1.0),
            },
            &["ctl", "0"],
        ),
        ("R1", (0, -12), 0, resistor.clone(), &["vdd", "out"]),
        (
            "S1",
            (20, -12),
            0,
            Part::default_for(PartKind::VoltageSwitch),
            &["out", "0", "ctl", "0"],
        ),
        (
            "Vdrv",
            (40, -12),
            0,
            Part::VoltageSource {
                source: square(2.0),
            },
            &["drv", "0"],
        ),
        ("R2", (-40, 12), 0, resistor.clone(), &["drv", "sense"]),
        (
            "Vsense",
            (-20, 12),
            0,
            Part::VoltageSource {
                source: Source::dc(0.0),
            },
            &["sense", "0"],
        ),
        ("R3", (0, 12), 0, resistor, &["vdd", "out2"]),
        ("W1", (20, 12), 1, current_switch, &["out2", "0"]),
    ];
    let mut schematic = Schematic::default();
    for (name, position, rotation, part, nets) in parts {
        let id = schematic.allocate_id();
        let c = component(id, name, position, rotation, part);
        for (pin, net) in c.pins().into_iter().zip(nets) {
            let id = schematic.allocate_id();
            schematic.labels.push(NetLabel {
                id,
                name: net.to_string(),
                position: pin,
            });
        }
        schematic.components.push(c);
    }
    let mut project = Project::new("Switches");
    let circuit = &mut project.circuits[0];
    circuit.schematic = schematic;
    circuit.simulation.transient = Transient {
        stop: 4e-3,
        step: 1e-6,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::OperatingPoint,
    };
    project
}

/// Where the op-amp of [`opamp_project`] sits.
pub const OPAMP_AT: (i32, i32) = (0, 0);

/// D26: a non-inverting amplifier of gain 10 with the general-purpose
/// op-amp `OA_GP` on ±15 V rails, set to the operating point: `Vin` (1 V)
/// into `+`, `Rf` 9 kΩ from `out` to `fb` (`−`), `Rg` 1 kΩ from `fb` to
/// ground and a 2 kΩ load `RL`, every pin joined to its net by a label.
pub fn opamp_project() -> Project {
    let r = |resistance| Part::Resistor { resistance };
    let dc = |volts| Part::VoltageSource {
        source: Source::dc(volts),
    };
    let parts: [SwitchBenchPart; 7] = [
        (
            "U1",
            OPAMP_AT,
            0,
            Part::OpAmp {
                model: "OA_GP".into(),
            },
            &["in", "fb", "out", "vp", "vn"],
        ),
        ("Vin", (-24, 16), 0, dc(1.0), &["in", "0"]),
        ("Vp", (-24, -16), 0, dc(15.0), &["vp", "0"]),
        ("Vn", (0, -16), 0, dc(-15.0), &["vn", "0"]),
        ("Rf", (24, -16), 0, r(9e3), &["out", "fb"]),
        ("Rg", (24, 16), 0, r(1e3), &["fb", "0"]),
        ("RL", (0, 16), 0, r(2e3), &["out", "0"]),
    ];
    let mut schematic = Schematic::default();
    for (name, position, rotation, part, nets) in parts {
        let id = schematic.allocate_id();
        let c = component(id, name, position, rotation, part);
        for (pin, net) in c.pins().into_iter().zip(nets) {
            let id = schematic.allocate_id();
            schematic.labels.push(NetLabel {
                id,
                name: net.to_string(),
                position: pin,
            });
        }
        schematic.components.push(c);
    }
    let mut project = Project::new("Amplifier");
    let circuit = &mut project.circuits[0];
    circuit.schematic = schematic;
    circuit.simulation.selected = xsim_project::AnalysisChoice::OperatingPoint;
    project
}

/// D15: a 1 V source driving two 1 µF capacitors in series to ground. Their
/// junction, labelled `mid`, has no DC path to ground.
pub fn series_capacitor_project() -> Project {
    let mut project = rc_project();
    let schematic = &mut project.circuits[0].schematic;
    for component in &mut schematic.components {
        match component.name.as_str() {
            "R1" => {
                component.name = "C2".into();
                component.part = Part::Capacitor {
                    capacitance: 1e-6,
                    initial_voltage: None,
                };
            }
            "C1" => component.name = "C3".into(),
            _ => {}
        }
    }
    schematic.labels[0].name = "mid".into();
    project
}

/// The dialog window over `cx`'s window (D22), the topmost one when
/// dialogs are stacked, as a test context of its own.
pub fn try_dialog(cx: &VisualTestContext) -> Option<VisualTestContext> {
    use gpui_kit::VisualContext as _;
    let owner = cx.window_handle();
    let top = cx
        .cx
        .read(|app| xsim_editor::dialog_window::topmost(owner, app));
    (top != owner).then(|| VisualTestContext::from_window(top, &cx.cx))
}

/// [`try_dialog`], which must find one.
pub fn dialog(cx: &VisualTestContext) -> VisualTestContext {
    try_dialog(cx).expect("a dialog window is open")
}

/// Whether a dialog window is open over `cx`'s window.
pub fn has_dialog(cx: &VisualTestContext) -> bool {
    try_dialog(cx).is_some()
}

/// Presses and releases `key` in a dialog window, as [`press`] does; when
/// the press closes the window, the release goes nowhere.
pub fn press_in_dialog(key: &'static str, cx: &mut VisualTestContext) {
    use gpui_kit::VisualContext as _;
    let keystroke = Keystroke::parse(key).unwrap_or_else(|_| panic!("{key} is not a key"));
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    if cx.windows().contains(&cx.window_handle()) {
        cx.simulate_event(KeyUpEvent { keystroke });
    }
    cx.run_until_parked();
}
