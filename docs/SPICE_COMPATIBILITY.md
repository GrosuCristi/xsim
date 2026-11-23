# SPICE and LTspice compatibility

[Question index](QUESTION_INDEX.md)

## Schematic, symbol, and netlist interoperability

### Q63

★ Must xsim open existing LTspice `.asc` schematic files?

no

### Q64

★ Must it also save/export `.asc` files that LTspice can reopen, or is one-way import sufficient initially?

no

### Q65

Must custom LTspice `.asy` symbol files be imported and exported?

no

### Q66

★ Should compatibility target LTspice syntax first, or a broader SPICE dialect supporting LTspice, PSpice, ngspice, and standard Berkeley SPICE?

a more generic spice dialect

### Q67

Must xsim support `.cir`, `.sp`, and `.net` files as first-class editable documents?

no

### Q68

Should the parser retain comments, original formatting, unsupported statements, and source locations for round-trip editing?

yes

## Directives, expressions, and sources

### Q69

How closely must `.subckt`, `.ends`, `.include`, `.lib`, nested parameter scopes, and hierarchical node naming match LTspice?

not as close. things must be done from a user interface not commands

### Q70

Must `.param`, `.func`, expression evaluation, parameter substitution, and arbitrary math functions reproduce LTspice semantics?

no, we work with interfaces

### Q71

What level of `.model` syntax and parameter compatibility is required?

again, things should be done trhough a ui

### Q72

Which source functions must be supported: DC, AC, PULSE, PWL, SINE, EXP, SFFM, and behavioral expressions?

DC, AC, PULSE, PWL, SINE, EXP, SFFM

### Q73

Must `.step` support nested parameter, temperature, source, and model-parameter sweeps?

yes

### Q74

How completely should `.meas`/`.measure` support WHEN, FIND, AVG, RMS, MIN, MAX, INTEG, DERIV, and TRIG/TARG?

completely better. If possible avoiding writing and having a UI.

### Q75

Must `.options` use LTspice-compatible settings and defaults, or can xsim have its own names and defaults?

again, we do not do like ltspice, we do better, so we make a UI

### Q76

Must `.ic`, `.nodeset`, `.temp`, initial conditions, and state-loading behavior be compatible?

i mean, we should have them as features, but they should be settable through a ui with input boxes not through code

## Dialect behavior and compatibility validation

### Q77

How should xsim handle SPICE numeric suffixes and case-insensitive identifiers? Should it exactly follow LTspice, including `M` meaning milli rather than mega?

no, it should be case sensitive

### Q78

What happens when xsim encounters an unsupported LTspice-specific element or directive: reject the file, preserve it with a warning, or offer a conversion?

reject

### Q79

How should xsim handle encrypted or proprietary manufacturer model libraries that it cannot legally or technically interpret?

throw error

### Q80

Should every supported syntax feature have a documented compatibility test against a selected LTspice version?

no, we are not comparable to LtSpice. WE ARE THE BETTER VERSION OF LTSPICE

## Related questions

- [Q01](PRODUCT.md#q01) — LTspice reference release
- [Q02](PRODUCT.md#q02) — Compatibility goals
- [Q31](PROJECT_FILE_FORMAT.md#q31) — Native project container
- [Q138](WAVEFORM_VIEWER.md#q138) — LTspice waveform files
- [Q179](ROADMAP.md#q179) — Feature-completeness matrix
