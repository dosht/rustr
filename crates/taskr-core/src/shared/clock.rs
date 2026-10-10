/// Milliseconds since the Unix epoch, UTC.
pub type Millis = i64;

pub trait Clock: Send + Sync {
    fn now(&self) -> Millis;
}
