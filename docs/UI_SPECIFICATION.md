# Desktop UI specification

[Question index](QUESTION_INDEX.md)

## Workspace and appearance

### Q21

★ Should the visual direction resemble LTspice, KiCad, Zed, VS Code, or have an original identity?

keep the native gpui-kit theme for now

### Q22

What should the default layout be: project explorer on the left, schematic in the center, properties on the right, and waveform/output panels below?

schematic editor on the left, simulation results like plots on the right. when no simulation is made, the schematic is on the full window. a bottom closable/openable dock on the schematics frame should show the warnings and the error list. the schematics window would have a toolbar on the top with the basic components. voltage source, restistor, inductance, capacitor, diode, transistor.

### Q23

Should schematics, waveforms, netlists, and model editors all open as tabs in a shared workspace?

yes

### Q24

Should panels be detachable into separate windows and movable between monitors?

no

### Q25

Do you want dark mode, light mode, system theme, and custom accent colors?

just dark and light mode, default is light.

## Shortcuts, rendering, and accessibility

### Q26

Should keyboard shortcuts follow LTspice defaults or a more conventional modern-editor scheme?

conventional modern-editor scheme

### Q27

Must users be able to customize and export keyboard shortcuts?

No

### Q28

★ Should custom schematic/waveform rendering use GPUI's own painting APIs first, with a separate GPU rendering subsystem added only when benchmarks justify it?

yes

### Q29

What input methods must work well: mouse, high-resolution trackpad, touch, pen, or keyboard-only?

mouse and keyboard

### Q30

What requirements do you have for accessibility, high-DPI displays, internationalization, and interface languages?

only english, allow users to scale the UI from the settings

## Related questions

- [Q47](SCHEMATIC_EDITOR.md#q47) — Schematic editing interactions
- [Q127](WAVEFORM_VIEWER.md#q127) — Waveform rendering
- [Q157](TESTING.md#q157) — Automated UI testing
