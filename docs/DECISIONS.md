# Decisions

Decisions with significant consequences (Q174). Each entry states the decision, why, and what it affects.

## D1 — Workspace of independent crates

xsim is a Cargo workspace:

| Crate | Path | Role | Depends on |
| --- | --- | --- | --- |
| `xsim-spice-engine` | `crates/engine` | Circuit simulator library: MNA, devices, analyses (Q09, Q98) | nalgebra, faer, serde |
| `xsim-project` | `crates/project` | Project document, schematic model, netlisting, file I/O, app data | engine |
| `xsim-waveform` | `crates/waveform` | Trace expressions, measurements, decimation, ticks, CSV | engine |
| `xsim-editor` | `crates/editor` | GPUI schematic editor, part properties, component library | project, engine, waveform |
| `xsim-plot` | `crates/plot` | GPUI waveform viewer, image export | project, engine, waveform |
| `xsim-desktop` | `crates/desktop` | The `xsim` application shell | all of the above |

**Why:** the engine must stay usable without the UI (Q98). Splitting the UI into editor, plot and shell crates keeps each crate small and lets it be tested on its own.

## D2 — Charge-based device interface

Devices express capacitances and inductances as state quantities (charge or flux) and their derivatives. `LoadContext::integrate` applies the active integration method.

**Why:** devices stay independent of the integration method (Q105). Local truncation error is estimated from the state histories alone (Q106), and adding a method never touches device code.

## D3 — Linear solvers

The engine uses a sparse LU (faer, sequential) in production and a dense LU (nalgebra) as the reference. `SolverKind::Auto` uses dense for up to 60 unknowns.

**Why:** Q101 asks for existing solvers. Q102 allows dense as a reference or fallback. Sequential factorization keeps results deterministic (Q108).

## D4 — Project file is readable JSON (owner decision, 2026-11-22)

Q33 asks for CPU performance and Q42 for clean Git diffs. The owner chose readable JSON for everything, including stored results. Output is canonical: struct field order, 2-space indent, LF line endings and a trailing newline. Floats round-trip exactly, and every save is atomic. The schema is versioned and migrates older schemas: schema 1 is the first prototype, schema 2 the first rebuild format, and schema 3 (D22, doubled coordinate resolution) is current.

## D5 — Transient start (owner decision, 2026-11-22)

By default a transient starts from the DC operating point, as SPICE does. A per-circuit **Start from rest** option (`InitialState::ZeroState`) switches sources on at t = 0 with capacitors discharged and inductors de-energized, or at their initial conditions. The bundled RC example uses Start from rest. Schema-1 files migrate to it, because the prototype always behaved that way.

## D6 — Part selection (owner decision, 2026-11-22, completes Q54)

The schematic toolbar holds the common parts: voltage and current source, resistor, inductor, capacitor, diode, NPN and PNP transistor, ground, wire and net label. A **Component library** window offers every part and model. It has a search field, a category tree, a list, and a preview with metadata (Q144).

## D7 — Failure diagnostics (owner decision, 2026-11-22, clarifies Q162)

A failed simulation reports where it failed (time, sweep value, node or branch). It also reports what percentage of the run was complete, for example `Simulation did not converge at 1.23 ms (41.0% complete)`.

## D8 — Unanswered questions are not built

Q85 (MOSFET models) is unanswered, so "transistor" means BJT and JFET only. Q51, Q55, Q121, Q146 and Q148–Q152 are also unanswered (Q165 was answered later, see D21). Nothing is built for them until they are answered.

## D9 — Mixed units in one plot pane (owner decision, 2026-11-22, completes Q128)

A pane whose traces have different units shows the first unit on the left y-axis and a second unit on its own scaled axis on the right (LTspice style). A trace with a third unit is added to a new pane.

## D10 — No update checker (owner decision, 2026-11-22, resolves Q20 vs Q175)

