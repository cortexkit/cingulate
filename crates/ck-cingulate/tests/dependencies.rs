fn locked_versions(name: &str) -> Vec<&str> {
    let lock = include_str!("../../../Cargo.lock");
    let name_line = format!("name = \"{name}\"");
    let versions: Vec<_> = lock
        .split("[[package]]")
        .filter(|package| package.lines().any(|line| line == name_line))
        .map(|package| {
            package
                .lines()
                .find_map(|line| {
                    line.strip_prefix("version = \"")
                        .and_then(|v| v.strip_suffix('"'))
                })
                .unwrap()
        })
        .collect();
    assert!(!versions.is_empty(), "{name} missing from Cargo.lock");
    versions
}

#[test]
fn current_subc_releases_are_exactly_pinned_and_locked() {
    for manifest in [
        include_str!("../Cargo.toml"),
        include_str!("../../cingulate-vectors/Cargo.toml"),
    ] {
        for pin in [
            "subc-client-rs = \"=0.41.0\"",
            "subc-protocol = \"=0.37.4\"",
        ] {
            assert!(
                manifest.lines().any(|line| line == pin),
                "missing exact pin {pin}"
            );
        }
    }
    let manifest = include_str!("../Cargo.toml");
    for pin in [
        "subc-daemon = { version = \"=0.49.0\", features = [\"test-support\"] }",
        "subc-transport = \"=0.18.0\"",
        "cortexkit-test-support = \"=0.1.3\"",
    ] {
        assert!(
            manifest.lines().any(|line| line == pin),
            "missing exact pin {pin}"
        );
    }
    assert_eq!(locked_versions("subc-client-rs"), ["0.41.0"]);
    assert_eq!(locked_versions("subc-protocol"), ["0.37.4"]);
    assert_eq!(locked_versions("subc-control"), ["0.41.0"]);
    assert_eq!(locked_versions("subc-daemon"), ["0.49.0"]);
    assert_eq!(locked_versions("subc-transport"), ["0.18.0"]);
    assert_eq!(locked_versions("cortexkit-test-support"), ["0.1.3"]);
}
