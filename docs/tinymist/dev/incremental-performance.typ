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
priority respond to memory pressure. Final results are recorded after validation.

The original 183-second event has not been retrospectively decomposed. Compile
worker queue timing starts at dispatch; it excludes the time a newer revision
remains pending behind an already-running compilation. Stage timings are wall
durations and cannot be summed across overlapping background work as CPU time.
