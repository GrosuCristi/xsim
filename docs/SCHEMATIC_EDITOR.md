# Schematic editor

[Question index](QUESTION_INDEX.md)

## Canvas, wires, and connectivity

### Q47

★ Should we recreate LTspice-style schematic editing or design a new editor based on modern CAD interactions?

a new editor based on modern CAD interactions

### Q48

What coordinate system, grid spacing, zoom limits, and snap behavior should be used?

Snap components and wires to the visible grid by default.

### Q49

Should wires be orthogonal only, support diagonal segments, or allow arbitrary angles?

allow arbitray angles but not by default 

### Q50

How should junctions work when wires cross: automatic connection, explicit junction dots, or configurable behavior?

a dot where the wire connects. no dot when they dont connect but cross

### Q51

Should the editor support buses, multi-signal labels, and bus entry symbols?

*Unanswered*

### Q52

What types of net naming are required: local labels, global labels, power symbols, and hierarchical ports?

only global label logic. labels are scoped to schematics. no schematic hierarchy is allowed

## Components, properties, and symbols

### Q53

Which transformations must components support: rotation, horizontal mirroring, vertical mirroring, and free positioning?

rotation, horizontal mirroring, vertical mirroring, and free positioning

### Q54

What should the component selection experience be: searchable palette, categorized library, graphical previews, favorites, and recent components?

there will be a window with components that 

### Q55

Should component properties support expressions, parameter references, model selection, and validation while editing?

*Unanswered*

### Q56

★ Must users create their own symbols visually, including pins, graphics, parameters, and reusable templates?

yes, users should be able to do their custom symbols visually, including pins, graphics, parameters

## Editing, validation, export, and probing

### Q57

Should undo/redo cover all schematic operations, and should history survive reopening a project?

undo, redo must cover all schematic operations. history should not survive reopening a project

### Q58

Should copy/paste work between schematics and projects, including associated model references?

yes

### Q59

Should xsim have electrical rule checking (ERC) for floating pins, unconnected wires, invalid connections, and missing references?

no

### Q60

What should happen to wires when a connected component moves or rotates: stretch, reroute, disconnect, or user-selectable behavior?

when it moves the wires stretch/reroute by not creating diagonal lines. if it rotates it gets disconnected.

### Q61

Should schematics support PDF, SVG, PNG, printing, and configurable page sizes?

yes

### Q62

Should users be able to probe voltages, currents, and power directly by clicking the schematic after simulation?

yes, if a user hovers a component a detailed box should appear next to the mouse that shows the rms voltage and current. and the average power dissipation.
if a user clicks and releases on a component terminal or wire hte voltage is measures to ground. if the user clicks and hold down, a little probe appears that will allow the user to drag it to another node and measure the voltage between the 2 nodes.

## Related questions

- [Q22](UI_SPECIFICATION.md#q22) — Workspace layout
- [Q63](SPICE_COMPATIBILITY.md#q63) — Schematic interoperability
- [Q130](WAVEFORM_VIEWER.md#q130) — Probe-to-trace interaction
- [Q146](LIBRARIES_AND_EXTENSIONS.md#q146) — Model pin mapping
