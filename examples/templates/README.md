# Templates: one private template repo per business type

The job [decision 0022](../../docs/decisions/0022-what-a-swarm-needs.md) was written for, as one
network file. A list of business types goes in; each type gets a private GitHub repo generated from a
starter and marked as a template itself.

```
list ──> dispatch ──each──> template        (all.network.json)
                              └─ plan ──> create ──> verify     (template.network.json, one neuron outside)
```

- **`dispatch`** answers the list as an array, and `each` forwards one signal per type.
- **`template`** is the one-type network as a composed neuron. It states `effect` because `create`
  inside is one, takes four calls at once (`maxInflight`), and starts a failed type again twice
  (`retry`), which is safe because `create` is idempotent by the repo's name.
- **`create`** does nothing unless it is given `--apply`. `template.network.json` leaves it out, so
  `all.network.json` is a dry run; `template.apply.network.json` adds it with `args`, and
  `all.apply.network.json` runs over that file. The two pairs differ in that one argument and the
  file name that follows from it: a composed file takes no parameter.

## Settings

Each neuron reads its own `.env`, beside its file (git-ignored; `env.example` shows the names):
`neurons/plan/.env` has the owner and the starter repo, `neurons/create/.env` a token that may create
private repos, `neurons/verify/.env` one that may read them. The neurons call GitHub's API themselves.

## Running it

```sh
E=rust-engine/target/release/neuron-engine
$E run examples/templates/all.network.json       --inputs examples/templates/all.inputs.jsonl --result -   # dry: what it would do
$E run examples/templates/all.apply.network.json --inputs examples/templates/all.inputs.jsonl --result -   # creates the repos
```

The last line is the run's result: one answer per type from `template`, each failure, and the exit
status is 3 if a type failed for good. A failed attempt that `retry` started again is listed with
`"retried": true` and does not fail the run. Running it again changes nothing: every repo that is
already right answers `"action": "none"`.

The budget is 30 `repos` for the list, one per attempt, so a long list with many retries can run out:
the types after that are refused with code `budget`.
