#set document(title: "Flow incremental compilation performance")
#set page(paper: "a4", margin: 20mm)
#set text(size: 10pt)
#set heading(numbering: "1.")

= Flow incremental compilation performance

This change follows an investigation of a native ARM Flow installation on an
Apple M2 with 8 GiB of memory. One editor session reported compilations of
15.9, 12.0, 205.9, 183.1 and 127.0 seconds. Hover and semantic-token requests
also stalled. These observations identify an interactive performance problem;
they do not identify how much of any one compilation was CPU work, cache
contention, queueing or memory pressure.

== Implemented behavior

Compilation logs now identify the project, revision, queue wait, execution,
diagnostics, total elapsed time and whether the snapshot was superseded.
Memory observations describe the whole process, including physical footprint
on macOS; they are not per-compilation allocation measurements.

Queued work checks its revision and project lifetime before executing.
Completed work is acknowledged by the project actor before it can publish a
result. A late worker cannot release a replacement worker's admission slot.
The editor also checks diagnostic tickets when it consumes queued updates.
Already-running Typst evaluation finishes safely; this change does not add
mid-instruction cancellation to Typst or its WASM interpreter.

Themed standalone previews start without compiling either palette. A palette
becomes active when its first viewer connects, and becomes idle after its last
viewer disconnects. Input state remains current while idle. WebSocket EOF now
terminates its actor, and connection-owned render and outline tasks stop with
that connection. Previously, an open broadcast channel could keep those tasks
alive after the socket had ended.

Cache maintenance has one active owner even when only the dedicated dark
project is used. Superseded work that actually executed can request maintenance;
skipping a queued task does not advance cache ages.

== Expensive byte results

The vendored comemo 0.5.1 patch keeps normal lookup hashes and dependency
validation. It permits opt-in extended retention for successful, dependency-free
`StrResult<Bytes>` results whose computation took at least 50 milliseconds.
This includes Typst plugin byte outputs, but does not retain plugin instances,
WASM stores, errors or calls with tracked dependencies or mutations through
the extended budget.

The policy allows at most 64 protected entries and 64 MiB of reported output
payload, with at most 16 unused eviction sweeps. macOS warning pressure lowers
the payload budget to 16 MiB; critical pressure removes extended protection.
When pressure information is unavailable, the normal budget applies. Manual
`comemo::evict(0)` clears protected entries too. These limits describe logical
output payloads, not total allocations, ordinary caches, or process RSS.

Protection reservations live in a side table only for function caches that have
protected outputs. Ordinary cache entries carry no retention field. Each function
cache remembers its positive or negative output-policy lookup; a registration
generation invalidates that lookup when another type is registered. Normal misses
therefore avoid repeatedly locking and hashing in the global policy registry.

Maintenance logs include cumulative `expensive_compute_count` and
`expensive_compute_time` for eligible costly byte computations, including those
that could not reserve space in the budget. `protected_hits` counts reuse of
protected entries separately; these counters do not describe the whole compiler's
cache hit rate. Computation timing stops before cache insertion. Nested calls can
overlap, so summed computation times do not measure elapsed time or time saved.

Expired outputs and tracked calls are detached while holding their cache lock
and destroyed after releasing it. Eviction callbacks also run without holding
the global registration lock. Insertion-conflict destruction is unchanged.
Temporary detachment vectors scale with the values and tracked calls removed by
one function-cache sweep; their memory is outside the protected-output budget.

== Native macOS execution

Rayon workers receive a user-initiated QoS baseline. Compilation and semantic
analysis use scoped user-initiated QoS. Cache maintenance uses utility QoS under
normal or unknown memory pressure, and user-initiated QoS under warning or
critical pressure so reclamation remains responsive. The pressure is checked
for each sweep, and the worker's previous class is restored afterward. macOS
chooses the actual
cores. The application does not pin threads to performance cores or infer core
assignment from QoS. Critical memory pressure limits simultaneous project
compilations while allowing at least one pending task to make progress.

The app's LaunchAgent declares interactive work. The `flow-release` Cargo
profile enables ThinLTO, and `flow-app.py build --build-engine` records the native
target and build settings. No Metal backend, PGO training or WASM JIT is part
of this change.

== Reproduction

The integration harness uses disposable documents, ports and focus stores:

```sh
python3 tests/perf/incremental-performance/preview_demand.py \
  --baseline /path/to/baseline --candidate /path/to/candidate \
  --output /path/to/results/preview-demand.json
```

The PDE replay uses an isolated snapshot and modifies only in-memory documents.
Run the two binaries sequentially against the same snapshot, then compare their
recorded results:

```sh
python3 tests/perf/incremental-performance/book_replay.py \
  --binary /path/to/baseline --book /path/to/book-snapshot \
  --output /path/to/results/baseline --modes compile mixed --rounds 6 \
  --timeout 300 --max-footprint-gib 5 --warm-quiet-seconds 3
python3 tests/perf/incremental-performance/book_replay.py \
  --binary /path/to/candidate --book /path/to/book-snapshot \
  --output /path/to/results/candidate --modes compile mixed --rounds 6 \
  --timeout 300 --max-footprint-gib 5 --warm-quiet-seconds 3
python3 tests/perf/incremental-performance/book_replay.py \
  --compare /path/to/results/baseline /path/to/results/candidate \
  --output /path/to/results/comparison
```

Replay output records exact protocol replies, input hashes, receipt times,
compilation logs and process memory. The footprint guard stops the harness's
own server if the requested budget is exceeded. Baseline and candidate run
sequentially with identical inputs; unrelated activity on the shared Mac can
still affect timings. Separate LSP and standalone processes do not share their
in-memory compiler caches.

Each in-memory edit includes a tiny text probe with a unique, intentionally
missing font. Its compiler warning identifies the newest compiled source.
`edit_result_seconds` ends when that exact diagnostic arrives; it includes
diagnostic conversion and publication. Word-count status repeats and older
compilation notifications cannot satisfy this endpoint. The probe is never
written to the book snapshot or the user's sources.

The Maquette retention replay lets two completed cache sweeps age the figure's
inner results while only the chapter changes. It then replaces `nx = 64` with
the equal integer expression `nx = 32 * 2`, forcing figure reevaluation with
unchanged mesh and render arguments. A hidden, placed font probe in that same
figure confirms delivery of its new revision. Each measured phase must log one
successful main compilation, a subsequent fresh sweep worker, and its completed
sweep in that order. Extra or overlapping work fails the phase instead of using
an ambiguous counter snapshot. Successful LSP status and unchanged relevant logs
must then remain quiet for three seconds (`--settle-quiet-seconds`); warm-up also
uses the protocol quiet check. This is an observed boundary, not an engine idle
API. The required evidence is at least four protected lookup events, no new
eligible costly computations at the trigger, and unchanged protected entry and
payload counts across aging and triggering. Hit events do not identify distinct
outputs, and calls below the cost threshold are not counted as costly. This
checks cache reuse; complete PDF equality is verified separately.

```sh
python3 tests/perf/incremental-performance/cache_replay.py \
  --baseline /path/to/baseline --candidate /path/to/candidate \
  --book /path/to/book-snapshot --output /path/to/results/cache-replay \
  --require-retention
```

For the book edit/query replay, after the warm marker arrives, the harness
requires three seconds of successful
compilation without new compile activity or diagnostic publications. This lets
initial dependency-watch enrollment settle before measured edits. The duration
is configurable with `--warm-quiet-seconds` and recorded in the results; it is
an observed quiet window rather than a server readiness guarantee. A latest-query
failure still fails the run, and measured requests are never silently retried.

Both runs must complete without a footprint-limit stop, and their input hashes,
warmup settings, token legends and comparable protocol responses must agree
before interpreting latency changes. Six rounds provide a small exploratory
sample; retain the raw values and report the median and range. A p95 from six observations is effectively
the largest sample. Close results need more rounds or a reversed execution order.

== Validation commands

Run these commands from the repository root. The shared Cargo environment used
on the 8 GiB Mac limits build concurrency and debug-data growth:

```sh
export CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
```

Native compiler, preview and language-server library tests:

```sh
cargo test --locked \
  -p tinymist-project -p tinymist-preview -p tinymist \
  --features tinymist/system,tinymist/preview,tinymist/export,tinymist/trace \
  --lib
```

Strict Clippy for all targets in the affected native crates:

```sh
cargo clippy --locked \
  -p tinymist-cli -p tinymist -p tinymist-project \
  -p tinymist-preview -p tinymist-std \
  --all-targets -- -D warnings
```

