# xsim architecture

[Question index](QUESTION_INDEX.md)

## Engine dependencies and public API

### Q97

★ Must the simulation engine be entirely native Rust, or are carefully chosen C/C++ numerical dependencies permitted through FFI?

if crtical components do not exist in rust we can go through FFI. but if there is a rust available solution of course we should use that

### Q98

★ Must the engine remain completely independent of GPUI Kit, with a public API usable by other Rust programs?

yes the engine should an indepent module/sector of the app. i see it as a separate crate. i think it should not be hardcoded into the UI

## Mixed-signal architecture

### Q123

Should mixed-signal simulation use an integrated numerical solver, an event-driven digital subsystem, or another architecture?

integrated numerical solver

## Architecture decisions and dependency boundaries

### Q174

Should every architectural choice require an Architecture Decision Record (ADR), or only decisions with significant consequences?

only decisions with significant consequences

### Q175

Which dependency constraints should coding agent follow: permissive licenses only, no C-based simulation libraries, no network services, etc.?

no network services. permissive licenses only. C-based simulation libraries only if rust alternatives dont exist.

## Related questions

- [Q09](PRODUCT.md#q09) — Independent simulation library
- [Q16](REQUIREMENTS.md#q16) — Headless execution
- [Q22](UI_SPECIFICATION.md#q22) — UI workspace
- [Q28](UI_SPECIFICATION.md#q28) — Canvas rendering
- [Q31](PROJECT_FILE_FORMAT.md#q31) — Project storage
- [Q66](SPICE_COMPATIBILITY.md#q66) — SPICE dialects
- [Q68](SPICE_COMPATIBILITY.md#q68) — Parser and round-trip data
- [Q95](DEVICE_MODELS.md#q95) — Device model extension interface
- [Q99](SPICE_ENGINE.md#q99) — Numerical engine decisions
- [Q125](SPICE_ENGINE.md#q125) — Simulation results and CLI
- [Q127](WAVEFORM_VIEWER.md#q127) — Waveform plotting implementation
- [Q139](WAVEFORM_VIEWER.md#q139) — Streaming and waveform storage
- [Q143](LIBRARIES_AND_EXTENSIONS.md#q143) — Model dependencies
- [Q147](LIBRARIES_AND_EXTENSIONS.md#q147) — Plug-ins and extension architecture
- [Q161](TESTING.md#q161) — Reproducible builds
- [Q169](GOVERNANCE.md#q169) — Approval authority
- [Q176](ROADMAP.md#q176) — coding agent decision authority
