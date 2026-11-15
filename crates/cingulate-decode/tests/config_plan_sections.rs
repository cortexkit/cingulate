use cingulate_decode::ConsentRequest;
use serde_json::{json, Value};

fn body() -> Value {
    json!({"kind":"config_plan","expires_in_ms":300000,"binding":{},"config_plan":{
        "plan_digest":"digest","base_revision":"42",
        "apply":{"operation":"change","rollback_to":null,"modules":[{
            "module_id":"module","transition":"live","whole_generation":false,
            "waits_for_next_start":false,"waits_for_daemon_restart":false,
            "deferred_live_fields":["settings.enabled"],"explanation":"Applies immediately."
        }],"review":{"supervisor_reconcile":true}},
        "affected_projects":["project"],
        "changes":[{"module_id":"module","path":"settings.enabled","old":false,"new":true,
            "old_source":"default","new_source":"project","apply":"live"}],
        "policy_changes":[{"kind":"declaration","module_id":"module","old":null,"new":{"redacted":true},"reason":"New policy"}],
        "launch_manifests":[{"module_id":"module","origin":"project","programs":[{
            "input":"bin/tool","old":null,"new":{"redacted":true}
        }]}]
    }})
}

fn invalid(body: Value, message: &str) {
    let error = ConsentRequest::decode(body).unwrap_err();
    assert_eq!(error.code, "elicitation_invalid_request");
    assert!(
        error.message.contains(message),
        "expected {message:?}: {error}"
    );
}

fn set_invalid(pointer: &str, values: impl IntoIterator<Item = Value>, message: &str) {
    for value in values {
        let mut b = body();
        *b.pointer_mut(pointer).unwrap() = value;
        invalid(b, message);
    }
}

#[test]
fn typed_sections_required_apply_and_nonempty_plan() {
    let mut b = body();
    for field in [
        "apply",
        "affected_projects",
        "policy_changes",
        "launch_manifests",
    ] {
        b["config_plan"].as_object_mut().unwrap().remove(field);
    }
    invalid(b, "config_plan.apply");
    set_invalid("/config_plan/apply", [Value::Null], "config_plan.apply");
    let mut b = body();
    b["config_plan"]["changes"] = json!([]);
    b["config_plan"]["policy_changes"] = json!([]);
    b["config_plan"]["launch_manifests"] = json!([]);
    invalid(b, "at least one row");
}

#[test]
fn typed_sections_closed_shapes_and_required_members() {
    for pointer in [
        "/config_plan/apply",
        "/config_plan/apply/modules/0",
        "/config_plan/apply/review",
        "/config_plan/policy_changes/0",
        "/config_plan/launch_manifests/0",
        "/config_plan/launch_manifests/0/programs/0",
    ] {
        let original = body();
        let mut b = original.clone();
        b.pointer_mut(pointer).unwrap()["unexpected"] = json!(true);
        invalid(b, "unexpected");
        for field in original
            .pointer(pointer)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
        {
            if ["module_id", "reason"].contains(&field.as_str())
                && pointer.ends_with("policy_changes/0")
            {
                continue;
            }
            let mut b = original.clone();
            b.pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            invalid(b, field);
        }
    }
    for pointer in [
        "/config_plan/affected_projects",
        "/config_plan/policy_changes",
        "/config_plan/launch_manifests",
    ] {
        set_invalid(
            pointer,
            [Value::Null, json!({})],
            pointer.trim_start_matches('/').replace('/', ".").as_str(),
        );
    }
    for pointer in [
        "/config_plan/policy_changes/0/module_id",
        "/config_plan/policy_changes/0/reason",
    ] {
        set_invalid(pointer, [Value::Null], pointer.rsplit('/').next().unwrap());
    }
}

