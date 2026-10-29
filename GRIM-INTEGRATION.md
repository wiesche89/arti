# Grim integration of tor-hsservice 0.44.0

Upstream: https://gitlab.torproject.org/tpo/core/arti

The `fix/hsservice-publication-status` branch contains the source fixes and regression
tests, based on upstream release source `46472491e80b1fdfb878efa2a6413deb90885b0e`.
Use that branch's source commit when reporting the bug upstream.

This integration branch additionally uses the crates.io-normalized manifest for
`crates/tor-hsservice`. Its dependencies resolve to the unchanged registry releases
instead of the sibling Git workspace packages. This lets Grim patch just
`tor-hsservice` without introducing a second, incompatible copy of the Tor types.
No dependency versions, circuit timeout, or publication rate limit are changed.
The integration branch is intended as a Cargo dependency for Grim; use the fix
branch for development and testing of the complete Arti workspace.

## Reproduction

A scheduled publication sets the publisher status to `Bootstrapping`. If prior
in-flight uploads have since made all time periods clean, `upload_all` returned
without another upload or status update. The status could remain stuck until the
next unrelated event, despite successful publication to both HSDir rings.

A second premature return skipped later dirty time periods when the first period
was already clean. The fix restores status from existing upload results for an
entirely clean pass and continues to later periods when only one period is clean.
It does not assume successful publication or suppress upload failures.

The regression test uses a mock runtime and mock HSDirs, with no Tor network:

```sh
cargo test -p tor-hsservice --lib publication_results_preserve_progress_and_ignore_superseded_uploads
```

Before the fix the all-clean case reports `Bootstrapping` instead of `Running`.
The test also verifies that a clean first period does not block a later pending
period. The tor-hsservice library suite passed with 41 tests and 2 ignored tests.

## Superseded publications

A second publication could start while an earlier batch was still retrying. The
old batch continued uploading obsolete descriptors and could replace the newer
batch's successful status with its late failures. Each period now has one current
upload task. Replacing it, or invalidating its descriptor after an IPT/config
change, cancels the old task. A unique publication identity also rejects results
already queued before cancellation. The mock regression covers replacement,
late failure after success, and invalidation while waiting for the rate limit.

The 60-second publication rate limit and five-minute batch deadline are unchanged.

## Incremental publication results

The publisher previously waited for all HSDirs in a time period before reporting
any result. One slow HSDir could therefore hide successful uploads to the rest
of a ring for the entire retry budget. It now reports each completed upload,
merges outcomes by relay, and schedules one republish timer when the batch ends.
Existing status rules and superseded-publication checks remain in force.

The mock regression verifies immediate reachability from successful uploads in
both rings while one HSDir is still retrying, a later `DegradedReachable` result
when that relay fails, and recovery by retrying only that relay without losing
other results or repeating their uploads.

Retry sleeps previously could outlast the overall upload deadline. They now
observe that same deadline; the existing five-minute budget is unchanged.
`publish::backoff::tests::overall_timeout_interrupts_retry_delay` reproduces this
with a retry delay longer than the overall timeout.

Source commits for these additions:
- `2ecb8ebbcc5f0ea8f69d7984f3c8a71ba2ccff59` (retry deadline)
- `95ea96823f88f1a8c425e573b6498b1588f42a09` (incremental results)
