//! Tests cross the real process boundary, including exit status and streams.

use std::process::{Command, Output};

fn atelier(arguments: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_atelier"))
        .args(arguments)
        .output()
}

#[test]
fn version_is_the_workspace_version() {
    for argument in ["--version", "-V"] {
        let output = atelier(&[argument]).expect("atelier starts");
        assert!(output.status.success());
        assert_eq!(
            output.stdout,
            format!("atelier {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
        );
        assert_eq!(output.stderr, Vec::<u8>::new());
    }
}

#[test]
fn help_is_successful_and_describes_the_binary() {
    let output = atelier(&["--help"]).expect("atelier starts");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(text.contains("A personal software factory"));
    assert!(text.contains("--version"));
    assert_eq!(output.stderr, Vec::<u8>::new());
}

#[test]
fn invalid_arguments_and_missing_command_fail() {
    for arguments in [&["--unknown"][..], &["daemon"][..], &[][..]] {
        let output = atelier(arguments).expect("atelier starts");
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(output.stdout, Vec::<u8>::new());
        assert_ne!(output.stderr, Vec::<u8>::new());
    }
}
