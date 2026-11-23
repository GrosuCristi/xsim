# Platform and distribution requirements

[Question index](QUESTION_INDEX.md)

## Operating systems and CPU architectures

### Q11

★ Which operating systems must be supported from the beginning: Windows, Linux, macOS?

Windows, Linux, macOS

### Q12

Which OS versions are the minimum supported versions?

windows 10. other oses do not really matter

### Q13

Which CPU architectures are required: x86-64, ARM64, or both?

both

### Q14

On Linux, must the application support both X11 and Wayland?

Wayland only

### Q15

Should a browser/WebAssembly version eventually exist?

no

## Execution, installation, and updates

### Q16

Must the simulator also run headlessly through a CLI, with no GUI?

no

### Q17

Which installation formats should be offered: MSI/MSIX, portable ZIP, DMG, AppImage, Flatpak, native packages?

.exe for windows, dmg for apple, AppImage for debian

### Q18

Must simulation and project editing work completely offline?

yes

### Q19

Can users launch multiple independent xsim instances, each with its own projects?

yes

### Q20

Should xsim include a built-in update checker, or should releases be managed entirely through package managers and GitHub?

built in update checker

## Related questions

- [Q98](ARCHITECTURE.md#q98) — Independent engine API
- [Q126](SPICE_ENGINE.md#q126) — GUI and CLI execution
- [Q158](TESTING.md#q158) — Cross-platform CI
