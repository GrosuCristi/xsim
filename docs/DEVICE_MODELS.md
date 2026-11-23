# Electrical device models

[Question index](QUESTION_INDEX.md)

## Core electrical devices

### Q81

★ Which device families must work in the first simulation-engine milestone?

resistors, current sources, inductors or coils, diodes, transistors, voltage sources. 

### Q82

Must independent and controlled sources (V, I, E, F, G, H) be supported from the beginning?

stick with V and I for now

### Q83

Which diode effects must be modeled: forward conduction, reverse breakdown, capacitance, charge storage, and temperature?

yes, all of them

### Q84

Which BJT and JFET model variants are required?

2N2222, BC547, BC557, BC558. you can get the first 10 most used models of each one from the internet

### Q85

★ How far should MOSFET compatibility extend: basic SPICE MOS models, VDMOS, BSIM families, and other advanced compact models?

*Unanswered*

### Q86

Must xsim support IGBTs, power MOSFETs, and specialized power-electronics devices?

yes

### Q87

Are voltage-controlled and current-controlled switches required?

yes

### Q88

Must arbitrary behavioral voltage/current sources and nonlinear expressions be supported?

yes

## Magnetic, high-frequency, and mixed-signal devices

### Q89

What transformer and magnetic-component modeling is needed: coupled inductors, saturation, hysteresis, or detailed core-loss models?

coupled inductors, saturation and hysteresis

### Q90

Should transmission lines and frequency-dependent transmission-line models be included?

Lossless line: just a delay and a characteristic impedance. make these parameters be modifiable

### Q91

Should ideal op-amps, comparators, controllers, and other convenience components be built in?

yes

### Q92

Must analog and digital devices operate together in one simulation?

yes

### Q93

Should xsim replicate LTspice's special mixed-mode device behavior, including its proprietary device extensions where independently implementable?

yes

### Q94

How completely must temperature dependencies, self-heating, thermal noise, and device parasitics be modeled?

as complete as the ltspice one

## Model extensions and reference tests

### Q95

Should contributors be able to implement new device models through a documented Rust extension interface?

not necessarily. we should do like ltspice where manufacturers are able to create spice models of their compoentns. our simulator must be able to interpret external spice models and component symbols

### Q96

Should every device model have automated reference tests across operating points, temperatures, and relevant frequencies?

Well, it should not be close to ltspice, it should be close to reality. these test should only remain in the development phase and must not end up in the production version

## Related questions

- [Q71](SPICE_COMPATIBILITY.md#q71) — Model syntax and parameters
- [Q123](ARCHITECTURE.md#q123) — Mixed-signal execution
- [Q141](LIBRARIES_AND_EXTENSIONS.md#q141) — Bundled model library
- [Q153](TESTING.md#q153) — Reference accuracy
