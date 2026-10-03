# Local comemo patch

Imported from the crates.io `comemo` 0.5.1 source archive, upstream commit
`0eaf0707f37032ad936d6d63bb60755fdd1d2cda`. The original MIT and Apache-2.0
licenses are included without modification.

Tinymist-flow additions:

- Opt-in retention by output type and elapsed computation cost. Only outputs
  without tracked calls or mutations qualify. Per-type entry and logical output
  payload limits are shared across function caches. Limits can be reduced when
  the host is under memory pressure. Ordinary comemo behavior is unchanged until
  the embedding application registers a policy. Cumulative expensive-computation
  counts and body times include budget-rejected eligible results; protected hits
  are reported separately and do not represent the whole compiler cache hit rate.
- Policy lookups are cached per memoized function, including negative lookups.
  A registration generation safely refreshes existing caches after late policy
  registration; budget changes remain visible through the shared policy handle.
  Ordinary misses compare one atomic generation under their existing cache
  lock, without taking the global policy lock or hashing an output type again.
- Protection reservations live in a lazily allocated per-function side map
  keyed by leaf ID, not in ordinary cache entries. Lookup hashing and dependency
  validation remain unchanged. Removal and manual clear detach reservations
  before leaf IDs can be reused. Only functions with protected entries perform
  a side-map lookup on hits; ordinary hits retain one optional-map branch.
- Expired outputs and tracked calls are detached under the cache lock and
  destroyed after unlocking. Global eviction callbacks run without holding the
  evictor registration lock. Detachment remains linear and uses temporary ID,
  output, and tracked-call buffers; those buffers are not part of the protected
  payload budget. The output buffer is sized once to avoid repeated relocations.
- Normalize the upstream zero-argument macro helper, trait-object test
  declarations, and nested constraint check to remain warning-free when built
  as a workspace member. The constraint check uses the same Rust 1.88-compatible
  let-chain syntax already present upstream.
- Regression coverage is in `tests/retention.rs`. Existing upstream tests remain
  intact and must run with `--all-features` when updating this patch.

The payload limit is not an RSS or total allocator limit. It counts the sizes
reported by the registered output callback; it excludes shared allocation
capacity and ordinary cache entries. The standard comemo hash and constraint
validation are unchanged. No second hash-only lookup or on-disk cache is added.
