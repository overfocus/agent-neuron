# agent-neuron

A specification for agent-neurons (wrappers around any program, wired together
by a JSON network file), its conformance fixtures, and the Rust engine that
runs it (`rust-engine/`, the `neuron-engine` binary). Read
[CONTRIBUTING.md](CONTRIBUTING.md) for the commands, the layout and the
gotchas; [docs/SPEC.md](docs/SPEC.md) is the specification.

- Run `npm run check` before you finish. It needs a Rust toolchain.
- `spec/fixtures/` is the contract: change a fixture only with the SPEC text it
  pins, and never edit an `expect` to make a test pass.
- Keep the neuron interface small: prefer an option to a method, and a default
  that does nothing to a setting everyone must learn.
- Use made-up data in fixtures, examples and traces: no real names, emails or paths.
