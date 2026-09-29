# Red team: Sybershoke for the Jev/Kev decision path

Phase 1 of the review: an attempt to prove the work wrong before anyone builds on it. Every
finding below was reproduced against the unmodified `sybershoke.zip` (SHA-256 prefix
`33d2711116dd579c`; 76 tests passing). Nothing in this section was fixed when it was written.
Phase 2 status is at the end.

## What the work claims, and what would have to be true

| Claim | Must be true for it to hold | Verdict |
|---|---|---|
| The checkers catch six seeded bugs | Each checker detects the *defect*, not a label the defect happens to write | **Partly false.** I3's catch is the system confessing (R1). |
| A correct pipeline passes | The checker's idea of "correct" is independent of the model's | **True only by construction.** Same function on both sides (R4); the "correct" pipeline fails on phrasings outside the workload (R5). |
| A seed reproduces exactly | No hash-order, clock or platform input reaches the output | **True for `run`, `sweep` and `report`.** `check` output order is random (R10). |
| The shrinker minimizes | Fault windows can shrink around the call that needs them | **False for time.** Windows keep their random length (R6). |
| Findings describe RISE's structure | The modelled structure matches the RISE Worker | **Partly false.** Several decisive facts differ from the real code (R2). |

## Inputs I could and could not check

- **The six project documents were not supplied.** They are not in the zip, the RISE repository, any
  RISE branch, or the connected Google Drive. Where this review needs a fact from them, it checks the
  fact against the **real RISE Worker source** instead (`worker/jev-recommend.mjs` at RISE commit
  `082b3fa`, 2026-09-29), which is the stronger ground truth.
- **The Week 0 scaffold (shoke-sim, I6) was not available.** This workspace is the base.
- **CI has never run on GitHub.** I ran the same four steps locally on a fresh copy (Phase 2).

## Findings, ranked by how badly they would mislead a reader of REPORT.md

Severity: **Critical** changes what a headline number means; **High** makes a stated result
wrong or unsupported; **Medium** gives a wrong PASS on some input; **Low** is cosmetic or a
reproducibility nit. Evidence commands run from the repository root; `redteam/` holds the probe
crate and inputs (`cargo run --release --manifest-path redteam/Cargo.toml -- MODE`).

### R1. Critical: I3 only catches a cached fallback when the cache says it was a fallback

- **Evidence.** `redteam` mode `lying`: the 200 `cache-fallback` histories from the report,
  unchanged except that `origin=fallback` becomes `origin=model`, which is what a real cache
  records. The bug's own description says the fallback is stored "as if the model had produced it".
  I3 goes from **25/200 to 0/200**. `shoke check redteam/histories/lying_origin.hist` gives PASS.
- **Why.** I3 flags `origin != model` and otherwise compares a hit only with an *earlier* model
  plan for the key. The first failure for a key has no earlier model plan, so a mislabelled hit
  passes.
- **Also missed.** A cache hit served before any model answer exists (`race.hist`, PASS).
- **Refuted.** Overwrite (model A, then model B, then the cache serves A) is caught (`overwrite.hist`).
- **Smallest fix.** Stop trusting `origin`. A cache hit must equal the most recent `source=model`
  decision for its key *before it in time*; a hit with no such decision is a violation. Make the
  seeded bug label its entry `model`, as its description says.

### R2. Critical: the model differs from the RISE Worker on facts that decide the findings

Read against `worker/jev-recommend.mjs` @ `082b3fa`:

| Model | Real Worker | Findings affected |
|---|---|---|
| Up to 2 provider calls per request, 250 ms backoff (`max_calls=2`) | **Exactly one call.** Any failure returns 502/504 | `retry-storm` and I5 have no counterpart today |
| 8000 ms deadline from request arrival, covering retries | `AbortSignal.timeout(8000)` on the **provider fetch only**; Redis (2.5 s per call) and Neon run before it | I2 timing; real request latency can exceed 8 s with no violation of the Worker's own rule |
| Cache key is `normalize(text)` (case, spacing, punctuation folded) | HMAC of the **trimmed intent verbatim** plus catalog, menu, provider, revision, and a **per-turn variation cohort**; 1 h TTL | I3 on a real history would compare plans the real cache keeps apart, a false positive |
| Preset floor exists as an option | **No fallback.** Confirmed | `cache-fallback` cannot occur today; I7 finding confirmed |
| Night-drive misfire modelled as bare words | Confirmed: regex matches `drift`, `racing`, `tokyo`, `neon`, `highway`, `synthwave`, `outrun`; fires only for `schemaVersion: 3` clients; raises wpm below 250 to 300 | `bare-keyword-override` confirmed in code |
| Out-of-menu answers rejected | Confirmed (`validDecision`) | `no-validation` models a real safeguard |