Q175 wins: the app never uses the network. There is no update checker, automatic or manual. Updates are installed by hand.

## D11 — Trace colours are pinned (owner decision, 2026-11-22, completes Q130)

A trace gets the next free palette colour when it is added, and the colour is stored with the trace in the project. Removing a trace never recolours the others.

## D12 — Library search scope (owner decision, 2026-11-22, completes Q144)

Typing in the Component library search field searches every category. The category tree shows match counts. Clearing the search returns to the selected category.

## D13 — The bundled model library is generic (owner decision, 2026-11-22, resolves Q141/Q175 against Q84)

Many manufacturer model files carry restrictive notices, for example "Commercial Use or Resale Restricted" or a third-party copyright. xsim therefore ships only generic models that xsim authored itself: a generic small-signal NPN, a generic silicon diode and so on. Each one has its design targets documented, and none copies a vendor's or another simulator's parameters. Part-numbered models (Q84: 2N2222, BC547, BC557, BC558, …) will come from manufacturer libraries that users import themselves (Q142).

## D14 — Owner UI feedback, round 1 (2026-11-22)

Plot:
- The mouse wheel cannot zoom out past the full data extent, so traces never shrink below the plot's width or height.
- A **magnifier** tool in the plot toolbar is needed for box zoom. With the magnifier off, left-drag pans the plot; cursor lines can still be dragged directly.
- A trace chip can be dragged from a pane's legend onto another pane to move the trace there.
- **Cursors are crosshairs.** Each cursor has a vertical line at x and a horizontal line at y, and both are always visible while cursors are shown.
  - x1 and y1 are linked, and so are x2 and y2: y follows the cursor's trace at x.
  - Dragging the horizontal line moves the cursor along y, and x follows the trace.
  - The readout pane below shows Δx and Δy.
- The expression completion list opens below the input, so the typed formula stays visible.

Schematic:
- Single wire segments can be dragged to realign them.
- The preview of a voltage or current source shows one period of its waveform.
- The simulation settings window uses the same tab design as the source-type tabs.
- The schematic toolbar has the same height as the plot toolbar.
- The component library search field's focus ring is not clipped.
- Clicking a whole category row in the library tree opens or closes it, and the arrow has no hover background.
- Circuit tabs use the kit's Folder tab design, with the "+" right after the last tab and a tighter margin after the close icon.
- The dot grid is drawn twice as dense (every half unit at the default zoom). Snapping stays at whole units. *(Amended by D22: snapping now matches every dot.)*

## D15 — Floating capacitor nodes (owner decision, 2026-11-22)

A node with no DC path to ground that is reached only through capacitors (the middle of series capacitors, for example) is tied to ground through gmin for the operating point only, as LTspice does. The diagnostics dock shows a warning naming the node, and the run continues. A node fed only by a current source still gives a precise error.

## D16 — Initial conditions use LTspice semantics (owner decision, 2026-11-22, amends D5)

Capacitor and inductor initial conditions always apply. With the default operating-point start they constrain the starting operating point: a capacitor starts at its IC and the rest of the circuit is solved around it. With "Start from rest" they apply as before.

## D17 — Switches (owner decision, 2026-11-22, builds Q87)

Two switch parts are built now: a voltage-controlled switch with 4 pins and a RON/ROFF/VT/VH model, and a current-controlled switch controlled by the current through a named voltage source (like SPICE W). Both appear in the engine, the schematic, the toolbar or library, and the properties dialog.

## D18 — Bounded transient results (owner decision, 2026-11-22)

A transient result keeps the output-grid points, the breakpoints, and every accepted point needed for linear interpolation to reproduce the waveform within reltol·swing + vntol, so precision is kept. A memory cap stops a run that would still exceed it, with a clear error and the percentage complete (D7).

## D19 — Owner UI feedback, round 2 (2026-11-22, amends D14)