#[test]
fn typed_sections_apply_enums_flags_and_rollback() {
    set_invalid(
        "/config_plan/apply/operation",
        [json!("future"), Value::Null, json!(1)],
        "operation",
    );
    set_invalid(
        "/config_plan/apply/modules/0/transition",
        [json!("future"), Value::Null, json!(1)],
        "transition",
    );
    for flag in [
        "whole_generation",
        "waits_for_next_start",
        "waits_for_daemon_restart",
    ] {
        set_invalid(
            &format!("/config_plan/apply/modules/0/{flag}"),
            [Value::Null, json!("false"), json!(0)],
            flag,
        );
    }
    set_invalid(
        "/config_plan/apply/review/supervisor_reconcile",
        [json!(false), Value::Null, json!(1)],
        "supervisor_reconcile",
    );
    set_invalid(
        "/config_plan/apply/rollback_to",
        [json!("42"), json!(""), json!(42)],
        "rollback_to",
    );
    for value in [
        Value::Null,
        json!(""),
        json!("-1"),
        json!("1.0"),
        json!(" 1"),
        json!("１"),
    ] {
        let mut b = body();
        b["config_plan"]["apply"]["operation"] = json!("rollback");
        b["config_plan"]["apply"]["rollback_to"] = value;
        invalid(b, "rollback_to");
    }
    for transition in ["live", "module_restart", "next_start", "daemon_restart"] {
        let mut b = body();
        b["config_plan"]["apply"]["modules"][0]["transition"] = json!(transition);
        b["config_plan"]["apply"]["operation"] = json!("rollback");
        b["config_plan"]["apply"]["rollback_to"] = json!("00042");
        b["config_plan"]["apply"]["review"] = Value::Null;
        ConsentRequest::decode(b).unwrap();
    }
}

#[test]
fn typed_sections_list_caps_and_uniqueness() {
    for (pointer, cap, field) in [
        ("/config_plan/changes", 64, "changes"),
        ("/config_plan/apply/modules", 64, "apply.modules"),
        (
            "/config_plan/apply/modules/0/deferred_live_fields",
            64,
            "deferred_live_fields",
        ),
        ("/config_plan/affected_projects", 64, "affected_projects"),
        ("/config_plan/policy_changes", 64, "policy_changes"),
        ("/config_plan/launch_manifests", 64, "launch_manifests"),
        ("/config_plan/launch_manifests/0/programs", 16, "programs"),
    ] {
        let mut b = body();
        let row = b.pointer(pointer).unwrap()[0].clone();
        *b.pointer_mut(pointer).unwrap() = json!(vec![row; cap + 1]);
        invalid(b, field);
    }
    set_invalid(
        "/config_plan/launch_manifests/0/programs",
        [json!([])],
        "programs",
    );
    for (pointer, field) in [
        ("/config_plan/apply/modules", "apply.modules.module_id"),
        ("/config_plan/affected_projects", "affected_projects"),
        (
            "/config_plan/launch_manifests",
            "launch_manifests.module_id",
        ),
    ] {
        let mut b = body();
        let row = b.pointer(pointer).unwrap()[0].clone();
        *b.pointer_mut(pointer).unwrap() = json!([row, row]);
        invalid(b, field);
    }
}

#[test]
fn typed_sections_list_boundaries_and_optional_omission() {
    for size in [0, 64] {
        let mut b = body();
        let module = b["config_plan"]["apply"]["modules"][0].clone();
        b["config_plan"]["apply"]["modules"] = json!((0..size)
            .map(|i| {
                let mut row = module.clone();
                row["module_id"] = json!(format!("module-{i}"));
                row["deferred_live_fields"] = json!(vec!["settings.enabled"; size]);
                row
            })
            .collect::<Vec<_>>());
        b["config_plan"]["affected_projects"] = json!((0..size)
            .map(|i| format!("project-{i}"))
            .collect::<Vec<_>>());
        b["config_plan"]["policy_changes"] =
            json!(vec![b["config_plan"]["policy_changes"][0].clone(); size]);
        let manifest = b["config_plan"]["launch_manifests"][0].clone();
        b["config_plan"]["launch_manifests"] = json!((0..size)
            .map(|i| {
                let mut row = manifest.clone();
                row["module_id"] = json!(format!("module-{i}"));
                row["programs"] = json!(vec![row["programs"][0].clone(); 16]);
                row
            })
            .collect::<Vec<_>>());
        b["config_plan"]["changes"] = json!(vec![b["config_plan"]["changes"][0].clone(); 64]);
        ConsentRequest::decode(b).unwrap();
    }
    let mut b = body();
    for field in ["affected_projects", "policy_changes", "launch_manifests"] {
        b["config_plan"].as_object_mut().unwrap().remove(field);
    }
    ConsentRequest::decode(b).unwrap();
}

