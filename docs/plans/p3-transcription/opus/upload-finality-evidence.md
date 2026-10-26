# Late signed-PUT finality: GCS XML and Cloudflare R2 (primary-source research, p1Z)

**Scope:** documentation only. I made no provider calls and changed no code. Pages were read on 2026-10-24.

**Question:** after a signed PUT URL expires, or the client times out, is there a documented hard bound after which an already-accepted upload can no longer commit?

**Answer: neither provider documents one.** A fixed grace window, such as the proposed 24 h, has no primary support. An absent HEAD is not proof that a PUT will not still commit.

## What the documentation does say

| Topic | GCS (XML API, signed URL) | Cloudflare R2 (S3 presigned) |
|---|---|---|
| Expiry semantics | Signed URL: "Anyone who knows the URL can access the resource until the expiration time ... or the key used to sign the URL is rotated" ([signed-urls](https://cloud.google.com/storage/docs/access-control/signed-urls)). It does not say whether expiry is checked at request start or at commit. | "authorizing anyone with the URL to perform a specific operation ... until the URL expires"; expiry 1 s to 7 days ([presigned-urls](https://developers.cloudflare.com/r2/api/s3/presigned-urls/)). The check time is not stated. |
| In-flight write after a negative read | Writes are atomic: "The object only becomes visible in the bucket once the entire upload has successfully completed and the server has sent a success response" ([consistency](https://cloud.google.com/storage/docs/consistency)). No bound on how long an accepted request may run. | Explicitly **not final**: "a request that was still in flight ... may still commit after the negative read has returned. Reads ... do not wait for or cancel earlier writes" ([consistency](https://developers.cloudflare.com/r2/reference/consistency/#negative-reads-and-in-flight-writes)). |
| Total request or idle bound | None found for a single PUT. Resumable sessions last up to one week (410 or 404 afterwards; [resumable-uploads](https://cloud.google.com/storage/docs/resumable-uploads)). We do not use resumable uploads. | None found. [Limits](https://developers.cloudflare.com/r2/platform/limits/) gives sizes (single PUT 4.995 GiB) and 1 write/s per key, but no duration. |
| Create-only condition | `x-goog-if-generation-match: 0`: "only proceeds if no object with the specified name exists ... or if there are only noncurrent versions" ([request-preconditions](https://cloud.google.com/storage/docs/request-preconditions)). XML `If-None-Match` is documented for GET/HEAD only ([reference-headers](https://cloud.google.com/storage/docs/xml-api/reference-headers)). It is **not documented** whether the condition is evaluated at request start or at commit. | `If-None-Match: *` on PutObject is "evaluated atomically against the committed bucket state, as part of the same write that commits the object" ([api](https://developers.cloudflare.com/r2/api/s3/api/), [consistency#conditional-writes](https://developers.cloudflare.com/r2/reference/consistency/#conditional-writes)). |
| Binding the condition into the URL | `X-Goog-SignedHeaders`: "Headers that had to be included as part of any request that used the signed URL". Canonical headers may include `x-goog-` extension headers ([signed-urls](https://cloud.google.com/storage/docs/access-control/signed-urls), [canonical-requests](https://cloud.google.com/storage/docs/authentication/canonical-requests)). Our GCS adapter uses SigV4 HMAC interop; signing `x-goog-if-generation-match` there is **not explicitly documented**. | `X-Amz-SignedHeaders` lists the signed headers; the documented example is Content-Type, which returns 403 if a different value is sent ([presigned-urls](https://developers.cloudflare.com/r2/api/s3/presigned-urls/)). Signing `if-none-match` specifically is **not documented**. |
| Deleted or overwritten data | Soft delete is on by default, retaining for 7 days (configurable 7–90 days, or disabled). Soft-deleted objects "cannot be read, modified, or overwritten" but can be restored ([soft-delete](https://cloud.google.com/storage/docs/soft-delete)). | PutBucketVersioning and Object Lock are not implemented ([api](https://developers.cloudflare.com/r2/api/s3/api/)). Nothing is documented as retained after delete. |

For comparison only, AWS S3 (not either provider) states: "checks the expiration date and time of a signed URL at the time of the HTTP request ... the download continues even if the expiration time passes" ([AWS presigned](https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-presigned-url.html)). Neither provider documents the same rule.

## Alternative assessed: conditional create-only PUT plus a persistent tombstone

**Mechanism (not implemented):**
1. Every new upload capability signs a create-only condition: GCS `x-goog-if-generation-match: 0`, R2 `If-None-Match: *`. It is returned in `TemporaryAccess.headers`, which the port already carries.
2. Erasure never deletes the upload key. It overwrites the key with a zero-byte, metadata-free tombstone and keeps the tombstone permanently. A late conditional PUT then finds a live object and fails with 412.

**Findings:**
- **R2 is plausible.** The commit-time atomic evaluation is documented. It still needs an authorized real-R2 probe showing that a presigned PUT with signed `if-none-match` refuses an omitted or changed header and returns 412 over an existing key.
- **GCS is unproven.**
  - The condition's evaluation time for a long single PUT is undocumented.
  - Whether a SigV4-interop signed URL enforces `x-goog-if-generation-match` needs a real probe. A V4 signature may be required instead.
  - An overwrite by the tombstone leaves the previous data soft-deleted, so it can be restored, for the bucket's retention period. Honest erasure then needs soft delete disabled on the upload bucket, or the retention reported. This is a deployment fact (P9).
- **Tombstones make a delete precondition-proof, but only with deletion by overwrite.** With delete-then-create, a conditional late PUT lands in the gap, because generation-match 0 and `If-None-Match: *` both pass on an absent key.
- **Key privacy.** Today's upload key is `{account}/{job}/{file name}` (`sources::upload_key`). The file name is user text, so a permanent tombstone at that key keeps a pseudonymous ID plus possible PII. Tombstones are only non-PII with opaque per-capability keys. That is a key-layout change against the source and needs a recorded decision.
- **Product behavior.** Create-only breaks today's re-upload of the same name over the same draft (UPL-009/011 PUT overwrite), unless each capability gets its own opaque key.
- **Browser.** `If-None-Match` and `x-goog-*` are not CORS-safelisted request headers, so the bucket CORS rules and the web upload must send and allow them (P5 and deployment).
- **Old capabilities.** Unconditional URLs already issued cannot be retrofitted. Their in-flight bound is undocumented on both providers. Rotating or deleting the signing key is documented, for GCS, only as ending the URL's validity, not as cancelling a request already accepted.

## Implication for the FullyErased label

With today's unconditional PUTs, no documented time makes "deleted and absent" final. For the current design, the saga cannot honestly claim `FullyErased` for an account that has any issued upload capability. The candidates are:

- **(a) Unbounded residual risk, stated honestly.** Keep upload re-sweep items open, or close them as an approved exception that names this residual risk. This needs a mu decision, because the contract forbids an invented margin.
- **(b) Adopt the conditional, opaque-key, tombstone design,** after authorized probes on both providers prove the signed-condition enforcement and commit-time evaluation, and after the lead and mu decide on the key layout and on soft delete for the upload bucket.

Pre-rollout capabilities stay under (a) either way. No fixture, and no absent HEAD, can substitute for the probes.
