use std::path::PathBuf;
use std::process::{Command, Output};

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("taskr-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn taskr(home: &PathBuf, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taskr"))
        .arg("--home")
        .arg(home)
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn add_list_done_flow() {
    let h = home("flow");
    let o = taskr(&h, &["add", "Buy", "milk"]);
    assert!(o.status.success());
    assert_eq!(stdout(&o), "Added #1 Buy milk\n");

    taskr(&h, &["add", "Call mom"]);
    let list = stdout(&taskr(&h, &["list"]));
    assert!(list.starts_with("[Open (2)]  Done (0)  All (2)\n"), "{list}");
    assert!(list.contains("ID  TITLE"), "{list}");
    assert!(list.contains("[ ]  1  Buy milk"), "{list}");

    assert_eq!(stdout(&taskr(&h, &["done", "1"])), "Done #1 Buy milk\n");
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&taskr(&h, &["list", "--scope", "done", "--json"]))).unwrap();
    assert_eq!(json[0]["title"], "Buy milk");
    assert_eq!(json[0]["done"], true);
}

#[test]
fn rejections_use_exit_code_1_and_shared_messages() {
    let h = home("reject");
    taskr(&h, &["add", "Buy milk"]);
    let o = taskr(&h, &["add", "buy MILK"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&o.stderr),
        "taskr: There is already an open task with that title.\n"
    );
    assert_eq!(taskr(&h, &["done", "42"]).status.code(), Some(1));
}

#[test]
fn verbose_prints_telemetry_to_stderr() {
    let h = home("verbose");
    let o = taskr(&h, &["-v", "add", "x"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("\"event\":\"task.added\""), "{err}");
}

#[test]
fn corrupt_storage_uses_exit_code_3() {
    let h = home("corrupt");
    std::fs::create_dir_all(&h).unwrap();
    std::fs::write(h.join("tasks.json"), "nope").unwrap();
    assert_eq!(taskr(&h, &["add", "x"]).status.code(), Some(3));
}
