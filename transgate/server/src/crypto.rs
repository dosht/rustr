//! The host's [`ApiCrypto`] (P7 D2/D5): what the core may not link. SHA-256 and
//! HMAC-SHA256 for key digests and callback signatures, a CSPRNG token for new
//! keys, and bcrypt verification of migrated source keys (pgcrypto `$2a$`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use transgate_core::api::ports::ApiCrypto;
use transgate_core::jobs::ports::BoxFuture;

#[derive(Clone, Copy, Debug, Default)]
pub struct HostCrypto;

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

impl ApiCrypto for HostCrypto {
    fn random_token(&self) -> String {
        let mut bytes = [0u8; 32];
        // No key may be minted from a weak or absent entropy source.
        getrandom::fill(&mut bytes).expect("the operating system random source is unavailable");
        STANDARD.encode(bytes)
    }

    fn sha256_hex(&self, data: &[u8]) -> String {
        hex(&Sha256::digest(data))
    }

    fn hmac_sha256_hex(&self, secret: &[u8], message: &[u8]) -> String {
        // HMAC accepts a key of any length, so construction cannot fail.
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
        mac.update(message);
        hex(&mac.finalize().into_bytes())
    }

    fn verify_legacy<'a>(&'a self, key: &'a str, bcrypt_hash: &'a str) -> BoxFuture<'a, bool> {
        let (key, hash) = (key.to_owned(), bcrypt_hash.to_owned());
        // bcrypt is deliberately slow: keep it off the async workers. A malformed
        // hash or a join failure is a refusal, never a pass.
        Box::pin(async move { tokio::task::spawn_blocking(move || bcrypt::verify(key, &hash).unwrap_or(false)).await.unwrap_or(false) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transgate_core::api::keys::well_formed;

    // RFC 4231 test case 2 and the FIPS 180 "abc" vector.
    #[test]
    fn digests_match_their_published_vectors() {
        let crypto = HostCrypto;
        assert_eq!(crypto.sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            crypto.hmac_sha256_hex(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // A key longer than the block size is hashed first (RFC 4231 case 6).
        assert_eq!(
            crypto.hmac_sha256_hex(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First"),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn a_new_token_is_a_fresh_api_002_key_body() {
        let crypto = HostCrypto;
        let (first, second) = (crypto.random_token(), crypto.random_token());
        assert_eq!(first.len(), 44);
        assert!(well_formed(&format!("sk_{first}")), "{first}");
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn legacy_verification_refuses_malformed_hashes_and_wrong_keys() {
        let crypto = HostCrypto;
        let hash = bcrypt::hash_with_result("sk_example", 4).unwrap().format_for_version(bcrypt::Version::TwoA);
        assert!(hash.starts_with("$2a$04$"), "{hash}");
        assert!(crypto.verify_legacy("sk_example", &hash).await);
        assert!(!crypto.verify_legacy("sk_other", &hash).await);
        for malformed in ["", "not a hash", "$2a$99$short", "$2a$04$"] {
            assert!(!crypto.verify_legacy("sk_example", malformed).await, "{malformed:?}");
        }
    }

    // D2: the source keys were hashed by pgcrypto `crypt(key, gen_salt('bf'))`. The pinned
    // bcrypt version must verify exactly those hashes, whatever the cost and the characters.
    #[tokio::test]
    #[ignore = "needs TRANSGATE_SERVER_TEST_DATABASE_URL"]
    async fn bcrypt_verifies_hashes_generated_by_pgcrypto() {
        let url = std::env::var("TRANSGATE_SERVER_TEST_DATABASE_URL")
            .expect("TRANSGATE_SERVER_TEST_DATABASE_URL must name a disposable PostgreSQL database for these tests");
        let owner = billing_postgres::connect_owner(&url).await.expect("owner connection");
        sqlx::query("CREATE EXTENSION IF NOT EXISTS pgcrypto").execute(&owner).await.expect("pgcrypto is available");
        let crypto = HostCrypto;
        let keys = [format!("sk_{}", crypto.random_token()), "sk_pässwörd-ключ-密钥".to_string(), "sk_".to_string()];
        for key in &keys {
            for cost in [4_i32, 6, 10] {
                let hash: String = sqlx::query_scalar("SELECT crypt($1, gen_salt('bf', $2))")
                    .bind(key)
                    .bind(cost)
                    .fetch_one(&owner)
                    .await
                    .unwrap();
                assert!(hash.starts_with("$2a$"), "pgcrypto produced {hash}");
                assert!(crypto.verify_legacy(key, &hash).await, "{key:?} at cost {cost}: {hash}");
                assert!(!crypto.verify_legacy(&format!("{key}x"), &hash).await, "a near miss must not verify");
                // The database agrees with itself: this is the source's own comparison.
                let source_check: bool = sqlx::query_scalar("SELECT crypt($1, $2) = $2").bind(key).bind(&hash).fetch_one(&owner).await.unwrap();
                assert!(source_check);
            }
        }
        owner.close().await;
    }
}
