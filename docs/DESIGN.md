# Design: Sybershoke for the Jev/Kev decision path

## First principles

Sybershoke asks one question of a system: after it is broken on purpose, was anything lost,
accepted twice, or answered wrongly? Everything else serves that question.

Jev and Kev are typed decision models. They choose from a menu and code validates the choice. So
the useful faults are the ones that hit the code around the model, not the model's weights:
time (a deadline, a cold start), failure (an error status, a truncated answer), and the rules that
rewrite or reuse an answer (a keyword override, a cache).

A model never grades correctness. Deterministic checkers do.

## Three integration points

1. **Jev/Kev as the system under test.** A fault proxy sits between the Worker and the model
   host. It injects error statuses, slowness, cold starts, truncated answers and out-of-menu
   answers. This is what `shoke-jev` models today.
2. **Jev/Kev as the Planner and Verifier inside Fanout.** Kill a worker mid-run with a Jev plan and
   again with a Kev plan, and check nothing is lost or accepted twice (I6). Not built.
3. **Kev as a fault scheduler.** A stretch goal that earns its place only if it finds bugs faster
   than the seeded random baseline. If it does not, delete it. Not built.

## History format

`shoke-history/v1`: one event per line, plain text, no dependencies.

```text
shoke-history/v1
# comments and blank lines are ignored
meta deadline_ms=8000
meta max_calls=2
500 req id=r1 text=tokyo%20drift
503 call req=r1 n=1
1300 resp req=r1 n=1 status=ok
1300 decision req=r1 source=model wpm=300 sound=3 visual=3 book=4
```

- Line one is the header. `meta key=value` lines carry configuration and may repeat.
- An event is `<time_ms> <kind> key=value ...`. Kinds and keys use `A-Za-z0-9_.-`.
- Values are percent-encoded: every byte except letters, digits and `- . _ ~ : , /` becomes `%XX`.
- Invariants read `deadline_ms` and `max_calls` from `meta`, so a file is self-describing. A value
  that does not parse, or a key repeated with a different value, is an input error. `shoke check`
  prints the bar it applied.
- Line order does not matter: checking sorts events by time, and a same-time tie keeps file order.
- A history starts with an empty cache: a cache hit needs an earlier model answer for its key.

Events the Jev/Kev invariants read:

| Kind | Fields | Meaning |
|------|--------|---------|
| `req` | `id`, `text` | A reader's request arrived. |
| `call` | `req`, `n` | A provider call started. |
| `resp` | `req`, `n`, `status` | It ended: `ok`, `http_429`, `http_503`, `http_500`, `truncated`, `out_of_menu`, `timeout`. |
| `decision` | `req`, `source`, `key`?, `origin`?, `wpm`, `sound`, `visual`, `book` | A plan was admitted. `source` is `model`, `cache` or `fallback`. `key` is the system's own cache key when it reports one; otherwise I3 uses the normalized request text. `origin` is informational: I3 never trusts it. |
| `error` | `req`, `reason`, `visible`, `preserved` | The request ended without a plan. |

## Why the checks are shaped this way

- **I4 uses rule verifiers as its oracle.** "Fast means at least 250 wpm, silence means silent
  audio, no visuals means visuals off" are the most trusted labels in the post-training report.
  Conflicting words ("fast" and "slow", "loud" and "quiet") produce no expectation rather than a
  wrong one.
- **The model and I4 share those rules, so I4 cannot grade the rules themselves.** A hand-labelled
  phrase table (`crates/shoke-jev/golden/phrases.tsv`) is the independent oracle. The checker may
  never demand more than it, and the correct pipeline must satisfy it.
- **I7 is a policy, not a defect.** Without a preset floor a failed request ends in a visible error,
  which is correct behaviour under I2. I7 asks for more, and is switched on with `--require-floor`.
- **The model is deliberately correct.** The stand-in for Jev or Kev honours every explicit word,
  so every violation comes from the pipeline around it.
- **A seed is a bug report.** Traffic, fault windows and per-call latencies each draw from their
  own stream, so removing a fault never changes the others. That is what makes shrinking sound.
- **Shrinking is time-aware.** After removing faults it trims each window from either end, so it
  can close in on the call that needs it, and moves windows earlier. It keeps the invariant failing,
  not necessarily the same request.

## Phases

The phases follow the Fanout rule that each must pass its exit test before the next begins.

| Phase | Build | Exit test | State |
|-------|-------|-----------|-------|
| 1 | Core, history format, checkers | I1 to I5 each catch a hand-written violation | Done: `crates/shoke-jev/golden` |
| 2 | Fault proxy for the RISE Worker, against recorded fixtures | Finds the keyword misfire and the missing fallback with a replayable seed | Modelled; the Worker adapter is not built |
| 3 | Kev adapter | Zero-shot Kev against Jev on the 39-case eval, under faults | Not started |
| 4 | Fanout integration | Kill a worker under both planners; zero lost, zero double-accepted | Not started |
| 5 | Report and write-up | Every published number reproduced by a script | The report exists for the model only |

## Cautions

- TypeSafe's terms bar using Jev output to train a model. Recorded Jev answers are acceptable as
  test fixtures, never as training labels.
- Any Kev result is an unmeasured prior until phase 3 runs. The post-training report records that
  no Kev capture on the real request exists.
- Tell maintainers before publishing a score for their system (roadmap rule 5).
