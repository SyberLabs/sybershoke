# Sybershoke

Shock a system, then check what happened.

Sybershoke injects faults into a system, records what it did as a plain-text **history**, and
checks the history against **invariants**. This repository holds the harness and its first
target: the Jev/Kev decision path that RISE uses to turn a reader's request into a reading plan.

**Status: Research.** Nothing here has been run against a real Jev, Kev or the RISE Worker.
The target is a model built from the project documents. See [Scope](#scope).

## Try it

```sh
cargo run --release -p shoke-cli -- bugs                       # the six seeded bugs
cargo run --release -p shoke-cli -- run --seed 1 --faults 0    # a clean run: PASS
cargo run --release -p shoke-cli -- run --seed 1 --faults 0 \
    --bug bare-keyword-override --out bug.hist                 # "drift off to sleep" gets 300 wpm
cargo run --release -p shoke-cli -- check bug.hist             # same verdict, from the file alone
cargo run --release -p shoke-cli -- sweep --bug retry-storm --mix http --shrink
cargo run --release -p shoke-cli -- sweep --profile kev-cold --require-floor
```

`scripts/report.sh` regenerates [REPORT.md](REPORT.md), the full campaign. `--check` fails when the
committed copy is stale. Same seed, same result, on every machine.

## What it checks

| Id | Invariant |
|----|-----------|
| I1 | Every plan admitted to the reader is on the menu. |
| I2 | Every request ends exactly once, within the deadline, and a failure keeps the reader's text. |
| I3 | The decision cache never serves a degraded or a different plan. |
| I4 | Explicit words are honoured: "slow" is not fast, "silent" is not loud, "no visuals" is off. |
| I5 | No request makes more provider calls than the cap. |
| I7 | (policy, opt-in) Every request is answered with a plan even when the provider fails. |

I6 (nothing lost, nothing accepted twice) belongs to the Fanout target. See the
[roadmap](docs/DESIGN.md#phases).

## How it fits together

```
shoke-core   seeded RNG, history format, invariant trait, shrinker          (no dependencies)
shoke-jev    virtual-time model of the Jev/Kev path, fault proxy, I1-I7
shoke-cli    the `shoke` binary: run, sweep, shrink, check, report, bugs
```

Running is separate from checking. A run writes a `shoke-history/v1` file; invariants read only
that file. So a real system can be checked without linking any Rust: write its events in the
format ([spec](docs/DESIGN.md#history-format)) and run `shoke check FILE`.

## Scope

- **The target is a model.** It reproduces structure stated in the project documents: an 8 second
  deadline, a Kev scale-to-zero cold start of about 35 seconds, a keyword override that forces the
  night-drive look, a decision cache, a menu that answers are validated against. It does not
  call a model, and it is not the RISE source.
- **Findings are about that structure.** Whether the real Worker has the same failure needs the
  phase 2 adapter, run against recorded fixtures.
- **Two constants are assumptions**, not measurements: how long a scale-to-zero host idles before
  it goes cold (120 s) and the traffic pattern. Both are stated in the report.
- **Not built yet:** the RISE Worker adapter, a Kev adapter, the Fanout target (I6), the static
  trace page.

## Relationship to the Week 0 scaffold

The Week 0 workspace (`shoke-core`, `shoke-sim`, `shoke`, the acceptance-key simulator) was built
in an earlier session and was not available when this code was written. This workspace has its
own small core with the same crate names and the `shoke-history/v1` format from the Week 1 plan.
Before merging, reconcile the two `shoke-core` crates: the history format and the invariant trait
are the parts that must agree. The Fanout target and I6 stay in the Week 0 simulator.

## Open decisions

From the roadmap, still open and not decided here: **name clearance** (others already use
"sybershoke"), the **license** (none chosen, so this code is all rights reserved), the repository
home, and the disclosure window before any third-party score is published.
