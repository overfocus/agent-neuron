# Demos

Every visible behaviour of agent-neuron has a demo here, so the ideas can be
watched and argued with, not just read. Keep this table current: a feature
that adds or changes visible behaviour adds or updates its demo and its row.

| Demo | Command | What it shows | Status | Open questions it informs |
|---|---|---|---|---|
| First example | `echo approve \| npm run example` | A shell script, a local HTTP API and a human on stdin as three neurons in one network; a waiting `send` between them. | working | Who decides where an answer goes next; what a failed neuron means for the run. |
| Typing | `npm run demo:typing` | A search box emits the query one letter at a time without waiting (`emit`). Three neurons with different latencies each decide in their own adapter whether to act. A newer prefix replaces the older one: stale work is aborted and printed as dropped. Answers carry no prefix: the demo keeps a map of what it emitted and finds each answer's prefix through `replyTo`. Made-up catalogue. | working | What a passive channel is, as opposed to an active one; whether the wrapper or the network should decide to act on an input; which answers count as useful. |
| Typing, tracks in Rust | `npm run demo:rust` | "Wrap anything" across languages: the typing demo with its tracks neuron replaced by a small Rust program (`rust/catalogue`, std only) run through the shell adapter. Same rule and catalogue as the JavaScript neuron, so the answers agree; the neuron id `tracks (rust)` shows where they come from, and stale work is still aborted and printed as dropped. Needs a Rust toolchain. | working | Whether the wrapper really is language-neutral; what crossing a process boundary costs (a spawn per signal); where a composed neuron's inner network could run in another language. |
| Real-time search page | not built yet | The same music search in a browser, with the concerts and links neurons, results a person can click. Issue #11. | planned | Whether a click is a signal that strengthens a connection; chains inside the network (concerts listens to artist). |
