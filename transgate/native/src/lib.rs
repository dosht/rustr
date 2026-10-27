//! What the terminal shells share (P8 D5, D7, D8): the session vault, the loopback
//! redirect listener, the `NativePorts` a terminal can provide, and a handle that
//! waits on the presenter's views. The CLI and the TUI render the core view models
//! and forward intents; nothing here decides a product rule or carries copy.
pub mod browser;
pub mod loopback;
pub mod ports;
pub mod shell;
pub mod vault;

pub use loopback::Loopback;
pub use ports::{OpenUrl, TerminalPorts, local_offsets};
pub use shell::{HostConfig, Shell, Signal};
pub use vault::Vault;