#[test]
fn typed_sections_text_byte_bounds() {
    for (pointer, max, field) in [
        (
            "/config_plan/apply/modules/0/module_id",
            128,
            "apply.modules.module_id",
        ),
        (
            "/config_plan/apply/modules/0/deferred_live_fields/0",
            1024,
            "deferred_live_fields",
        ),
        (
            "/config_plan/apply/modules/0/explanation",
            512,
            "explanation",
        ),
        ("/config_plan/affected_projects/0", 128, "affected_projects"),
        (
            "/config_plan/policy_changes/0/module_id",
            128,
            "policy_changes.module_id",
        ),
        (
            "/config_plan/launch_manifests/0/module_id",
            128,
            "launch_manifests.module_id",
        ),
        ("/config_plan/launch_manifests/0/origin", 64, "origin"),
        (
            "/config_plan/launch_manifests/0/programs/0/input",
            1024,
            "programs.input",
        ),
    ] {
        set_invalid(pointer, [json!(""), json!("é".repeat(max / 2 + 1))], field);
        let mut b = body();
        *b.pointer_mut(pointer).unwrap() = json!("é".repeat(max / 2));
        ConsentRequest::decode(b).unwrap();
    }
    set_invalid(
        "/config_plan/policy_changes/0/reason",
        [json!("x".repeat(513))],
        "reason",
    );
}

#[test]
fn typed_sections_explanation_is_plain_text_with_newlines() {
    for control in (0..=0x1f).chain(0x7f..=0x9f) {
        let character = char::from_u32(control).unwrap();
        let mut b = body();
        b["config_plan"]["apply"]["modules"][0]["explanation"] =
            json!(format!("text{character}text"));
        if character == '\n' {
            ConsentRequest::decode(b).unwrap();
        } else {
            invalid(b, "apply.modules.explanation");
        }
    }
    let mut b = body();
    b["config_plan"]["apply"]["modules"][0]["explanation"] =
        json!("<b>literal</b> **text**, not rich content");
    ConsentRequest::decode(b).unwrap();
}

#[test]
fn typed_sections_policy_kind_module_rules() {
    set_invalid(
        "/config_plan/policy_changes/0/kind",
        [json!("future"), Value::Null, json!(1)],
        "kind",
    );
    for kind in ["declaration", "reviewed_program"] {
        let mut b = body();
        b["config_plan"]["policy_changes"][0]["kind"] = json!(kind);
        b["config_plan"]["policy_changes"][0]
            .as_object_mut()
            .unwrap()
            .remove("module_id");
        invalid(b, "policy_changes.module_id");
    }
    let mut b = body();
    b["config_plan"]["policy_changes"][0]["kind"] = json!("subc_jsonc");
    invalid(b.clone(), "policy_changes.module_id");
    b["config_plan"]["policy_changes"][0]
        .as_object_mut()
        .unwrap()
        .remove("module_id");
    ConsentRequest::decode(b).unwrap();
}

#[test]
fn typed_sections_values_preserve_json_and_redaction_with_size_caps() {
    for section in [
        "/config_plan/policy_changes/0",
        "/config_plan/launch_manifests/0/programs/0",
    ] {
        for field in ["old", "new"] {
            let pointer = format!("{section}/{field}");
            set_invalid(
                &pointer,
                [json!("x".repeat(4095))],
                &format!(
                    "{}.{field}",
                    if section.contains("policy_changes") {
                        "policy_changes"
                    } else {
                        "programs"
                    }
                ),
            );
            for value in [
                Value::Null,
                json!({"redacted":true}),
                json!(false),
                json!([1, "value"]),
                json!("x".repeat(4094)),
            ] {
                let mut b = body();
                *b.pointer_mut(&pointer).unwrap() = value;
                let request = ConsentRequest::decode(b.clone()).unwrap();
                assert_eq!(
                    serde_json::to_value(request).unwrap()["config_plan"],
                    b["config_plan"]
                );
            }
        }
    }
}

