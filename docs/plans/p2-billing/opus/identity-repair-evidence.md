# Identity repair: JWKS cached-hit expiry

Finding: `docs/plans/p2-billing/astra/host-review.md`, P1 "JWKS cached hit" (lines 17-19). Both verifiers returned a cached key for a known `kid` regardless of the set's age, so a key the provider removed stayed trusted until some unknown kid happened to force a refresh.

Source is complete. The lead has not run the tests yet (see Verification).

## Files

| File | Change |
|---|---|
| `crates/taskr-server/src/identity.rs` | `SupabaseJwt` JWKS cache bounded on hits; private clock; fetch timeout; internal tests `tests::jwks::*` |
| `transgate/billing/src/identity.rs` | `JwtIdentity`, same cache rule; private clock; internal tests `jwks_tests::*` |
| `crates/taskr-server/tests/fixtures/{README.md,jwks-test-key-a.pem,jwks-test-key-b.pem}` | synthetic RSA keys (Sonnet created them; I reworded the README) |
| `transgate/billing/tests/fixtures/{README.md,jwks-test-key-a.pem,jwks-test-key-b.pem}` | same keys, same README change |
| `transgate/billing/tests/jwks_rotation.rs` | **deleted**. Sonnet's wall-clock version (400 ms / 40 ms sleeps, not deterministic, with a known throttle bug). Its scenarios moved into the internal tests |

No Cargo.toml, lockfile or dependency changes. Public API: `SupabaseJwt::new(Option<&str>, Option<String>)` and `JwtIdentity::new(Option<&str>, Option<String>, bool, BTreeSet<String>) -> Result<_, String>` are unchanged. Callers `crates/taskr-server/src/main.rs:74` and `transgate/billing/src/lib.rs:155` are untouched. Sonnet's uncommitted `with_jwks_cache(max_age, miss_throttle)` builder and the `max_age`/`miss_throttle` fields are removed. They had no production caller, and the tests now run against the production constants instead.

## Behavior (identical in both verifiers)

Cache: `RwLock<Option<(JwkSet, Instant)>>`. Constants: `JWKS_MAX_AGE` = 600 s and `JWKS_REFRESH_AT_MOST_EVERY` = 60 s.

1. Read lock: return the key only if the set's age is below `JWKS_MAX_AGE` **and** it lists the kid.
2. Otherwise take the write lock, so only one refresh runs at a time. Then re-check: if the set is still fresh and lists the kid, return it, because another request already refreshed. If the set is fresh but the kid is unknown and the set is younger than the throttle, refuse as unknown without fetching.
3. Otherwise fetch. A transport error, non-2xx status or bad body gives `IdentityError::Unavailable` / `AuthError::Unavailable`. The cache is left untouched, so a stale key is never used and the next request tries the provider again. On success the whole set is replaced, so removed kids disappear. A kid that is missing after the refresh gives `Invalid` / `Unauthenticated`.
4. taskr's `reqwest::Client` now has a 10 s timeout, the same as transgate's. A hanging provider would otherwise hold the refresh lock and stall every JWKS verification. Building the client fails only when `Client::new` would panic too (no TLS backend).

Time control: a private `now: Box<dyn Fn() -> Instant + Send + Sync>` field, set to `Instant::now` in `new`. Only the in-module tests replace it, with a manual `Arc<Mutex<Instant>>` clock. There are no sleeps, no paused tokio time and no new tokio features. Token `exp` still uses real system time (now + 1 h); it is independent of the cache clock.

## Tests

Every case starts a local axum JWKS endpoint on `127.0.0.1:0`. The endpoint has a mutable key list, a "down" switch that returns 503, and a fetch counter. Tokens are signed with the synthetic RS256 keys. The cases go through the real public paths: `Identity::verify` for `SupabaseJwt`, and `BillingIdentity::authenticate` with an `Authorization: Bearer` header for `JwtIdentity`.

| Test (same name in both crates) | What it pins |
|---|---|
| `a_cached_key_is_trusted_only_for_the_cache_lifetime_and_a_removed_key_then_stops_verifying` | Fresh cache: the key is listed, then removed. At 599 s it still verifies and fetches stay at 1. At 600 s it is refused (`Invalid` / `Unauthenticated`) and fetches = 2. It stays refused with no further fetch |
| `a_rotated_in_key_refreshes_the_set_and_the_retired_key_is_refused_at_once` | Rotation A→B: after the throttle a B token triggers a refresh and verifies (fetches = 2). An A token is then refused with no fetch, long before 600 s |
| `an_expired_set_that_cannot_be_refreshed_fails_closed_until_the_provider_answers` | At 600 s with the provider down, two requests both get `Unavailable` and each tries the provider (fetches = 3). Once the provider is back the token verifies (fetches = 4) |
| `unknown_key_ids_refresh_the_set_at_most_once_per_throttle_window` | Unknown kid: 6 refusals up to 59 s cause no fetch. At 60 s five refusals cause exactly one fetch |
| `concurrent_requests_on_an_expired_set_share_one_refresh` | 4 concurrent verifications (`tokio::join!`) on an expired set all succeed with exactly one refresh (fetches = 2) |

The fixture moduli `N_A`/`N_B` were checked against both PEM copies with `openssl rsa -noout -modulus` (4/4 match), and the two directories hold identical copies.

## Verification

- rust-analyzer: diagnostics on both files are clean. Hover resolves `identity.verify` to `impl Identity for SupabaseJwt` and `identity.authenticate` to `impl BillingIdentity for JwtIdentity` inside the test modules.
- I have not run cargo build/test/fmt/clippy, as the assignment requires; the lead owns integrated checks. Commands for the lead:

```sh
cargo test --locked -p taskr-server --lib identity::
cargo test --locked -p transgate-billing --lib identity::
```

Both need only loopback networking. They make no production calls and use no real keys.

## Residual (not changed)

- While the provider is down, every request on an expired set (or an unknown kid after the throttle) makes one fetch attempt. The attempts run one at a time, each bounded by the 10 s timeout. Requests fail closed, but the failures themselves are not throttled. This is a possible follow-up if load against the provider matters.
- The taskr-server test module path is `identity::tests::jwks::`; transgate's is `identity::jwks_tests::`. The filters above cover both.
