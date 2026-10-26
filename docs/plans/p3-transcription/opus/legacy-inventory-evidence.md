# Legacy archive inventory (F9 / D2) — transport handoff, p1Z

Status: source-ready, not built or run. This closes the P3 transport half of F9. It is not a P9 waiver. P9 still has to prove the real snapshot and cutover inventory. Nothing here shows that any real object is absent.

## Seam (additive; no existing signature changed)

- `media-ports/src/lib.rs:106-119` `ObjectInventory`:
  - `kind()` and `bucket()` identify the store.
  - `inventory(owner) -> Result<Vec<ObjectRef>, TransportError>` is a full, bounded, paginated traversal of every object, at any depth, under `{owner}/`.
  - `owner` is a single key segment: the erasure record's account UUID. The adapter builds the prefix itself.
  - A failed, unauthorized, too deep or too large traversal is an `Err`, as is any detected listing anomaly. It never comes back as a partial list.
  - **It is not an atomic snapshot.** Offset pages can miss objects shifted by concurrent writes or deletes.
  - No billing, host or domain types enter media-ports.
- `media-adapters/src/storage.rs:219-270` `impl ObjectInventory for LegacySupabaseStorage`:
  - It reuses the real `list_prefix` (`POST /storage/v1/object/list/jobs` with service authority). That method's name and page-size checks are unchanged.
  - It pages 1000 at a time and moves to the next offset until it gets a short page.
  - It recurses into folder entries (`id: null`).
  - It returns `{Supabase, "jobs", key, version: None}`, sorted by key. The Supabase listing `id` is not a generation. These are exactly the refs the same adapter's `delete` accepts, and `delete` rejects versions.
  - `S3Store` does not implement the trait; GCS/R2 objects are always recorded in rows.

| Condition | Result |
|---|---|
| owner empty, containing `/`, or `.`/`..` | `Rejected{legacy_inventory_scope}`, no request sent |
| any page or nested folder fails (HTTP status, 401, transport) | `Unavailable{status, "legacy list"}`; the whole traversal is lost |
| a name repeats within one folder's pages | `Unavailable{"legacy list changed during inventory"}` |
| a name containing `/`, or equal to `.`/`..` | `Malformed("legacy inventory path")` |
| a page longer than requested | `Malformed("legacy list page size")` |
| a non-JSON body | `Malformed("legacy list")` |
| folder depth at or beyond `LEGACY_INVENTORY_DEPTH` (8; the layout is `{account}/{job}/{file}`) | `LimitExceeded("legacy inventory depth")` |
| more than `LEGACY_INVENTORY_LIMIT` (100000) entries, folders included, which also bounds the queued folders | `LimitExceeded("legacy inventory size")` |
| more than `LEGACY_INVENTORY_REQUESTS` (20000) list calls, so empty-folder trees are bounded | `LimitExceeded("legacy inventory requests")` |

**Ordering is deliberately not checked.** The adapter asks for `sortBy name asc`, but Supabase sorts with the database collation, which is not Rust byte order. A byte-order check could falsely fail every real inventory and keep erasures open forever. Repeats are detected; skips are not.

**Completion rule.**
- An empty answer closes the item only after the relevant writers are fenced or drained: the erasure barrier is in place and outstanding legacy uploads have expired or been drained.
- A later traversal repairs names skipped because of concurrent deletes. It cannot see objects written after the final traversal.
- Before cutover the legacy app is a writer outside our barrier, so P9's drain is a precondition, not something this seam provides.

## Consumer contract (agreed by owners; they own those files)

- **p1W, `privacy/erasure.rs`:** `build_items` always emits `ItemTarget::LegacyAccountInventory { bucket: "jobs" }` as `Pending`. It runs in phase 0 with the legacy objects, so it always runs before `Identity`. The owner segment is the record's account UUID. The item creates no per-object rows.
- **p1X, `privacy/app.rs`:**
  - `PrivacyServices.inventories: Vec<Arc<dyn ObjectInventory>>`.
  - `run_item` handles each outcome as follows:
    - No inventory matching both kind and bucket: `Unresolved("legacy storage inventory is not configured")`.
    - `Err`: `Retrying`.
    - A non-empty list: delete each object through the `ObjectStore` matching both `Supabase` and `jobs`, then stay `Retrying`. A retained, unsupported or failed deletion keeps the item open.
    - An empty list: `Absent`.
  - Result: an erasure can reach `FullyErased` only after an empty traversal under the completion rule above.
- **p1T, `server/src/host.rs`:** keep the single `LegacySupabaseStorage` as one `Arc`, and push it into both `objects` and `inventories`. As p1T last reported, the source now *requires* `LEGACY_SUPABASE_URL` and `LEGACY_SUPABASE_SERVICE_ROLE_KEY` when `TRANSGATE_JOBS_ENABLED=1`, so a misconfigured host refuses to start. There is no bypass flag. `inventories` wiring waits for p1X's field.
- **Lead must confirm:** keep the required configuration, or go back to optional configuration where an unconfigured item stays `Unresolved`. Either satisfies "never silent FullyErased". With optional configuration, a deployment that never had the legacy bucket could never close an erasure without a recorded attested-absence decision. p1T switched between the two after my message; please confirm one in writing.

## Tests (local HTTP fixture, `crates/media-adapters/tests/storage.rs`)

- `d2_legacy_inventory_pages_every_folder_of_one_owner_and_feeds_exact_deletions` checks:
  - a full 1000-entry page, then a short page, then a nested job folder, giving 1002 objects;
  - every key stays under `account-a/` and the order is deterministic;
  - the exact list request bodies, offsets and service headers;
  - a folder entry is never returned as an object;
  - a returned ref is deleted and confirmed by the same authority;
  - only an authenticated `[]` gives an empty inventory.
- `d2_legacy_inventory_rejects_detected_listing_failures_instead_of_partial_results` checks:
  - the owner-scope rejections send no request;
  - a later page answering 503, and a nested folder answering 401;
  - a repeated name across pages;
  - escaping names and an oversized page;
  - excess depth and a non-JSON body;
  - every scripted page is consumed.

Command for the lead: `cargo test --locked -p media-ports -p media-adapters --test storage`. The size-limit and request-limit branches are not exercised, because reaching them would take 101 pages or 20001 requests; they are source-only.

## Remaining before F9 can close

1. p1W's item and p1X's `run_item` change land, with a core test that drives an erasure through the inventory: non-empty, then deletions, then an empty listing, then `FullyErased`. An unconfigured inventory must stay `Unresolved`, and an `Err` must keep the erasure open.
2. p1T wiring, then an actual-process smoke (p1Y) against a loopback Supabase Storage fixture. The legacy archive of an imported job is deleted before GoTrue deletion, and the saga stays open while the listing fails.
3. P9: inventory of the real snapshot and evidence for the deployed authority. No production call was made here.
