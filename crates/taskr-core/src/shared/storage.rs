//! Errors and concurrency tokens shared by every repository port.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageError(pub String);

/// Optimistic concurrency token. A repository hands it out on load and refuses
/// a save whose expected revision is no longer the stored one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(pub u64);

impl Revision {
    pub const NEW: Revision = Revision(0);

    pub fn next(self) -> Revision {
        Revision(self.0 + 1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    /// Someone else saved since we loaded. Reload and try again.
    Conflict,
    Failed(StorageError),
}

impl From<StorageError> for SaveError {
    fn from(e: StorageError) -> Self {
        SaveError::Failed(e)
    }
}

/// How many times a use case reloads and re-applies a change after a conflict.
pub const MAX_ATTEMPTS: usize = 3;