Minimal, preview and web feature checks compile the library on the host:

```sh
cargo clippy --locked -p tinymist --lib --no-default-features \
  --features no-content-hint -- -D warnings
cargo clippy --locked -p tinymist --lib --no-default-features \
  --features no-content-hint,preview -- -D warnings
cargo clippy --locked -p tinymist --lib --no-default-features \
  --features no-content-hint,web -- -D warnings
```

The patched comemo package, formatting and macOS app unit tests:

```sh
cargo test --locked -p comemo
cargo clippy --locked -p comemo --all-features --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -m unittest discover -s tests/flow-app -p 'test_*.py' -v
```

The end-to-end suite uses the engine already staged at
`editors/vscode/out/tinymist`. Stage the binary being evaluated before running
the suite; this command does not build or select that engine:

```sh
INSTA_UPDATE=no cargo test --locked -p tests
```

== Validation results

Native library tests passed: 95 language-server, 11 preview and 30 project
tests, with eight pre-existing ignored cases. Three macOS scheduling/memory
tests and 34 comemo unit, integration and documentation tests also passed.
Strict Clippy passed for affected native crates and the minimal, preview and
web feature combinations on the ARM Mac host. These feature checks are not a
cross-compilation to WebAssembly. Formatting and Python syntax checks passed.

The intermediate 0.1.4 candidate at `7bfc336f` passed nine CLI/end-to-end tests,
nine app tests and both preview smoke suites. The preview demand comparison
reduced compilations from two to zero without viewers and from two to one with
one active palette. Two active palettes still received two updated documents.
Cold first frames, saved positions, offline edits and reconnects passed.

The 308-page book's expanded PDF bytes matched after masking only Info/XMP
timestamps, XMP document/instance IDs and the final trailer IDs. All 418 semantic
Note IDs were preserved. Rendered pages 1 and 269 were byte-identical and were
visually checked. Source snapshots were unchanged.

That intermediate candidate was withheld from installation after repeated-edit
measurements found a regression. With six samples per mode, baseline-first
compile edit-result medians were 11.37 and 15.01 seconds; mixed medians were
39.26 and 36.93 seconds. Reversing the compile-only execution order gave
11.53 and 13.56 seconds for baseline and candidate. All 18 latest mixed-query
responses agreed, but these timings did not demonstrate an overall speedup.
The follow-up reduces general cache bookkeeping overhead and makes maintenance
priority respond to memory pressure.

=== Final 0.1.4 candidate

The final engine was built from clean commit `b9d56e3b` for
`aarch64-apple-darwin`, using `flow-release` and ThinLTO. Its SHA-256 is
`985921fe7a6883d9c21a9e37368874d9bf2f180deccb3a8c1164627ade7611d2`.
The frozen installed 0.1.3 baseline is from `e298657d`; its manifest does not
record the original Cargo profile or compiler flags. This is a comparison of
two product binaries, not an isolated measurement of one source change.

The final binary passed the nine end-to-end tests, nine app tests, both preview
smoke suites and the demand comparison again. Five parser regressions for the
cache replay boundaries passed. The final 308-page PDF again matched the
normalized baseline byte for byte, preserving all 418 semantic Note IDs.
Rendered pages 1 and 269 matched exactly and were visually inspected.

Plain edits were measured in candidate-baseline-baseline-candidate order, with
six edits in each fresh process. All four runs had matching configuration,
unchanged source manifests and seven compilations including warmup. Results:

#table(
  columns: (2fr, 1fr, 1fr),
  [Measurement], [Baseline], [Candidate],
  [First pair median], [12.568 s], [12.378 s],
  [Reverse pair median], [11.157 s], [12.664 s],
  [Pooled 12-edit median], [11.858 s], [12.479 s],
  [Pooled range], [9.274–17.119 s], [9.234–16.670 s],
  [Median excluding first edit of each run], [12.217 s], [12.913 s],
  [Whole-process CPU median], [14.715 CPU-s], [17.492 CPU-s],
  [Maximum sampled footprint], [3.966 GiB], [4.095 GiB],
)

The pooled plain latency increased by 5.24%, and process CPU time by 18.88%.
The two pairwise latency changes differed (-1.51% and +13.51%). These are two
process sessions per build; the twelve edits are not independent experimental
runs. The plain-compilation regression is reduced but has not been eliminated.
No extra compilation or sweep was found: each plain run had exactly seven of
each. Detached eviction buffers, retention checks, instrumentation and scheduling
changes add work; these observations do not isolate any one cause.

