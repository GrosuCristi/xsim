# Libraries, extensions, and integrations

[Question index](QUESTION_INDEX.md)

## Model libraries and schematic symbols

### Q141

★ Should the application ship with a permissively licensed generic component/model library, rather than depending on proprietary LTspice assets?

generic component/model library

### Q142

Should users be able to install their own manufacturer SPICE libraries and custom component symbols?

yes

### Q143

Should model dependencies be copied into projects, referenced globally, or controlled per dependency?

the generic component library should never be copied by default since we know for sure everyone has the same models. the user must be able to choose wether a model dependency gets copied or globally referenced. be default it should be globally referenced 

### Q144

How should the model browser handle categories, metadata, search, preview, pin mapping, and version information?

well, we have a tree on the left with the categories, then we have a window on the right that displays the list of components from the categores.
clicking a component shows a preview at the bottom and all it's metadata. a search bar should be on top that lets the user search trough models.

### Q145

Should xsim have a model-library package manager and optional online model discovery?

NO

### Q146

Should compatible models automatically generate schematic symbols, or require explicit pin mapping?

*Unanswered*

## Plug-ins, scripting, and isolation

### Q147

Should xsim support plug-ins, and if so, can plug-ins extend the UI, device models, analyses, or all three?

NO

### Q148

Should we use GPUI Kit's optional `gpui-shell` JavaScript extension environment, keep plug-ins native Rust, or support both?

*Unanswered*

### Q149

Are Python bindings, a scripting API, or an external automation API required?

*Unanswered*

### Q150

What access should extensions have to files, network connections, processes, and the simulator's internal state?

*Unanswered*

### Q151

Should imported models and third-party extensions execute in a sandbox or restricted environment where feasible?

*Unanswered*

### Q152

Must xsim define a stable, versioned plug-in API from the beginning, or can extensions wait until the core architecture is mature?

*Unanswered*

## Related questions

- [Q43](PROJECT_FILE_FORMAT.md#q43) — Linked and portable dependencies
- [Q44](PROJECT_FILE_FORMAT.md#q44) — Missing and changed models
- [Q56](SCHEMATIC_EDITOR.md#q56) — Visual symbol editing
- [Q79](SPICE_COMPATIBILITY.md#q79) — Proprietary model libraries
- [Q95](DEVICE_MODELS.md#q95) — Rust device-model extension interface
- [Q175](ARCHITECTURE.md#q175) — Dependency constraints
