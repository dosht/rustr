use billing_core::ports::StoreError;
use media_postgres::{HeldLock, LockKey};
use sqlx::PgPool;
use transgate_core::jobs::ports::{BoxFuture, LockGuard, Locks};

/// `Locks` over media-postgres session advisory locks: each held lock owns a
/// connection taken out of the pool, so a crashed holder releases it.
#[derive(Clone)]
pub struct PgLocks {
    pool: PgPool,
}

impl PgLocks {
    pub fn new(pool: PgPool) -> PgLocks {
        PgLocks { pool }
    }
}

struct PgLockGuard(HeldLock);

impl LockGuard for PgLockGuard {
    fn release(self: Box<Self>) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            // A failed unlock closes the connection anyway, which frees the lock.
            let _ = self.0.release().await;
        })
    }
}

impl Locks for PgLocks {
    fn try_lock<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<Box<dyn LockGuard>>, StoreError>> {
        Box::pin(async move {
            let held = media_postgres::try_lock(&self.pool, LockKey::named(name))
                .await
                .map_err(|e| StoreError::Unavailable(e.to_string()))?;
            Ok(held.map(|h| Box::new(PgLockGuard(h)) as Box<dyn LockGuard>))
        })
    }
}
