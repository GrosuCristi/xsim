# Implementation status

This page is the Q179 feature-completeness matrix for the current build. The owner's decisions that complete or change the questionnaire are in [DECISIONS.md](DECISIONS.md) (D1–D20).

## Run it

From the workspace root, open the bundled example in the desktop app:

```powershell
cargo run --locked --release -p xsim-desktop -- "examples/RC low-pass.xsim_proj"
```

Press **F5** to run. The 1 V source charges a 1 µF capacitor through 1 kΩ, so V(out) is 0.632121 V at 1 ms and 0.993262 V at 5 ms. Click a wire to plot its voltage, a part to plot its current, or rest the pointer on a part for its RMS voltage, current and power.

To try the app without touching your real settings, recent files or crash-recovery data, set `XSIM_DATA_DIR` to an empty folder first.

## Crates

| Crate | Path | Role |
| --- | --- | --- |
| `xsim-spice-engine` | `crates/engine` | Simulator library with no UI dependency (Q98): MNA, devices, solvers, analyses, generic model library |
| `xsim-project` | `crates/project` | Schematic model, symbols, connectivity and netlisting, `.xsim_proj` file I/O, app data |
| `xsim-waveform` | `crates/waveform` | Trace expressions, measurements, plot decimation, ticks, CSV |
| `xsim-editor` | `crates/editor` | GPUI schematic editor, part properties dialog, component library window |
| `xsim-plot` | `crates/plot` | GPUI waveform viewer and image/CSV export |
| `xsim-desktop` | `crates/desktop` | The `xsim` application shell |

## Feature completeness

| Area | Working in this build | Still open |
| --- | --- | --- |
| Q07 desktop workflow | Create, draw, simulate, probe, plot, copy and export, save, reopen with results shown, crash recovery. Windows x64. | Builds and checks on Linux (Wayland), macOS and ARM64 |
| Q21–30 workspace | GPUI Kit light and dark themes and UI scale 75–200 %. Folder-style circuit tabs with **+**, and Run and Simulation settings at the right end of the tab bar (D20). Schematic on the left and plots on the right after a run. Diagnostics dock. Dialogs (part properties, component library, simulation settings, settings, recovery) open as modal OS windows centred over the main window (D22), with no animation (D19). App logo and Windows icon (D20). | Netlist and model editor tabs |
| Q31–46 project file | Single-file readable JSON, schema 3, written in canonical form with atomic saves. Schema-1 and schema-2 files migrate on open. Newer files open read-only. Multiple circuits, settings, plot setups and stored results; a stored result knows whether the schematic changed since it was computed. Recent files and multi-instance-safe crash recovery. | Copying or referencing model dependencies (Q143; models are referenced by name today); compact storage for very large results |
| Q47–62 schematic | **Parts:** V, I, R, L, C, diode, NPN, PNP, N- and P-JFET, ground, and voltage- and current-controlled switches (D17), from the toolbar or the component library (D6, D12).<br>**Drawing:** snapping to every dot of the grid (D14, D22), orthogonal wires, wires started from a pin (D20), dragging single wire segments (D14), parts that keep touching connections when pulled apart (D20), T-junctions and net labels.<br>**Editing:** select, box select, move, rotate and mirror, undo and redo, copy, paste and duplicate, properties dialogs with a one-period waveform preview.<br>**Probing:** voltage, differential and current probes, and the hover card with RMS V, RMS I and average power (Q62). | Custom symbols (Q56), schematic image export (Q61) |
| Q81–96 device models | **Passive:** R, and L and C with initial conditions.<br>**Sources:** V and I with DC, AC and PULSE, SINE, EXP, PWL and SFFM waveforms.<br>**Diode:** Shockley with RS, breakdown, depletion and diffusion charge, and temperature.<br>**BJT:** Gummel–Poon with charges, parasitic resistances, substrate and temperature.<br>**JFET:** Shichman–Hodges with the B parameter, charges and temperature.<br>**Switches:** voltage-controlled (S) and current-controlled (W) with hysteresis.<br>**Library:** 25 generic models (D13), each with its design targets checked by simulation. | MOSFET (Q85 unanswered); controlled sources E, F, G and H (Q82: V and I only for now); power devices (Q86); behavioural sources (Q88); magnetics (Q89); transmission lines (Q90); op-amps and other convenience parts (Q91); mixed-signal (Q92); a device extension interface (Q95) |
| Q97–115 solver | **Core:** pure Rust, f64 modified nodal analysis. Dense LU (nalgebra) below 61 unknowns and sparse LU (faer, COLAMD) above.<br>**Convergence:** Newton, then GMIN stepping, then source stepping, then pseudo-transient continuation.<br>**Integration:** backward Euler, trapezoidal, TR-BDF2 and Gear-2, with truncation-error control and exact breakpoints.<br>**LTspice behaviour:** floating capacitor nodes warn and simulate (D15); initial conditions use LTspice semantics (D16); transient results are thinned without loss and capped (D18).<br>**Switching:** each switch transition is located at its threshold crossing, and integration restarts there.<br>**Run control:** cancel, pause, resume, progress, and failure messages that give the place and the percentage complete (D7). | Reusing the LU factorisation between steps (speed is secondary, Q114) |
| Q116–126 analyses | Operating point, DC sweep, AC (linear, decade or octave) and transient. All node voltages and device terminal currents are recorded. | Noise (Q116), transfer function (Q117), Fourier and FFT (Q118), FRA (Q119), batched sweeps (Q120), sensitivity, pole-zero and optimisation (Q122, Q124), choosing which quantities to record (Q125). Monte Carlo (Q121) is unanswered. No CLI (Q126). |
| Q127–140 waveforms | **Panes:** several x-synchronised panes; every pane shows x-axis numbers and no unit-only titles (D20).<br>**Traces:** expressions such as `V(a)-V(b)`, `db()`, `ph()`, `I(R1)*V(out)`, `deriv()` and `integ()`. A second y-axis for a second unit (D9). Trace colours stay put when others are removed (D11). Traces can be dragged between panes (D14). Bode layout for AC.<br>**Cursors and measurements:** two x cursors with y values, Δx and Δy (D19); measurements.<br>**Navigation:** magnifier box zoom, pan, a zoom-out limit at the data (D14), and pane heights adjustable by dragging the divider between panes (D23).<br>**Export:** clipboard, PNG, JPEG, SVG and CSV. | XY plots (Q129), FFT view (Q134), sweep-step groups (Q133), disk-backed datasets (Q140) |
| Q63–80, Q141–152 interoperability | Schematics feed the engine directly; generic bundled models (D13) | SPICE netlist parser and export, importing manufacturer libraries (Q142), refusing encrypted models with an error (Q79) |
| Q11–20 distribution | Windows x64 executable with an embedded icon; local only, no network use (Q175, D10) | Installers and packaging; other OSes and architectures |
| Q153–162 testing | Automated suites: unit, integration, headless UI, property and fuzz tests, and LTspice reference decks under `crates/engine/tests/reference/` | Continuous integration and a published benchmark set |

