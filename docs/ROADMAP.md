# Roadmap and coding agent development process

[Question index](QUESTION_INDEX.md)

## Milestone-based delivery

### Q173

★ Should coding agent work through small milestone-based pull requests, each independently reviewable and testable?

no

## coding agent decision authority

### Q176

★ When coding agent encounters unspecified product behavior, must it stop and ask rather than choosing on your behalf?

stop and ask

### Q177

What kinds of internal implementation decisions may coding agent make independently?

ones that are extremely obvious to the architectire of the project

## Milestone acceptance and compatibility review

### Q178

Should each milestone require passing tests, updated documentation, and a working end-to-end demonstration?

yes please. but these should stay local and not pushed to git

### Q179

Should coding agent maintain a feature-completeness matrix and record unsupported or partially implemented functionality?

yes

### Q180

Should we require an initial architecture and compatibility review before coding agent starts writing production application code?

not necessarily

## Related questions

- [Q07](PRODUCT.md#q07) — First usable milestone
- [Q164](TESTING.md#q164) — Compatibility regression gates
- [Q172](GOVERNANCE.md#q172) — Release and versioning policy
- [Q174](ARCHITECTURE.md#q174) — Architecture decision and dependency policies
