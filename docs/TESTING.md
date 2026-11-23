# Testing and quality

[Question index](QUESTION_INDEX.md)

## Numerical accuracy and reference simulations

### Q153

★ What numerical accuracy is acceptable compared with reference simulations: specified absolute/relative tolerances per analysis, or a general accuracy goal?

6 numbers after decimal point

### Q154

Should we use ngspice, LTspice, independently calculated circuits, and analytical solutions as validation references?

yes

### Q155

Will we maintain a versioned collection of reference netlists and expected results?

yes

## Automated tests, CI, and performance

### Q156

Should coding agent build parser fuzz tests and randomized circuit/property tests?

yes

### Q157

Should every schematic editing operation be covered by automated UI integration tests where practical?

yes

### Q158

What cross-platform CI must pass before merging changes?

none for now

### Q159

What benchmark circuits should detect numerical correctness and performance regressions?

low pass filters, high pass filters, inductive and capacitive circuits, switching transistors/switching power dissipation, etc...

### Q160

Should we enforce memory limits and test very long-running simulations for leaks?

yes

## Build reproducibility, diagnostics, and release checks

### Q161

Do we require reproducible builds, pinned dependencies, and a committed `Cargo.lock`?

YES

### Q162

What diagnostics must appear when simulations fail, rather than simply reporting that they did not converge?

the of the gabarits percentage 

### Q163

Should security testing include malicious netlists, recursive includes, oversized inputs, and untrusted project archives?

no

### Q164

Should releases be blocked if any previously supported SPICE syntax, device model, or file-format compatibility test regresses?

no

## Related questions

- [Q06](PRODUCT.md#q06) — Reference circuit examples
- [Q80](SPICE_COMPATIBILITY.md#q80) — Syntax compatibility tests
- [Q96](DEVICE_MODELS.md#q96) — Device model reference tests
- [Q111](SPICE_ENGINE.md#q111) — Engine diagnostics
- [Q178](ROADMAP.md#q178) — Milestone acceptance tests
