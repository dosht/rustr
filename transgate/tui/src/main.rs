fn main() -> std::process::ExitCode {
    transgate_tui::run::run(std::env::args().skip(1), |key| std::env::var(key).ok())
}