#[test]
fn typed_sections_zero_change_policy_and_program_only_plans() {
    for section in ["policy_changes", "launch_manifests"] {
        let mut b = body();
        b["config_plan"]["changes"] = json!([]);
        let other = if section == "policy_changes" {
            "launch_manifests"
        } else {
            "policy_changes"
        };
        b["config_plan"].as_object_mut().unwrap().remove(other);
        ConsentRequest::decode(b).unwrap();
    }
}

fn layer_file_body() -> Value {
    let mut b = body();
    b["config_plan"]["changes"] = json!([]);
    b["config_plan"]
        .as_object_mut()
        .unwrap()
        .remove("launch_manifests");
    b["config_plan"]["policy_changes"] = json!([{
        "kind":"layer_file", "module_id":"module", "layer":"user",
        "old":{"absent":true}, "new":{"sha256":"a".repeat(64),"bytes":42}
    }]);
    b
}

#[test]
fn typed_sections_layer_file_only_plan_preserves_file_facts() {
    for value in [
        json!({"absent":true}),
        json!({"sha256":"0123456789abcdef".repeat(4),"bytes":0}),
        json!({"sha256":"a".repeat(64),"bytes":u64::MAX}),
    ] {
        let mut b = layer_file_body();
        b["config_plan"]["policy_changes"][0]["layer"] = json!("é".repeat(32));
        b["config_plan"]["policy_changes"][0]["old"] = value.clone();
        b["config_plan"]["policy_changes"][0]["new"] = value;
        let request = ConsentRequest::decode(b.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(request).unwrap()["config_plan"],
            b["config_plan"]
        );
    }
}

#[test]
fn typed_sections_layer_file_closed_facts_and_conditional_members() {
    for field in ["old", "new"] {
        for value in [
            Value::Null,
            json!(false),
            json!({}),
            json!({"absent":false}),
            json!({"absent":true,"extra":1}),
            json!({"absent":true,"sha256":"a".repeat(64),"bytes":0}),
            json!({"sha256":"A".repeat(64),"bytes":0}),
            json!({"sha256":"a".repeat(63),"bytes":0}),
            json!({"sha256":"g".repeat(64),"bytes":0}),
            json!({"sha256":"a".repeat(64)}),
            json!({"sha256":"a".repeat(64),"bytes":-1}),
            json!({"sha256":"a".repeat(64),"bytes":1.5}),
            json!({"sha256":"a".repeat(64),"bytes":1,"extra":true}),
        ] {
            let mut b = layer_file_body();
            b["config_plan"]["policy_changes"][0][field] = value;
            invalid(b, &format!("policy_changes.{field}"));
        }
    }
    for field in ["module_id", "layer"] {
        let mut b = layer_file_body();
        b["config_plan"]["policy_changes"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        invalid(b, field);
    }
    for layer in [json!(""), json!("é".repeat(33)), Value::Null] {
        let mut b = layer_file_body();
        b["config_plan"]["policy_changes"][0]["layer"] = layer;
        invalid(b, "layer");
    }
    let mut b = layer_file_body();
    b["config_plan"]["policy_changes"][0]["reason"] = json!("");
    invalid(b, "reason");
    for kind in ["declaration", "subc_jsonc", "reviewed_program"] {
        let mut b = body();
        b["config_plan"]["policy_changes"][0]["kind"] = json!(kind);
        b["config_plan"]["policy_changes"][0]["layer"] = json!("user");
        if kind == "subc_jsonc" {
            b["config_plan"]["policy_changes"][0]
                .as_object_mut()
                .unwrap()
                .remove("module_id");
        }
        invalid(b, "layer");
    }
}
