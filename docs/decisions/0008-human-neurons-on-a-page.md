# Decision record 0008: `human` neurons on a page (2026-11-05)

**Status: decided; steps 1 to 3 done, step 4 decided as a middle path and built (2026-11-05).** Kevin answered the seven questions below on 2026-11-05, each as
suggested. Step 1 of the order of work is in: `src/engine/page.ts`, the `desk` option of `runNetwork`, the day-long
ceiling, SPEC Appendix D and `npm run demo:human`. Step 2 is in too: the scripted person (CONFORMANCE 5.5,
`VirtualHost.desk()`), 8 fixtures in `spec/fixtures/human/`, both engines at 201 of 201, and 3 more mutants. Step 3 ran the network
beside the hand-built pipeline on a real file of 37 passages ("The side-by-side", below). Step 4 is not to retire the
hand-built review yet but to batch the network's agents ("Step 4: the middle path").

## The problem

The most common reason people give for wiring agents together is a person in the loop: an agent proposes, a person
approves, edits or refuses, and only what was approved goes on. The specification already has the neuron for it,
`human` (SPEC 3.4: the engine shows the input to a person and takes a typed reply; empty is quiet; cancel and
`timeout` stop the wait), and the library has a `HumanAdapter` on stdin. But the reference engine does not run
`human` neurons from a network file, and a terminal prompt is not where anyone reviews ten paragraphs of prose.

There is already a working example of the pattern outside this repo: the maintainer's *voice review*, a local page on
which he marks each rewritten passage of a draft ok, no, or his own text, after which an agent applies only what he
approved. It is a hand-built pipeline: extract, a writer, an invariant check, two independent reviewers, one retry,
the person, then apply. That is a network of neurons with one `human` in it, written without the engine.

## The proposal

**The reference engine runs `human` neurons by serving one local page per run.** Each call to a `human` neuron becomes
an item on the page; the person's answer becomes the neuron's reply, exactly as SPEC 3.4 already says: empty is quiet,
anything else is a text payload. No change to the specification is needed, because 3.4 leaves "the interface" to the
engine. The engine, the trace and the routing rules see a `human` neuron as they see any other.

### What the page offers for one item

| Answer | The reply | Read as |
|---|---|---|
| **Approve** | the proposed text, unchanged | "go on with this" |
| **Edit** | the person's text | "go on with mine" |
| **No** | empty | quiet: nothing goes on (SPEC 3.4) |

The payload says what to show. A payload with `draft` and `proposal` text fields is shown as a diff of the two, with
Approve sending `proposal`; any other payload is shown as its text or its JSON. This is a convention of the page, not
of the specification: a neuron that sends something else still works, it is just shown plainly.

### How it fits rules the engine already has

- **Staleness** (`replaces`, SPEC 5): a newer input with the same key cuts a waiting `human` call like any other, so the
  item is marked *withdrawn: a newer version arrived* and its buttons go. Nobody approves an outdated draft.
- **`timeout`**: the wait stops; the item is marked *timed out* and the call ends as the spec says.
- **Quorum** (`threshold`, SPEC 5.2): "needs two approvals" is two `human` neurons linked to one neuron with
  `threshold: 2`. The engine already counts distinct senders. It is a quorum of named reviewers, not authorisation:
  who may answer is not modelled (see "Not in this step").
- **`effect`** (SPEC 11): the neuron after the person, the one that writes the approved text into a file, is an effect.
  It is never cut by staleness once started, and a crash is `unknown`, not a failure safe to retry.
- **The trace**: each answer is an `answer` event from the `human` neuron, as any reply. The page adds nothing to the
  trace of its own.

### The voice review as the first user

The voice pipeline becomes a network file. Invented names, the real shape:

