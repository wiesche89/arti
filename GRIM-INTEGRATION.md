# Grim integration of tor-hsservice 0.44.0

Upstream: https://gitlab.torproject.org/tpo/core/arti

The `fix/hsservice-publication-status` branch contains the source fix and regression
test, based on upstream release source `46472491e80b1fdfb878efa2a6413deb90885b0e`.
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
cargo test -p tor-hsservice --lib completed_uploads_survive_a_redundant_scheduled_publication
```

Before the fix the all-clean case reports `Bootstrapping` instead of `Running`.
The test also verifies that a clean first period does not block a later pending
period. The tor-hsservice library suite passed with 40 tests and 2 ignored tests.