- **Unverified.** Kev's ~35 s scale-to-zero cold start and the scale-to-zero deployment itself appear
  nowhere in the RISE repository. Sensitivity (R3) shows the only thing that matters is whether
  the cold start exceeds 8 s.
- **Smallest fix.** Say this in README and REPORT. The phase 2 adapter must record the system's own
  cache key; I3 must use it when present.

### R3. High: every seed count is set by assumed traffic and fault density, not only the kev-cold row

REPORT.md warns that the kev-cold I7 count comes from the assumed traffic. The same is true of
every count in tables 1 and 3. Each row changes one constant; 200 seeds, 60 requests, 2 faults,
as in the report (`redteam/sensitivity.sh`):

| Assumption changed | kev-cold I7 | jev I7 | retry-storm I5 | cache-fallback I3 | no-validation I1 | no-deadline I2 | lose-request I2 |
|---|---:|---:|---:|---:|---:|---:|---:|
| baseline | 199 | 49 | 25 | 25 | 10 | 28 | 49 |
| idle threshold 120 → 240 s | 87 | 49 | 25 | 25 | 10 | 28 | 49 |
| idle threshold 120 s → 15 min | 50 | 49 | 25 | 25 | 10 | 28 | 49 |
| long gap 1 in 12 → 1 in 1000 | 190 | 184 | 129 | 130 | 53 | 110 | 184 |
| long gap 130–200 → 30–60 s | 115 | 115 | 75 | 56 | 22 | 51 | 115 |
| fault windows 1–16 → 0.25–2 s | 199 | 12 | 4 | 6 | 4 | 9 | 12 |
| Jev latency 0.6–2.5 → 2–7 s | 199 | 56 | 15 | 26 | 10 | 50 | 56 |
| repeats 1 in 4 → 1 in 20 | 200 | 55 | 38 | 24 | 11 | 19 | 55 |
| cold start 35 → 20 s | 199 | 49 | 25 | 25 | 10 | 28 | 49 |
| retry cap 40 → 3 | 199 | 49 | 25 | 25 | 10 | 28 | 49 |
| books 15 → 31 | 199 | 49 | 25 | 25 | 10 | 28 | 49 |

- Long idle gaps stretch the fault horizon, so fewer faults land on requests. Counts move 3–8×.
- **Numbers that read as findings but are assumptions.** Every non-zero count in tables 1 and 3,
  and the first-failing seeds. What survives: a bug is caught (>0) or not (0), and both `with floor`
  rows are 0.
- **Insensitive.** The cold-start value (any value over 8 s), the retry cap (any value over 2) and the
  catalogue size (15 or 31) do not change any count.
- **Smallest fix.** REPORT says in one place that counts are not rates and lists every assumption.
  Commit the sensitivity script so the table obeys roadmap rule 1.

### R4. High: "a correct model passes I4" is true by construction

- **Why.** `text::expectations` is both the oracle (I4) and the model (`apply_expectations`). Whatever
  it misreads, the model does, and I4 approves.
- **Independent oracle, part 1: the real Worker's own intent regexes** (`requestsNoSound`, the
  no-visual test), run by `redteam/worker-oracle.mjs`.
  - They **agree on all 180 distinct workload texts**, so inside the workload the circularity does
    no harm for sound and visuals.
  - Outside it (`redteam/phrases.txt`), the oracle has **no rule** for 10 phrasings the Worker treats
    as binding: "no sounds", "no music", "without audio", "sound off", "muted", "visuals off",
    "turn visuals off", "dark screen", "reading only", "no animation". A pipeline that ignores them
    passes I4.
- **Independent oracle, part 2: hand labels.**
  - The oracle **demands the wrong thing** for "not too fast" (≥ 250 wpm: negation looks back one
    word), "nothing fast" (≥ 250) and "fast asleep" / "quick nap before bed" (≥ 250).
  - The "correct" model answers "not too fast" at **300 wpm and passes**.
  - In the workload, "fast and loud" has **no loudness rule** (serving silence passes).
  - "fast and loud, silent" demands silence, although pace contradictions produce no rule. The
    policy is inconsistent.
