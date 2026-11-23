<p align="center">
  <img src="crates/desktop/assets/logo-light-128.png" alt="xsim" height="64">
</p>

# xsim

xsim is a circuit simulator in the spirit of LTspice, written in Rust with a native [GPUI Kit](https://crates.io/crates/gpui-kit) desktop interface. It aims for precision first: results agree with LTspice and closed-form solutions to six significant digits on well-posed circuits.

> **Status:** an early milestone. It draws schematics and runs OP, DC sweep, AC and transient analyses on R, L, C, sources, diodes, BJTs, JFETs and switches. It plots, probes and measures the results. See [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) for what works and what is still open.

## Features

- **Schematic editor:**
  - parts from a toolbar and a searchable component library;
  - orthogonal wires (started from a pin or the Wire tool), net labels and T-junctions;
  - drag, rotate, mirror, undo/redo and copy/paste;
  - wire segments that can be dragged, and parts that stay connected when pulled apart.
- **Engine** (`xsim-spice-engine`, usable without the UI):
  - f64 modified nodal analysis with dense and sparse LU;
  - Newton with GMIN, source stepping and pseudo-transient fallbacks;
  - backward Euler, trapezoidal, TR-BDF2 and Gear-2 integration with truncation-error control;
  - devices: Gummel–Poon BJTs, JFETs, diodes, voltage- and current-controlled switches;
  - sources with PULSE, SINE, EXP, PWL and SFFM waveforms.
- **Waveform viewer:**
  - multiple synchronised panes, trace expressions (`V(a)-V(b)`, `db()`, `I(R1)*V(out)`, …) and a second y-axis per pane;
  - Bode layout, cursors and measurements;
  - export to PNG, JPEG, SVG and CSV, and copy to the clipboard.
- **Probing:**
  - click a wire for its voltage, drag between nets for a differential probe, click a part for its current;
  - hover over a part for its RMS voltage, RMS current and average power.
- **Projects:**
  - one readable, diff-friendly `.xsim_proj` JSON file per project, holding several circuits, settings, plot setups and stored results;
  - atomic saves and crash recovery.
- **Models:** a bundled library of generic models written for xsim. Each model's design targets are documented and verified by simulation.

## Build and run

You need the Rust toolchain pinned in `rust-toolchain.toml` (installed automatically by `rustup`).

```sh
cargo run --locked --release -p xsim-desktop -- "examples/RC low-pass.xsim_proj"
```

Press **F5** to simulate. To keep your real settings and recovery data untouched while experimenting, set `XSIM_DATA_DIR` to an empty folder first.

Run the checks:

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

The engine's reference decks in `crates/engine/tests/reference/` record the LTspice runs that the tests compare against.

## Repository layout

| Crate | Role |
| --- | --- |
| `crates/engine` (`xsim-spice-engine`) | Simulator library: MNA, devices, solvers, analyses, generic model library |
| `crates/project` (`xsim-project`) | Schematic model, symbols, netlisting, project file I/O |
| `crates/waveform` (`xsim-waveform`) | Expressions, measurements, plot decimation, CSV |
| `crates/editor` (`xsim-editor`) | Schematic editor, properties dialog, component library window |
| `crates/plot` (`xsim-plot`) | Waveform viewer and image/CSV export |
| `crates/desktop` (`xsim-desktop`) | The `xsim` application |

Product requirements are recorded as answered questions in [`docs/`](docs/QUESTION_INDEX.md), and the decisions taken along the way are in [`docs/DECISIONS.md`](docs/DECISIONS.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in xsim by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
