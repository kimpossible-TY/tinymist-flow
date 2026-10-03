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
- Expired outputs and tracked calls are detached under the cache lock and
  destroyed after unlocking. Global eviction callbacks run without holding the
  evictor registration lock.
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