```json
{
  "version": 0,
  "name": "voice",
  "neurons": [
    { "id": "passages", "source": true, "description": "Each drafted passage, keyed by its id (replaces)." },
    { "id": "writer", "run": ["node", "neurons/writer.mjs"], "description": "Rewrites one passage in the house voice." },
    { "id": "invariants", "run": ["node", "neurons/invariants.mjs"], "description": "Same cites, links, numbers and quotes as the draft; quiet if not." },
    { "id": "voice-check", "run": ["node", "neurons/reviewer.mjs", "voice"], "description": "Scores the rewrite against the voice guide." },
    { "id": "fact-check", "run": ["node", "neurons/reviewer.mjs", "facts"], "description": "Same claims as the draft, nothing added or dropped." },
    { "id": "ready", "run": ["node", "neurons/ready.mjs"], "threshold": 2, "within": 600000, "description": "Both reviewers passed: sends { draft, proposal }." },
    { "id": "maintainer", "human": true, "timeout": 3600000, "description": "Approve, edit or no." },
    { "id": "apply", "run": ["node", "neurons/apply.mjs"], "effect": true, "description": "Writes the approved text into the data file." }
  ],
  "links": [
    { "from": "passages", "to": "writer" },
    { "from": "writer", "to": "invariants" },
    { "from": "invariants", "to": "voice-check" },
    { "from": "invariants", "to": "fact-check" },
    { "from": "voice-check", "to": "ready" },
    { "from": "fact-check", "to": "ready" },
    { "from": "ready", "to": "maintainer" },
    { "from": "maintainer", "to": "apply" }
  ]
}
```

The file passes `npm run engine -- check`. Its `timeout` is today's ceiling, one hour; decision 7 raises it to a day for `human` neurons.

(The retry is left out of the sketch: it is a link back to `writer` bounded by `maxHops`, or the reviewers' notes in
the payload. Which one is a question for the voice pipeline, not for the engine.)

What changes for the person: one page that fills in as passages clear their checks, instead of a proposal file that
appears when every passage is done; and an edit to one passage while another is still being written. What stays: the
writer and reviewer programs, the voice guide, and the rule that nothing reaches a data file without the person.

### The page