## Accuracy

- **Engine against LTspice 24.1.9.** The reference decks agree to about 1e-9 for diode and JFET operating points, 1e-7 relative for BJTs, and 1e-6 for transients and switching. Two settings are needed for this: model cards written inline in the deck, and `.temp` and `tnom` set to 26.9996851609185 °C, because LTspice uses older physical constants.
- **Engine against closed forms.** RC and RLC transients and the RC low-pass AC response agree to 1e-6 or better.
- **Deliberate differences from LTspice:**
  - `I(S1)` is reported as the current flowing into the switch's P terminal, which is the opposite sign to LTspice.
  - Smooth-mode switches use the exact DC slope in AC analysis.

## Verification

Run these from the workspace root:

```powershell
cargo fmt --all --check
```

```powershell
cargo clippy --locked --workspace --all-targets -- -D warnings
```

```powershell
cargo test --locked --workspace
```

```powershell
cargo build --locked --release -p xsim-desktop
```

Results on 2026-11-23 (Windows 11 x64, Rust 1.99.0):

- Format check and clippy: clean.
- Release build: succeeds.
- Tests: **1546 passed, 0 failed, 4 ignored**. The ignored ones are:
  - an accuracy table that is only meaningful in a release build (engine);
  - a helper that runs inside child processes (project);
  - the test that regenerates the example file (project);
  - a timing check that is only meaningful in a release build (waveform).

**Known limits**, all measured and recorded:

- Backward Euler at the default tolerance needs millions of points per period. It is correct but slow, and the analysis dialog says so.
- Six digits are not reached in two cases. One is series diode strings at very low forward bias. The other is an ideal edge into a sub-nanosecond time constant on runs longer than about 10⁶ time constants, where the time resolution is 10⁻¹⁵ of the run.
- A switch's transition can be one minimum step late on very long runs.

The UI tests run headlessly. The owner checked the app by hand after rounds 1 and 2. The later changes (D14 round 2, D19, D20, D22 and the D17 switch parts) were checked only by headless tests and independent review; nobody has seen them in a real window yet. Run is now at the right end of the circuit tab bar. Copy plot is in the plot's own toolbar and under Simulate > Copy plot (Ctrl+Shift+C).