- Panels open and close without animation. This covers dialogs (properties, analysis settings, component library, settings, recovery), the diagnostics dock, the plot's side panels, and menus and popovers. Implementation note: this uses the kit's global reduce-motion switch, which also makes tooltips and tab highlights instant. The run spinner is drawn by xsim and still turns.
- Plot cursors move on the x axis only. Each cursor is a vertical line dragged along x, with no horizontal cursor lines. The readout still shows each cursor's value of its trace at x (y1, y2), Δx and Δy. This replaces D14's crosshair cursors.

## D20 — Owner UI feedback, round 3 (2026-11-23)

Schematic:
- Pressing on a component terminal in the Select tool starts a wire from it. While simulation results are shown, a click on a terminal still probes, as documented.
- When two parts touch terminal to terminal and one is dragged away, a wire is inserted so the connection follows.

Plot:
- No axis titles that are only a unit name ("Power (W)", "Current (A)", "Time (s)"). The unit next to each tick number is enough.
- Every pane shows the time (x) axis numbers, not only the bottom pane.
- Narrow spikes in a compressed view should not look like a thin hair. Fix it only if a simple rendering change does it.

Shell:
- The analysis-type dropdown next to Run is removed.
- Run and Simulation settings move to the right end of the circuit tab bar.
- Copy plot is removed from the top bar; the plot has its own copy buttons.
- The app uses the owner's xsim logo (the blue wave behind "xsim") as its window/taskbar icon and in the title bar.

Source properties:
- The preview shows no "1 period · 1 ms" caption. The plot has equal margins left of the y-axis labels and right of the trace, with no extra gap under the "Preview" title.
- The voltage/current source dialog is narrower: input boxes are half as wide.

## Awaiting owner confirmation (choices made during implementation, 2026-11-23)

The implementers made these choices where the decisions above did not say; each was reviewed. Change any of them by telling us, and the entry moves into a decision.

- **Engine:**
  - D16 conflict policy: when initial conditions cannot all hold in the starting operating point (for example a capacitor IC across an inductor), xsim warns and releases the hold. LTspice instead forces it, with huge currents.
  - A PWL with repeat whose last value differs from its first jumps ideally at each repeat. LTspice 24 rejects that file.
  - Switches change state exactly at their located threshold crossing (ideal switching), and integration restarts there.
  - BJT substrate capacitance CJS is not temperature-scaled, as in SPICE3 and LTspice.
  - `I(S1)` is positive into the switch's P terminal (opposite sign to LTspice). `I(W1)` matches LTspice.
- **Schematic:**
  - A press on a pin starts a wire unless results are shown (then it probes).
  - The Select tool stays active after a wire started from a pin.
  - Shift or Ctrl on a pin toggles the part's selection.
  - A wire inserted when pulled-apart parts separate is an ordinary wire afterwards.
  - A new current switch's control defaults to "V1".
  - Part names allow ASCII letters, digits and `_` only.
  - In the library tree, a double click toggles a category once.
