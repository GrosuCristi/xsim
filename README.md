<p align="center">
  <img src="crates/desktop/assets/logo-light-128.png" alt="xsim" height="64">
</p>

# xsim

xsim is a circuit simulator in the spirit of LTspice. Draw a schematic, run it, and probe the results. It is built for precision: results match LTspice and exact solutions to six significant digits.

It is written in Rust with a native desktop interface. It covers operating point, DC sweep, AC and transient analyses, with diodes, bipolar transistors, JFETs and switches.

xsim is in early development. See [what works today](docs/IMPLEMENTATION_STATUS.md).

```sh
cargo run --release -p xsim-desktop
```

## License

MIT or Apache-2.0, at your option.
