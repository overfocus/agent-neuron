# Decision record 0010: the desk at a table — grants, choices and the record (2026-11-08)

**Status: built in the Rust engine (2026-11-08), on Kevin's direction that the decision surface lives in
agent-neuron, not in each demo's proxy.** Items 1 and 5 of the build order in [MULTIPLAYER.md](../MULTIPLAYER.md)
("Grants on the desk"; the first decision kind it names, choose-between-options), and the cheap slice of item 3
(presence on the page). Durable run state and quiescence (items 2 and 4) are not in this step. Nothing in the
SPEC's normative sections changes: 3.4 leaves the interface to the engine, and Appendix D, which this extends, is
informative.

## The problem

Decision 0008 gave a run one desk and one token: one person answers every item. The team demo
(`examples/team/`) needed three people, so it built what the desk lacked in a proxy of its own (`pageserver.mjs`):
a page per person, each item filtered to the person whose neuron it is and whose role the ask's
`answerable_by` names, vote buttons for the options of the ask, and a shared ledger of who answered what. Every
demo of a team deciding would have to build that proxy again, and the engine would keep no record of who decided.
The proxy is the org layer living outside the referee — the shape MULTIPLAYER argues against.

## The decision

1. **Grants are a file the run is given**: `neuron-engine run <file> --inputs <jsonl> --grants <grants.json>`.
   `{"version": 0, "people": [{"id", "name"?, "roles"?, "answers"}]}`, closed like the network file: `id` is
   lowercase letters, digits, `-` and `_` (at most 32), unique, never `owner`; `answers` names the human neurons
   whose items the person may answer, and each must be a human neuron of the network, or the run is refused before
   it starts. It is a run argument, not a network field: the network file is the house rules for every table; who
   sits at this one is the run's business. JSON, not YAML: the engine reads one format.
2. **Each person gets their own token.** The desk prints one `desk person: {id, name, roles, url}` line per person
   on stderr after the `desk:` line. The run's own token stays the root (MULTIPLAYER: "the token stays the root")
   and answers anything, recorded as `owner`.
3. **The desk checks every answer at its POST.** A person may answer an item when its neuron is in their `answers`
   and the ask's `answerable_by` (a role, or a list of roles), when the payload has one, names one of their roles.
   Anything else is refused with 403, and a `desk refused: {item, neuron, by, why: "not granted"}` line goes to
   stderr, as the desk's answers do. The item keeps waiting.
4. **Who decided is on the record, never in the network.** The `desk answer:` line carries `by` (when the run has
   grants), and `/state` carries the shared `record` of every accepted answer (`item, neuron, kind, text, by?,
   option?, why?, ms`), the viewer's `me`, each item's `yours` and `answerable_by` (the people who may answer it),
   and `people` with each person's count of waiting items. The neuron still receives only the person's text (3.4):
   names are the desk's, never a payload.
5. **`choose` is the one new answer kind.** An ask whose payload has `options: [{id, label?, text?}]` shows one
   button per option (plus No). `POST {item, kind: "choose", option}` replies with the option's `text`, else its
   `id`; an option the item does not have is refused with 400. It is page surface over the same reply as an edit:
   no protocol verb, no new event.
6. **`why` rides on any answer**: one line, at most 500 characters, on the answer line and in the record. It is
   never sent to the network — it is the audit, not the input.
7. **Presence, on the page only**: the page's title counts the items waiting on its viewer, and a table line shows
   everyone's waiting count. Pings outside the page (MULTIPLAYER item 3's MCP notification) are not in this step.

Without `--grants` the desk behaves as 0008 has it, byte for byte on the answer line, except that `/state` gains
`record` and each item `yours: true`.

## Not decided here

- **Durable state.** The record dies with the run, as 0008's desk does. Item 2 of the build order.
- **Inner human neurons.** A human neuron inside a composed instance asks as `box/who`; grants name only the outer
  network's human neurons, so such an item is answerable by the owner alone.
- **Identity.** A token is a bearer secret on 127.0.0.1. An org's directory, sign-in or a hosted desk is later;
  the grant file is the seam it would fill.
- **Weights.** Every grant is equal; a vote's weight waits on link strength (SPEC Appendix B).

## How it is checked

`rust-engine/tests/desk.rs`: per-person tokens and the refusal of an ungranted item (403, the stderr line, the item
still waiting), `answerable_by` against roles with the owner answering anything, `choose` sending the option's text
with a bad or missing option refused, and four malformed grants files refused before the desk opens. The team demo's
proxy can now be retired for the desk itself (agent-neuron-demos).