- **Plot:**
  - The magnifier stays on until it is clicked again or Escape is pressed.
  - A horizontal-line search only looks in the visible range (superseded by D19's x-only cursors).
  - A DC sweep keeps the swept source's name ("V1") under the bottom pane.
  - The cursor table keeps column headings such as "Time (s)".
  - Traces are drawn with GPUI paths instead of the kit's Line, so that spikes keep their full width.
- **Shell:**
  - The command bar was removed, and the "results out of date" hint sits in the tab bar before Run.
  - Copy plot remains as Simulate > Copy plot (Ctrl+Shift+C).
  - The button and dialog are still titled "Analysis settings"; renaming them "Simulation settings" is recommended.
  - The light-theme logo uses dark letters with a light halo.
  - The circuit tab separators sit slightly off centre above 125% UI scale, a limit of the kit.
- **Dialogs:**
  - Enter on a focused button presses it; Enter in a form applies it when valid.
  - The source preview's y labels sit inside the curve's height, and a repeating PWL shows its first repeat.

## D21 — License (owner decision, 2026-11-23, answers Q165)

xsim is dual-licensed under MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`; `license` in every crate manifest). The repository is public at https://github.com/GrosuCristi/xsim.

## D22 — Owner UI feedback, round 4 (2026-11-23)

- **Dialogs are separate OS windows.** Every panel that used to open on top of the content now opens as its own modal OS window: part properties, component library, simulation settings, settings and crash recovery. The window is centred over the main window and stays in front of it. The main window ignores clicks and keys until the dialog closes; clicking the main window brings the dialog back to the front. There is still no open/close animation (D19).
- **Add pane** is a plus icon at the very bottom of the plot dock.
- **Snapping matches the dot grid.** Parts, wires and labels snap to every visible dot, which is half the previous step. The project file therefore stores coordinates at the finer resolution: schema 3 doubles every coordinate, and older files migrate when opened. Symbols keep their size on screen.
- **D22 (dialog windows, plot dock, grid):**
  - A dialog's window is sized when it opens and grows or shrinks downward if its content changes.
  - The library window is resizable; the other dialog windows fit their content.
  - Recovery Discard removes the row in place.
  - The dot grid is uniform, with no major-grid emphasis.
  - The add-pane plus keeps the toolbar's left inset, below the readout.

## D23 — Owner UI feedback, round 5 (2026-11-23)

- The Run button shows the selected simulation type to the right of "Run", for example "Run  Transient", in secondary text. It updates when the analysis changes.
- Plot pane heights can be changed by dragging the divider between two panes. Heights are saved with the plot setup in the project.
- **D23 (Run label, pane heights):**
  - Run uses the dialog's names, so AC shows as "AC sweep".
  - Below 38 rem of window width, "Op. point" is used and the out-of-date hint is shortened.
  - When Run's text changes, the active tab is scrolled back into view.
  - Double-clicking a pane divider makes the two panes equal.
  - The minimum pane height is 7.5 rem.
  - Pane heights are stored as relative weights.

## D24 — Dialog windows open at their full height (bug fix, 2026-11-23)

The owner reported that most dialog windows were too short and needed scrolling to reach OK/Cancel. Cause: a dialog window opened centred at an estimated height, then resized to its measured content. GPUI cannot move an open window, so it could only grow downward until the screen bottom, which is reached quickly at 200% display scaling (about 1030 logical px of height). Fix: a dialog window must open with its real content height, fully on screen and centred over the main window. It scrolls only when the content is taller than the screen's work area.

## D25 — Native prompts must not block inside UI callbacks (crash fix, 2026-11-23)

The owner reported a crash (exit 0xc0000409) when closing a second circuit tab. Likely cause: the native confirmation box (`rfd`) was opened synchronously inside GPUI's click handler. Its nested Windows message loop re-entered GPUI while state was borrowed, so the app panicked, and a panic in a window callback aborts the process. The same pattern was used for unsaved-changes, open, save-as and export prompts. Fix: every native prompt runs asynchronously, and its answer is applied after it returns. Panics are also logged to a crash file in the app-data folder so future crashes can be diagnosed.
- **D24 and D25 implementation (for confirmation):**
  - A dialog is measured in a hidden probe window, then opens at its real height.
  - It reserves room for the taller states it can switch to (other waveform or analysis types, a few more PWL rows, a larger UI scale). It therefore opens slightly above centre when needed.
  - The OK/Cancel footer is pinned; only content taller than the screen scrolls above it. This replaces D22's "footer included".
  - The Windows frame size is compensated for Dialog-kind windows.
  - Native questions are GPUI task dialogs with Save / Don't save / Cancel and Delete / Cancel buttons; their title bar shows "Warning".
  - While a question is open, other prompting commands are ignored.
  - Panics are written to `crash.log` in the app-data folder; it rotates past 1 MiB.
  - Settings and copy-plot failures are reported in the diagnostics dock instead of toasts.
