use std::process::Command;

fn boy(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_boy"))
        .args(args)
        .output()
        .expect("failed to run boy")
}

#[test]
fn no_arguments_prints_the_five_commands() {
    let out = boy(&[]);
    assert!(out.status.success());

    let text = String::from_utf8(out.stdout).unwrap();
    for cmd in ["i <name>", "r <name>", "l ", "s [query]", "u [name]"] {
        assert!(text.contains(cmd), "missing `{cmd}` in:\n{text}");
    }
}

#[test]
fn unknown_command_fails_with_help() {
    let out = boy(&["x"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("unknown command")
    );
}
