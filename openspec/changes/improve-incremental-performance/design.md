# Design

## Measurement

Keep the existing total elapsed report compatible. Add stage timing with project
and revision identity, including queue wait before execution and diagnostic
collection after compilation. Mark obsolete work explicitly. Memory observations
describe the current process and must not be presented as allocations attributable
to an individual compile.

## Scheduling and viewer demand

Cancellation is cooperative at queue and publication boundaries. An already
running Typst call must finish safely; completing or skipping it must release the
project's in-flight state so the latest pending revision can run. Lifecycle tickets
must distinguish a replacement project from a previous instance using the same ID.

Standalone themed previews begin idle. The first live connection activates its
variant and the final disconnect suspends further compilation. World updates and
pending reasons remain available while idle. Reconnection compiles or reuses a
result only when it matches current inputs. Multiple viewers of one palette share
one compiler. Active viewers of both palettes retain independent current outputs.
Origin checks, focus mapping, and saved reading state keep their existing meaning.

## Expensive results

Use a small, licensed local comemo patch rather than vendor the Typst workspace.
Retention is opt-in for successful pure byte results with measured expensive
misses. Existing input hashes and dependency validation remain authoritative.
Bound extended retention by age, entry count and logical output payload bytes.
These limits are not a bound on total process memory or the ordinary cache.
Manual cache clearing must bypass extended retention. Memory pressure may reduce
the budget; unavailable pressure information must have a documented fallback.
Remove expired entries under the cache lock and run their destructors after
releasing it to avoid blocking unrelated lookups.
Report cumulative eligible expensive-computation counts and elapsed body times,
including computations whose outputs exceed the retention budget. Count protected
cache hits separately. Exclude errors, tracked dependencies and mutable calls;
these measurements are neither whole-cache hit rates nor wall-clock time savings.

## Apple Silicon

Use native ARM builds and operating-system QoS hints, not CPU affinity. Interactive
compilation and semantic analysis receive user-initiated QoS; maintenance receives
utility QoS. Scoped changes restore the worker's previous class. Parallel Rayon
workers receive an interactive baseline so work stealing does not silently move
layout work to a differently classified pool. Other platforms keep their existing
behavior. Report physical footprint and memory pressure on macOS where supported.
ThinLTO is an explicit release build option; PGO or GPU changes require separate
evidence and are not implied by native compilation.

## Validation

Add deterministic regressions for stale queue work, revision/lifecycle races,
completion acknowledgement, viewer reference counts and reconnects, expensive
cache retention and pressure budgets, manual clearing, and lock-free destruction.
Run affected crate tests, strict lint and native/web feature checks. Compare the
installed baseline and candidate on identical saved source snapshots with repeated
in-memory edits. Record correctness, timing, memory and limitations. Build and
validate the bundled application before applying the candidate installation.
