# SPICE engine

[Question index](QUESTION_INDEX.md)

## Circuit equations and numerical solvers

### Q99

Is standard double precision (`f64`) sufficient, or do we require higher-precision solving for difficult circuits?

f64 is enough

### Q100

Should Modified Nodal Analysis be the canonical circuit equation formulation?

Yes

### Q101

Should we adopt an existing Rust sparse matrix solver or implement our own solver eventually?

Use existingones

### Q102

Is using a dense solver for small circuits acceptable as a fallback or reference implementation?

yes

### Q103

Which nonlinear solver strategies are required beyond Newton–Raphson?

[Attempt Direct Newton-Raphson] 
       │
       ├──► Success: Proceed to simulation (.DC / .TRAN)
       │
       └──► Fail: Try GMIN Stepping Loop
                    │
                    └──► Fail: Try Source Stepping Loop
                                 │
                                 └──► Fail: Try Pseudo-Transient / Abort

### Q104

Must xsim implement GMIN stepping, source stepping, damping, and other convergence recovery strategies?

*Unanswered*

## Transient integration, tolerances, and parallelism

### Q105

Which integration methods are mandatory: trapezoidal, modified trapezoidal, Gear/BDF?

all of them

### Q106

Must transient simulation implement automatic timestep control and local truncation-error estimation?

               ┌─────────────────────────────┐
               │  Solve MNA Matrix at t_n    │
               └──────────────┬──────────────┘
                              │
               ┌──────────────▼──────────────┐
               │ Did Newton-Raphson Converge?│
               └──────────────┬──────────────┘
                        YES   │   NO
             ┌────────────────┴────────────────┐
             ▼                                 ▼
┌───────────────────────────┐     ┌───────────────────────────┐
│ Estimate LTE (Truncation) │     │ Reject Step!              │
└────────────┬──────────────┘     │ Shrink dt (e.g., dt / 8)  │
             │                    └────────────┬──────────────┘
      ┌──────┴──────┐                          │
  LTE ≤ Tol     LTE > Tol                      │
      ▼             ▼                          │
┌───────────┐ ┌─────────────┐                  │
│Accept Step│ │Reject Step! │                  │
│Grow dt    │ │Shrink dt    │                  │
└─────┬─────┘ └─────┬───────┘                  │
      │             │                          │
      └─────────────┴──────────────────────────┴─► [Loop to next try]


### Q107

Should default solver tolerances imitate LTspice, or should xsim define independent documented defaults?

independent defaults

### Q108

Is deterministic numerical output across runs required, even when parallel processing is enabled?

yes

### Q109

Should parallelization focus on independent parameter sweeps, single-circuit solving, or both?

yes

## Execution control, diagnostics, and performance

### Q110

Must simulations support cancellation, pause/resume, and safe interruption?

yes

### Q111

What diagnostic information should be visible: iteration counts, timestep rejections, matrix problems, singular nodes, convergence history?

none

### Q112

Should simulation state be serializable to resume long simulations later?

no

### Q113

What circuit sizes should be practical on ordinary hardware: hundreds, thousands, or tens of thousands of elements?

hundreds

### Q114

What should our performance objective be relative to LTspice or ngspice, and on which benchmark circuits and hardware?

we dont chase speed, we chase precision

## Analyses and sweeps

### Q115

★ Should the first engine support `.op`, `.dc`, `.ac`, and `.tran`, or should we introduce them one at a time?

all at once

### Q116

Is small-signal noise analysis (`.noise`) mandatory for the first major release?

yes

### Q117

Must transfer-function analysis (`.tf`) be implemented?

yes

### Q118

Must Fourier analysis, FFT, harmonic measurements, and total harmonic distortion be supported?

yes

### Q119

Do we want LTspice-style frequency-response analysis (`.fra`) for switching converters and control loops?

yes but with a better user interface

### Q120

Should temperature, component, parameter, and nested simulation sweeps be runnable in batches?

yes

### Q121

Should Monte Carlo, tolerance, and statistical simulations be included, even where they extend beyond core LTspice features?

*Unanswered*

### Q122

Should sensitivity, pole-zero, and optimization analyses be supported?

yes

## Optimization, recorded signals, and interfaces

### Q124

Should xsim support automated design optimization against measurement targets?

yes

### Q125

Can users configure which node voltages, branch currents, and internal device quantities are recorded?

yes

### Q126

Should users be able to run simulations through both the GUI and a stable CLI with machine-readable results?

only gui

## Related questions

- [Q97](ARCHITECTURE.md#q97) — Engine dependencies and API
- [Q73](SPICE_COMPATIBILITY.md#q73) — SPICE sweep syntax
- [Q123](ARCHITECTURE.md#q123) — Mixed-signal solver architecture
- [Q139](WAVEFORM_VIEWER.md#q139) — Live waveform updates
- [Q153](TESTING.md#q153) — Numerical validation
