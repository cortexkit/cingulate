fn locked_version(name: &str) -> &str {
    let lock = include_str!("../../../Cargo.lock");
    let name_line = format!("name = \"{name}\"");
    let package = lock
        .split("[[package]]")
        .find(|package| package.lines().any(|line| line == name_line))
        .unwrap_or_else(|| panic!("{name} missing from Cargo.lock"));
    package
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|v| v.strip_suffix('"'))
        })
        .unwrap()
}

#[test]
fn current_subc_releases_are_exactly_pinned_and_locked() {
    for manifest in [
        include_str!("../Cargo.toml"),
        include_str!("../../cingulate-vectors/Cargo.toml"),
    ] {
        for pin in [
            "subc-client-rs = \"=0.34.3\"",
            "subc-protocol = \"=0.34.5\"",
        ] {
            assert!(
                manifest.lines().any(|line| line == pin),
                "missing exact pin {pin}"
            );
        }
    }
    let manifest = include_str!("../Cargo.toml");
    for pin in [
        "subc-daemon = { version = \"=0.39.0\", features = [\"test-support\"] }",
        "subc-transport = \"=0.15.0\"",
        "cortexkit-test-support = \"=0.1.3\"",
    ] {
        assert!(
            manifest.lines().any(|line| line == pin),
            "missing exact pin {pin}"
        );
    }
    assert_eq!(locked_version("subc-client-rs"), "0.34.3");
    assert_eq!(locked_version("subc-protocol"), "0.34.5");
    // The subc-daemon 0.39.0 fixture also locks subc-control 0.34.0;
    // the SDK uses subc-control 0.35.1 alongside it.
    assert!(include_str!("../../../Cargo.lock")
        .contains("name = \"subc-control\"\nversion = \"0.35.1\""));
    assert_eq!(locked_version("subc-daemon"), "0.39.0");
    assert_eq!(locked_version("subc-transport"), "0.15.0");
    assert_eq!(locked_version("cortexkit-test-support"), "0.1.3");
}
