# Open-source governance

[Question index](QUESTION_INDEX.md)

## Licensing and project identity

### Q165

★ Which license should xsim use: MIT, Apache-2.0, MIT/Apache-2.0 dual licensing, GPLv3, or another?

*Unanswered*

### Q166

★ With xsim as the public application name, should we choose distinct Rust crate names such as `xsim-spice-engine` and `xsim-desktop`?

not necessarily. we wont publish the spice engine on the global crates network nor the xsim-desktop will be published on the crates. but yeah, we could keep them both 

### Q167

What should the GitHub organization and repository name be?

GrosuCristi/xsim

## Contributions, approvals, and data collection

### Q168

Should contributions use a Developer Certificate of Origin (DCO), a Contributor License Agreement, or standard pull-request contributions?

standard pull request

### Q169

Who can approve architectural changes, model-library additions, and releases?

me

### Q170

Should the project accept optional telemetry/crash reports, or prohibit all automatic data collection?

prohibit automatic data collection

## Documentation and release policy

### Q171

What documentation is mandatory: user manual, developer handbook, API reference, compatibility matrix, and tutorial circuits?

no documentation for now

### Q172

What release/versioning approach should we use, and when should backwards compatibility become a guarantee?

releases will be published on github releases

## Related questions

- [Q141](LIBRARIES_AND_EXTENSIONS.md#q141) — Bundled library licenses
- [Q174](ARCHITECTURE.md#q174) — Architecture Decision Records
- [Q175](ARCHITECTURE.md#q175) — Dependency constraints
- [Q178](ROADMAP.md#q178) — Milestone requirements