One final six-round mixed-workload pair produced:

#table(
  columns: (2fr, 1fr, 1fr),
  [Measurement], [Baseline], [Candidate],
  [Edit-result median], [34.260 s], [33.697 s],
  [Edit-result range], [26.804–47.932 s], [29.462–39.457 s],
  [Latest completion median], [34.274 s], [33.697 s],
  [Latest semantic-token median], [16.974 s], [16.256 s],
  [Whole-process CPU median], [60.986 CPU-s], [67.462 CPU-s],
  [Maximum sampled footprint], [4.579 GiB], [4.560 GiB],
)

All eighteen latest replies matched after documented protocol normalization.
Both binaries already cancelled twelve requests and rejected six obsolete
queries. Both performed thirteen compilations; the candidate finished seven
and discarded six after execution. There were no before-start skips in this
particular replay. Thus it demonstrates stale-publication handling, not CPU
savings from interrupting those six evaluations. Mixed latency was near parity,
while process CPU time increased by 10.62%.

The hardened Maquette replay passed for both binaries with unchanged inputs.
Four protected entries totaling 4,840,856 payload bytes survived two unrelated
edit/sweep phases. Those phases added zero protected hits and zero eligible
costly computations. The figure trigger then added four protected lookup events
and zero eligible costly computations, with stable entry and payload counts.
This demonstrates retained-result reuse. The one paired trigger latency was
18.916 s for baseline and 20.073 s for candidate; it does not demonstrate a
wall-time gain. No claim is made that four lookup events identify four distinct
outputs or that sub-threshold computations did not occur.

The demonstrated practical reduction is demand-driven preview work: no-viewer
startup/edits changed from two compilations to zero, and one-palette edits from
two to one. Two active palettes still require two updated documents. First
connection after server startup waits for that palette's initial compilation;
an unchanged reconnect reuses its last successful document. These changes and
bounded retention are useful independently of general compiler throughput.
The release must not be described as a universal compiler acceleration or as a
resolution of the historical 183-second event.

Raw local evidence is retained under `.local/performance-20261003/`, including
`replay-{baseline,candidate}-refined-{a,b}`, the two mixed replays,
`comparison-refined-*`, `cache-replay-refined`, and
`pdf-qa/refined-candidate`. The source-only replay manifest checks 2,036 files;
the separate PDF and cache checks also cover their recorded binary inputs.

=== Installation status

The verified 0.1.4 bundle was installed at
`/Users/taeyoung/Applications/tinymist-flow.app` on 2026-10-03. The prior 0.1.3
bundle is backed up under the app's releases directory at
`20261003-233421-073173/tinymist-flow.app`. The menu app was restarted,
the managed PDE service was restarted, the stopped Ewald service stayed stopped,
and the selected profile stayed unchanged. The LaunchAgent now records
`ProcessType=Interactive`. Local and remote VS Code settings already select the
installed engine path; no separate running LSP process was found.

Live document delivery is pending macOS Documents consent. The first HTTP
probe received connection refused. A process sample showed the new engine
blocked in font-directory `open`; TCC logs explicitly reported a changed code
requirement and `AUTHREQ_PROMPTING` for `SystemPolicyDocumentsFolder`.
The computer-use tool refused access to UserNotificationCenter for safety,
so the user was asked to handle that existing macOS prompt. No consent database
or security setting was changed. A confirmed-release tag must wait for successful
local and Tailscale document-frame verification after consent.

On 2026-10-04, the same installed engine returned HTTP 200 for the frontend,
but no document frame arrived during the follow-up check. A new process sample
showed compilation blocked in source-file `open`, and TCC logs still recorded
Documents-folder prompts for the changed code requirement. HTTP availability
therefore did not establish document access or successful preview delivery.
The follow-up receiver was stopped; no password was used and no permission or
security setting was changed. The release remains untagged pending live delivery.

The original 183-second event has not been retrospectively decomposed. Compile
worker queue timing starts at dispatch; it excludes the time a newer revision
remains pending behind an already-running compilation. Stage timings are wall
durations and cannot be summed across overlapping background work as CPU time.
