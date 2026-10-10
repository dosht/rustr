//! Cross-cutting telemetry port. The core records what happened; each shell
//! decides where it goes (stderr, a log file, the browser console, logcat, os_log).

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEvent {
    pub level: Level,
    pub name: &'static str,
    pub fields: Vec<(&'static str, String)>,
}

impl LogEvent {
    pub fn new(level: Level, name: &'static str) -> Self {
        Self { level, name, fields: Vec::new() }
    }

    pub fn info(name: &'static str) -> Self {
        Self::new(Level::Info, name)
    }

    pub fn warn(name: &'static str) -> Self {
        Self::new(Level::Warn, name)
    }

    pub fn error(name: &'static str) -> Self {
        Self::new(Level::Error, name)
    }

    pub fn field(mut self, key: &'static str, value: impl ToString) -> Self {
        self.fields.push((key, value.to_string()));
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }
}

pub trait Telemetry: Send + Sync {
    fn record(&self, event: LogEvent);
}

pub struct NoTelemetry;

impl Telemetry for NoTelemetry {
    fn record(&self, _event: LogEvent) {}
}
