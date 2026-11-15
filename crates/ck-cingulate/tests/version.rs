use std::process::Command;

#[test]
fn version_reports_local_operator_feature_state_without_daemon() {
    let output = Command::new(env!("CARGO_BIN_EXE_ck-cingulate"))
        .arg("--version")
        .env_remove("SUBC_LAUNCH_NONCE")
        .output()
        .expect("run ck-cingulate --version");
    assert!(output.status.success(), "{output:?}");
    let state = if cfg!(feature = "dev-local-operator") {
        "on"
    } else {
        "off"
    };
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!(
            "ck-cingulate {} (dev-local-operator: {state})",
            env!("CARGO_PKG_VERSION")
        )
    );
}