- **Smallest fix.**
  - Make negation skip intensifiers ("too", "very", "so") and add "nothing".
  - Read "asleep", "nap" and "bed" as sleep words, so "quick nap" is a conflict, not a fast request.
  - Add the Worker's no-sound and no-visual phrasings.
  - Add "loud" as a loudness floor, with conflicts producing no rule, as pace does.
  - Pin all of it with a hand-labelled phrase table that the checker does not generate.

### R5. High: the "correct" pipeline fails I4, but only on words the workload never uses

- **Evidence.** `redteam` mode `adv`: one request, floor on, no bugs, no faults.
  - "tokyo drift, just read", "night drive text only": visual 3, and the oracle wants visuals off.
  - "night drive, mute": synth, and the oracle wants silence.
  - "tokyo drift, soft", "hushed night drive": synth, and the oracle wants rain or quieter.
- **Why.** The Narrow override's block list omits `mute`, `soft`, `hushed`, `just read` and `text only`,
  which `expectations` treats as binding.
- **Inside the workload: no false positive found.** Configurations searched: 3 profiles × requests
  {1, 5, 60, 400} × faults {0, 2, 10, 60} × 6 mixes × 2000 seeds each, floor on, I7 required.
  That is 576,000 runs with **zero violations**
  (`redteam/search.sh`). The claim holds inside the workload's 180 phrasings and fails just outside them.
- **Smallest fix.** The narrow override must not fire when the request carries any explicit rule
  other than a fast-pace rule.

### R6. High: the shrinker's "smallest reproduction" is not the smallest, and cannot tighten time

- **Evidence.** `redteam` modes `shrink` and `tight`.
  - REPORT row `no-validation` gives "out-of-menu answer for 14978 ms". That is the fault's original
    random length, unshortened. A 250 ms window at `t=220736` still fails I1.
  - `no-deadline`: 4381 ms is reported; 250 ms at `t=391800` fails.
  - Seed 4 `retry-storm --mix http` stalls at 4607 ms; 250 ms at `t=557165` fails.
- **Why.** `JevFault::simpler` only cuts a window from the right and halves `at`, which moves the window
  *earlier*. When the call that needs the fault starts late in the window, every candidate misses it.
  DESIGN's "moves the survivors" means "moves them earlier".
- **Also.** The shrunk scenario often fails a *different* request than the original (truncate,
  seed 4: r11–r15 before, r5 after). Sound for "this invariant still fails", but not "the same
  failure".
- **Refuted.** ddmin terminates and ends 1-minimal. A reduction lowers `n` to `max(n-1, 2)`; no
  reduction doubles it up to `len`, then stops. The only unbudgeted predicate calls are the first two.
- **Smallest fix.** Also offer windows trimmed from the left (keep the end, halve from the start),
  which still strictly lowers `dur`. Say in the report that shrinking keeps the invariant, not the request.

### R7. Medium: `shoke check` gives a wrong PASS on malformed histories

`shoke check redteam/histories/FILE`:

| File | What it is | Verdict | Should be |
|---|---|---|---|
| `unsorted_cache.hist` | Same events as `sorted_cache.hist`, stale hit listed first | PASS (sorted copy: I3 FAIL) | FAIL: order must not matter |
| `before_request.hist` | Answer 41 s *before* its request | PASS (`saturating_sub` gives 0 ms) | FAIL |
| `dup_id.hist` | Two requests `r1`, one answer | PASS | FAIL |
| `dup_id_i4.hist` | "drift off to sleep" answered at 300 wpm; a later `r1` says "fast" | PASS: I4 read the other text | FAIL |
| header only | No events at all | PASS, exit 0 | Input error: nothing to check |
| `bad_deadline.hist` | `meta deadline_ms=8s` | Silently uses 8000 | Input error |
| `dup_meta.hist` | `deadline_ms` twice, 99999999 first | First wins: 60 s answer passes | Input error |
| `orphan.hist` | Decision for a request that never arrived | Only I1 fires (plan invalid) | I2: unknown request |
| `noreq_calls.hist` | `call` with no `req` | I5 on subject `?` | I2: names no request |

- **Smallest fix.**
  - `check` sorts a stable time-ordered copy.
  - I2 flags answers before their request, duplicate request ids, and terminals or calls for unknown
    requests.
  - Malformed or conflicting `deadline_ms`/`max_calls` and zero-request histories are input errors
    (exit 2).
  - `check` prints the deadline and cap it used, since the file sets its own bar (`lax_deadline.hist`).
