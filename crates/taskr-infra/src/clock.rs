use std::time::{SystemTime, UNIX_EPOCH};

use taskr_core::shared::clock::{Clock, Millis};

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Millis {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as Millis).unwrap_or(0)
    }
}
