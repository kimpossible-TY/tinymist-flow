# Improve incremental performance

## Problem

Editing the PDE book produced compile reports between 12 and 206 seconds in one
session, with simultaneous delays in hover and semantic tokens. The existing
elapsed time includes worker queueing. Standalone preview maintains light and
dark variants even when nobody views them, while inexpensive and expensive memo
results share the same short retention policy. The native ARM build does not
explicitly classify CPU work for macOS scheduling.

## Changes

- Report queue, compilation and diagnostics time with project and revision IDs.
- Skip superseded queued compilations and acknowledge their completion safely.
  Publish results only when they still match the project's inputs.
- Compile themed previews on viewer demand, preserving current inputs while idle
  and cleaning up connection-owned work on disconnect.
- Retain expensive pure byte results within explicit entry and payload limits,
  reduce those limits under memory pressure, and release expired objects outside
  cache locks.
- Classify interactive CPU work and cache maintenance with macOS QoS. Record
  process memory and system pressure. Provide a reproducible native release build.
- Validate behavior and measure edit-to-result latency using isolated document
  snapshots and the existing installed engine as the baseline.

## Non-goals

This change does not add Metal rendering or replace the WASM interpreter. It
does not forcefully interrupt arbitrary Rust or WASM execution. Sharing all
in-memory compiler state between independent LSP and preview processes requires a
separate service protocol; this change reduces unused preview work instead.