- **Refuted.** A 300,000-request, 45 MB history checks in 1.9 s. Non-UTF-8 input exits 2 cleanly.

### R8. Medium: CLI inputs that report a PASS for work never done

- `shoke sweep --bug bare-keyword-override --seed 18446744073709551615 --seeds 2` prints `0/2`, exit 0.
  `start + seeds` wraps in release, so no seed runs; a debug build panics. The same bug fails every
  seed it does run.
- `sweep --seeds 0` prints `0/0`, exit 0. `run --requests 0` gives PASS.
- `run 5 --seed 1` silently ignores `5`.
- **Refuted.** A bad `--out` path exits 2. `--faults 500 --requests 3` is fine.
- **Smallest fix.** Checked addition, and reject zero seeds or requests and stray positionals, all as
  usage errors (exit 2).

### R9. Medium: a request that arrives while Kev is cold-starting is served warm

- **Evidence.** `shoke run --profile kev-cold --faults 0 --seed 3 --requests 80 --out cold.hist`.
  - r39 calls at 380131 ms, meets the cold start and times out.
  - r40 calls the same booting host 1.6 s later and is answered in 356 ms.
- **Why.** Requests are simulated one at a time, and `last_call` records only when a call started.
  The host is "warm" as soon as any call has begun.
- **Effect.** It undercounts cold-start damage per seed. The seed count barely moves, because one cold
  request already fails the seed.
- **Smallest fix.** Track when the host becomes ready. A call before then waits for the remainder.

### R10. Low: `shoke check` output order is random

- **Evidence.** Six requests over the cap, all with the same last-call time
  (`redteam/histories/order.hist`). Twenty runs of `shoke check` gave **19 distinct outputs**.
- **Why.** I5 sorts a `HashMap` by time only. Ties fall back to per-process hash order.
- **Smallest fix.** Sort by `(time, request id)`.

### R11. Low: docs that do not match the code

- README "Two constants are assumptions". R3 lists at least seven that move results.
- DESIGN "shortens and moves the survivors": they only move earlier (R6).
- The minimum Rust version is undeclared. `Option::is_none_or` needs 1.82, and older toolchains fail
  with an unhelpful error.
- The CI workflow has never run on GitHub. Its steps match the local gates; its first real run is
  still untested.
- **Checked and true.** Every command in README "Try it" behaves as described. The DESIGN example
  history checks PASS. `scripts/report.sh --check` passes on the zip.

## Which findings depend on unverified facts

| Fact from the docs | Status against RISE `082b3fa` | Findings resting on it |
|---|---|---|
| 8 s deadline | Present, but per provider call, not per request | `no-deadline`, I2 timing, every `timeout` |
| ~35 s Kev cold start, scale-to-zero | **Not in the RISE repo; unverified.** Only ">8 s" matters (R3) | kev-cold rows, I7 kev-cold count |
| `requestsNightDrive` misfires on "drift off to sleep" | **Confirmed in code** (bare `drift`), for `schemaVersion: 3` clients | `bare-keyword-override` |
| No fallback decision | **Confirmed in code** | I7 rows; `cache-fallback` is hypothetical until a floor exists |
| Retries | **None in code** | `retry-storm`, I5 |
| 15 or 31 works | Irrelevant to every count (R3) | none |
| Idle threshold 120 s, 1-in-12 gaps of 130–200 s | Assumptions; nothing in RISE states them | every count (R3) |

## Not touched, by rule

Name clearance, license and repository home are the owner's decisions. During the review no
provider was called, nothing was published or pushed, and no maintainer was contacted.

## Phase 2: what was fixed

Each fix has a regression test that fails on the zip's code and passes now. Tests went from 76 to
89. Every number in this document is reproduced by `redteam/sensitivity.sh`, `redteam/search.sh` or
the `redteam` probe (`cargo run --release --manifest-path redteam/Cargo.toml -- MODE`) on the
current tree. The one exception is a "before" figure, which needs the zip.

