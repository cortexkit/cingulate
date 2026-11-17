/// Both daemon module ids identify prefrontal during and after the fleet rename.
pub fn is_prefrontal_module(id: &str) -> bool {
    matches!(id, "prefrontal" | "prefrontal-core")
}

/// Match only reserved route labels, including labels retained in stored rows.
pub fn is_prefrontal_principal(label: &str) -> bool {
    label
        .strip_prefix("reserved:")
        .is_some_and(is_prefrontal_module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefrontal_aliases_require_exact_reserved_labels() {
        for id in ["prefrontal", "prefrontal-core"] {
            assert!(is_prefrontal_module(id));
            assert!(is_prefrontal_principal(&format!("reserved:{id}")));
            assert!(!is_prefrontal_principal(id));
        }
        for label in [
            "direct",
            "reserved:aft",
            "reserved:prefrontal-extra",
            "program:prefrontal",
        ] {
            assert!(!is_prefrontal_principal(label));
            assert!(!is_prefrontal_module(label));
        }
    }
}
