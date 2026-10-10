use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};
use taskr_core::shared::clock::Clock;
use taskr_core::shared::telemetry::{Level, LogEvent, Telemetry};

/// Writes one JSON object per event. The CLI points it at stderr, the TUI at a
/// file (stderr would corrupt the terminal screen).
pub struct JsonLinesTelemetry {
    out: Mutex<Box<dyn Write + Send>>,
    min_level: Level,
    clock: Arc<dyn Clock>,
}

impl JsonLinesTelemetry {
    pub fn new(out: Box<dyn Write + Send>, min_level: Level, clock: Arc<dyn Clock>) -> Self {
        Self { out: Mutex::new(out), min_level, clock }
    }

    pub fn stderr(min_level: Level, clock: Arc<dyn Clock>) -> Self {
        Self::new(Box::new(io::stderr()), min_level, clock)
    }

    pub fn append_to(path: &Path, min_level: Level, clock: Arc<dyn Clock>) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self::new(Box::new(file), min_level, clock))
    }
}

pub fn to_json(ts: i64, event: &LogEvent) -> String {
    let mut obj = Map::new();
    obj.insert("ts".into(), ts.into());
    obj.insert("level".into(), event.level.as_str().into());
    obj.insert("event".into(), event.name.into());
    for (k, v) in &event.fields {
        obj.insert((*k).into(), Value::String(v.clone()));
    }
    Value::Object(obj).to_string()
}

impl Telemetry for JsonLinesTelemetry {
    fn record(&self, event: LogEvent) {
        if event.level < self.min_level {
            return;
        }
        let line = to_json(self.clock.now(), &event);
        if let Ok(mut out) = self.out.lock() {
            let _ = writeln!(out, "{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taskr_core::testing::FixedClock;

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);

    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn writes_filtered_json_lines() {
        let buf = Buf::default();
        let t = JsonLinesTelemetry::new(
            Box::new(buf.clone()),
            Level::Info,
            Arc::new(FixedClock::at(5)),
        );
        t.record(LogEvent::new(Level::Debug, "noise"));
        t.record(LogEvent::info("task.added").field("id", 3));
        let text = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(text, "{\"event\":\"task.added\",\"id\":\"3\",\"level\":\"info\",\"ts\":5}\n");
    }
}
