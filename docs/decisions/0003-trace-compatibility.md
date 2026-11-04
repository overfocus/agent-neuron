# Decision record 0003: trace v1 and the specification

How SPEC section 7 stays inside trace format v1. TRACE.md's rules: a reader
ignores fields and event types it does not know; adding an optional field or
an event type stays version 1; changing what a field means or the replay rules
bumps `version`.

- Additions are all optional fields (`phase`, `replaces`, `code`, `engine`) or a
  new event type (`note`). v1 readers are unaffected.
- **Issue #16**: held and running work share the reason `replaced`; `phase`
  separates them. See decision 11 in 0001 for why not a new reason.
- **Issue #17**: the drop of a cut threshold call names the crossing input,
  which may not have been replaced. TRACE.md says so. Replay is unaffected (it
  removes a held entry by id, and a running one is not held).
- **Header**: `library` stays a version string; `demo` is now documented as a free
  name; `kind` values are descriptive strings, so the spec's kinds fit.
- **Links** are one-way in the format. The TypeScript library records both
  directions of a `connect`; replay never assumed symmetry.
- A committed-trace test accepts unknown event types, as TRACE.md already requires.
