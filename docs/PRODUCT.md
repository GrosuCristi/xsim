# xsim product

[Question index](QUESTION_INDEX.md)

## Compatibility and interface

### Q01

★ Which exact LTspice release should be our initial reference: a fixed 26.x release, or the latest version at each development milestone?

use the latest one and pin it

### Q02

★ Does full compatibility mean the same supported features, the same netlist syntax, similar numerical results, or all three?

it can be similar but not necessariliy 1 to 1 with ltspice. the project file should contain everything inside it (plot, circuit, etc...). where ltspice scatters them in multiple files.

### Q03

Must xsim reproduce LTspice's workflows and shortcuts, or can it have a completely redesigned interface?

it can have a completely redesigned interface

## Audience and reference circuits

### Q04

Who is the primary audience: electronics students, professional analog engineers, power-electronics engineers, researchers, or everyone?

everyone

### Q05

Which circuit types matter most initially: power supplies, amplifiers, filters, oscillators, RF, digital, or mixed-signal?

power supplies, amplifiers, filters, oscillators, digital

### Q06

Which five real circuits should become our first end-to-end reference examples?

q6: does this really matter?

## First release and product scope

### Q07

★ What should the first usable version accomplish? For example: create a project, draw an RC circuit, simulate it, plot the voltage, save and reopen it.

create a project, draw an RC circuit, simulate it, plot the voltage, save and reopen it, copy the plot to clipboard. 

### Q08

What is explicitly outside xsim's scope? PCB layout, firmware simulation, mechanical simulation, or anything else?

anything else

### Q09

Is the independent simulation library as important as the desktop application?

yes

### Q10

Should xsim prioritize LTspice compatibility over introducing new capabilities that LTspice lacks?

q10: no it should not

## Related questions

- [Q63](SPICE_COMPATIBILITY.md#q63) — LTspice interoperability
- [Q115](SPICE_ENGINE.md#q115) — First engine analyses
- [Q173](ROADMAP.md#q173) — Milestone-based delivery