The rules of the local pages that already exist (the voice review, and rigswig's page): bound to 127.0.0.1 only; a
random 128-bit token compared in constant time; an exact `Host` (no DNS rebinding) and `Origin`; answers by `POST`
with the token in a header and a JSON body under a size cap; a nonce CSP with no outside request of any kind; every
value from a payload set as text, never as HTML. The look can be shared with the other two pages as a template file
(the three are in TypeScript, JavaScript and Rust, so a shared library would not fit).

## Kevin's decisions (2026-11-05)

| # | Question | Decision |
|---|---|---|
| 1 | Does the reference engine run `human` neurons at all, or does it stay a library adapter? | **Yes, the engine runs them.** It is the most-asked-for neuron, and SPEC 3.4 already allows it. |
| 2 | One page per run, or one page per engine process serving several runs? | **One page per run.** The page lives as long as the run, and nothing outlives what the trace records. |
| 3 | The three answers (Approve, Edit, No), or also a free reply that is neither? | **Three.** Edit covers a free reply; a fourth button would be a fourth meaning to learn. |
| 4 | The `draft`/`proposal` diff convention, documented as non-normative (an appendix), or kept as a page detail? | **A non-normative appendix**, so another engine's page shows the same thing. |
| 5 | Does the voice review move onto it, retiring the hand-built pipeline once the network matches it? | **Yes**, in the order below, keeping the old one until the new one has reviewed a real proposal. |
| 6 | The page's look: kevinferron.com's (as rigswig's page), or agent-neuron's own? | **kevinferron.com's**, as rigswig's page and the voice review: black, neon green and cyan, Josefin Sans and Pixelify Sans (SIL OFL, served from the page's own origin), the kevinferron.com wordmark. |
| 7 | The schema caps `timeout` at one hour, which is short for a person. A longer ceiling for `human`? | **Yes: a day (86,400,000 ms) for `human` neurons only**; other kinds keep the hour. A schema change, so a SPEC row and fixtures go with it. |

## The order of work

1. **The engine runs `human` neurons through the page** (`src/adapters/` gains the page; the engine stops refusing
   `human`), with the day-long `timeout` ceiling for `human` (schema, SPEC row, fixtures) and the diff convention as a
   non-normative appendix. The library's stdin adapter stays.
2. **Conformance**: a scripted person in the conformance runner (as the scripted observer, decision 0007 row 16) that
   answers by rule, so approve, edit, no, withdrawn-by-staleness and timeout are fixtured like everything else. The
   runner refuses `human` today; it stops refusing it.
3. **The voice network** in the kit, its neurons wrapping the programs it already has, run against a real proposal next
   to the old pipeline, compared passage by passage.
4. **Retire** the hand-built voice review once the network has done a real proposal as well.

## Not in this step

- **A durable hold** (decision 0007 row 10's open problem): a wait that survives a restart, blocks its key and has an
  owner. Here the wait lives as long as the run; if the engine stops, the question is gone and the item with it.
- **Who may answer.** The page has one person: whoever holds the token. Several people, identities and grants are the
  protocol question of a harness talking to other harnesses, not this one.
- **Notifications.** The page does not tell anyone an item is waiting.

## What it would cost

One normative change to the specification, decision 7's ceiling for `human`, and a non-normative appendix (decision 4). In the reference engine, the
page server and the adapter that feeds it (by the voice review's size, about 250 lines with the page), and the end of
the refusal of `human`. Conformance gains a scripted person and fixtures for the five outcomes above. A network file
without a `human` neuron behaves as it did, and so does the stdin adapter.

## What step 3 found (2026-11-05)

The voice network is in the kit (`plugins/loop/scripts/voice-net/`, `voice net`): the sketch above, with `retry` as a
second threshold-2 join that sends a failing round-1 rewrite back to the writer once (the writer's `maxHops` refuses a
third round), and the writer and both reviewers wrapping the kit's existing agents, run headless one passage at a time.
On two passages of a real draft it worked end to end: each reached the page with its checks as notes. Four findings.

1. **A person's answer does not say which question it answers.** SPEC 3.4 makes the reply a text payload, so the
   neuron after a `human` neuron gets the text alone. In the sketch, `apply` would not know which passage to write.
   The kit's network ends at the person instead, and its host keeps each question with its answer through the desk
   (the library's `desk` option), writing them into the proposal that the existing `voice apply` applies. A join
   (`threshold: 2` on the question and the answer) would keep it inside the network, but `within` is capped at an hour,
   shorter than a person may take. Whether a `human` reply should carry its question (a field beside `text`), or
   `within` should follow `human`'s day, is a decision for the specification, not taken here.
2. **The page drew nothing.** A name used before its declaration in the page script (step 1, from a late change to
   the counts) threw on every poll, and the poll loop swallowed it: the questions were on the API and no card ever
   showed. Fixed, and `test/page.test.ts` now draws the page in headless Chrome, so the fix is tested.
3. **The page needed to say what each question is about.** Appendix D gains `title` and `notes` beside the pair:
   shown, never sent.
4. **Cost.** One agent call per passage per step, about 1.2 dollars a passage for one round (the CLI's figure), where
   the skill makes one call per step for the whole file. A file of 37 passages is roughly 45 to 55 dollars with
   retries. The page's gain (answering each passage as it clears) is paid for there.

## The side-by-side (2026-11-05)

Both on the same draft, 37 passages, each with its own writer (so only one rewrite matched, a passage that is a lone
template token). Kevin answered the network's page: 35 approved; 2 never reached it (a slot race in the kit's neuron
code, since fixed). The hand-built review's proposal is unmarked.

| | Hand-built | Network, one agent call per passage |
|---|---|---|
| ready / look / flagged | 13 / 22 / 2 | 0 / 33 / 4 |
| retried after review | 15 | 5 |
| agent calls | 7 | 124, 46 dollars as the CLI reports it |

Their flags did not overlap. The reviewers, shown one passage at a time, left a note on every pass (none came out
"ready") and were lenient on rule 15 (a rewrite that changed too little), which the batched reviewer had failed 12
times. The page worked: Kevin answered as passages arrived.

## Step 4: the middle path (Kevin, 2026-11-05)

Not retire the hand-built review, and not keep one agent call per passage, but batch. SPEC 3.2 gives a call one answer,
so a batch call cannot fan out into page items; instead the writer and the reviewers are level-1 neurons that gather
the calls arriving together and give them to one agent call, then answer each call on its own. Each passage stays its
own trace, key, join and page item; a file costs about one agent call per step, as the hand-built review does. On three
real passages it reached the page after one call per step. Whether it replaces the hand-built review is the next real
file's question: a batched run of a whole file, answered on the page, compared as above.
