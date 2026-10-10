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
            "subc-client-rs = \"=0.27.0\"",
            "subc-protocol = \"=0.30.0\"",
        ] {
            assert!(
                manifest.lines().any(|line| line == pin),
                "missing exact pin {pin}"
            );
        }
    }
    let manifest = include_str!("../Cargo.toml");
    for pin in [
        "subc-daemon = { version = \"=0.33.0\", features = [\"test-support\"] }",
        "cortexkit-test-support = \"=0.1.3\"",
    ] {
        assert!(
            manifest.lines().any(|line| line == pin),
            "missing exact pin {pin}"
        );
    }
    assert_eq!(locked_version("subc-client-rs"), "0.27.0");
    assert_eq!(locked_version("subc-protocol"), "0.30.0");
    assert_eq!(locked_version("subc-daemon"), "0.33.0");
    assert_eq!(locked_version("cortexkit-test-support"), "0.1.3");
}
