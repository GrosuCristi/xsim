# Project file format

[Question index](QUESTION_INDEX.md)

## Project container and versioning

### Q31

★ Should a xsim project be a directory containing multiple files, a single `.xsim_proj` file, or support both?

a single .xsim_proj file

### Q32

What should the native extensions be for projects, schematics, symbols, and simulation results?

the base extension name is .xsim_proj, then for symbols it should be .xsim_sym and so on

### Q33

Should native project files be human-readable JSON/TOML, a packaged archive, or a binary format?

what ever is more cpu performant

### Q34

Should documents have an explicit schema version with automatic migrations?

yes

## Project contents and saved state

### Q35

Can one project contain multiple independent circuits, each with its own simulation configuration?

yes

### Q36

Must projects support hierarchical schematics, reusable subcircuits, and nested designs?

no

### Q37

Should a project store its own component symbols and models, or reference shared global libraries?

it must be able to do both

### Q38

Should simulation results be stored in the project by default, stored externally, or discarded unless explicitly saved?

stored in the project by default

### Q39

Should plots, measurement configurations, simulation settings, and window layouts be saved per project?

yes per project

## Saving, recovery, and Git diffs

### Q40

How should autosave work: periodically, after every change, on focus loss, or only manually?

only manual save

### Q41

Should crash recovery restore unsaved changes and possibly previous simulation results?

yes

### Q42

Must native files generate clean Git diffs with stable ordering and minimal unrelated changes?

yes

## Dependencies and forward compatibility

### Q43

Should projects have relative paths, embedded dependency snapshots, or a choice between linked and portable dependencies?

a choice between linked and portable dependencies

### Q44

What happens if a model file is missing, has changed externally, or exists in multiple library directories?

show the user and error, actions that require that specific model should not be runnable. viewing the project schematics should still be possible.

### Q45

How should xsim handle files created by a newer incompatible version?

try opening them. let the user know that some features may not work.

## Project creation and discovery

### Q46

Should xsim offer project templates, recent-project history, project search, and a project creation wizard?

just a recent project history

## Related questions

- [Q63](SPICE_COMPATIBILITY.md#q63) — LTspice schematic import
- [Q143](LIBRARIES_AND_EXTENSIONS.md#q143) — Library dependency placement
- [Q172](GOVERNANCE.md#q172) — Versioning and compatibility policy