| Finding | Fix | Regression test |
|---|---|---|
| R1 | I3 judges provenance from the plans: a hit must equal the latest earlier model answer for its key. The seeded bug no longer confesses. Probe `lying`: **0/200 → 25/200**. | `golden/i3_unlabelled_fallback.hist`, `i3_hit_before_answer.hist`; `sim::a_cached_fallback_is_labelled_as_a_model_answer` |
| R2 | Stated in README and REPORT. I3 uses a system's own cache key when a history records `key=`; the RISE adapter does. | Adapter results, `docs/ADAPTER-RISE.md` |
| R3 | REPORT says the counts are not rates and lists every assumption. The sensitivity script is committed. | `redteam/sensitivity.sh` reproduces the table above on the fixed tree |
| R4 | Negation skips intensifiers; sleep idioms; the Worker's no-sound and no-visual phrasings; "loud" is a floor, and loud against quiet is a conflict. The hand-labelled `golden/phrases.tsv` is the independent oracle. | `tests/phrases.rs` (3 tests); 1 named gap: "quick nap before bed" |
| R5 | The narrow override never overrules an explicit rule that asks for less. | `phrases::the_correct_pipeline_honours_the_hand_labels` |
| R6 | Windows shrink from either end. REPORT's reproductions: 14978 → 250 ms (`no-validation`), 4381 → 250 ms (the three I2/I3 rows), 14978 → 7489 ms (`retry-storm`, which needs several calls). | `faults::a_window_can_shrink_onto_its_last_moment`, `faults::every_candidate_lowers_the_measure` |
| R7 | `check` sorts by time. I2 rejects early answers, duplicate ids and orphans. Bad or conflicting limits and empty histories are input errors (exit 2). `check` prints its bar. | `checker::invariants_see_events_in_time_order_whatever_the_file_order`; 3 new I2 goldens and `i3_out_of_order.hist`; `invariants::unreadable_or_conflicting_limits_are_errors_not_defaults`; `cli::check_rejects_histories_it_cannot_judge_and_names_its_bar` |
| R8 | Zero seeds, zero requests, a seed range past 2^64−1 and stray arguments are usage errors. `sweep` always runs exactly the seeds it reports. | `cli::inputs_that_would_pass_vacuously_are_usage_errors` |
| R9 | The host records when a boot finishes; a call during the boot waits for it. r40 in the R9 example now times out. | `provider::a_call_during_a_cold_start_waits_for_it` |
| R10 | I5 sorts by (time, id). | `invariants::i5_output_order_is_the_same_every_time` |
| R11 | README and DESIGN corrected; `rust-version = "1.82"` declared. | Gates below |

**What did not move.** Every count in REPORT tables 1 and 3 is unchanged. The I3 count of 25 is
now earned from the plans, not from a label. `redteam/search.sh` on the fixed tree: 576,000
correct-pipeline runs, zero violations.

**Gates**, run on a fresh copy with no `target/`:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `scripts/report.sh --check`

## Still open

- **Kev (roadmap phase 3) is blocked.** No recorded Kev answers exist in RISE: the "local-hf" records
  are a MiniLM similarity baseline, not Kev. Recording Kev on the 39 cases means calling it, which
  is the owner's decision. The roadmap's order then holds phase 4 (Fanout, I6), whose inputs were
  not supplied either.
- **I3 on real histories cannot yet see a stale hit.** `--turns` now asks each intent up to 25
  times. The Worker does serve an older key's entry for the same text (70 times in 25 turns), but
  each intent replays one recorded answer, so the old and new plans are equal and I3 passes with
  or without the key. Catching a stale hit needs answers that differ by turn, which means recording
  them. The cohort case never arises: none of the 42 intents is open-ended
  (`docs/ADAPTER-RISE.md`, Q1-Q4).
- **The sound rank is read from RISE's descriptions, not owned by RISE.** It clears 3 of the 7
  clean-run I4 flags. The other 4 are "soft bossa" and "quiet mystery", whose descriptions say
  "light" and "sparse". Whether those are quiet is RISE's call (`docs/ADAPTER-RISE.md`, P8).
- **Resolved: the 8 s is per provider call.** RISE's `docs/KEV-DEPLOYMENT.md` calls it "the
  Worker's existing 8-second provider deadline". The adapter's histories now say
  `meta deadline_scope=call`, and no timeout fails I2 (`docs/ADAPTER-RISE.md`, Deadline scope).
- **CI has still never run on GitHub.** Its four steps pass locally on a fresh copy.
- **The six project documents were never supplied.** Every fact this review took from them was
  checked against the RISE source instead, or is listed as unverified above.
- The owner has since chosen the repository home (`SyberLabs/sybershoke`) and published it with no
  license, so the code is all rights reserved. Name clearance and the disclosure window remain the
  owner's decisions.
