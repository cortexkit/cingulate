use cingulate_decode::ConsentRequest;
use serde_json::{json, Value};

fn body() -> Value {
    json!({"kind":"config_plan","expires_in_ms":300000,"binding":{},"config_plan":{
        "plan_digest":"digest","base_revision":"42","changes":[],
        "apply":{"operation":"rollback","rollback_to":"41","modules":[],"review":null},
        "launch_manifests":[{"module_id":"module","programs":[],"program_changed_since_target":true}]
    }})
}

fn rollback_body() -> Value {
    let mut b = body();
    b["config_plan"]["apply"]["rollback_programs"] = json!([{
        "module_id":"module","input":"bin/tool","approved_then":null,"on_disk_now":{"redacted":true}
    }]);
    b
}

fn invalid(body: Value, message: &str) {
    let error = ConsentRequest::decode(body).unwrap_err();
    assert_eq!(error.code, "elicitation_invalid_request");
    assert!(
        error.message.contains(message),
        "expected {message:?}: {error}"
    );
}

fn preserved(b: Value) {
    let request = ConsentRequest::decode(b.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(request).unwrap()["config_plan"],
        b["config_plan"]
    );
}

#[test]
fn program_flags_allow_each_flag_without_origin_or_program_rows() {
    for flags in [
        json!({"program_changed_since_target":true}),
        json!({"program_changed_since_active":true}),
        json!({"program_changed_since_target":true,"program_changed_since_active":true}),
    ] {
        let mut b = body();
        let mut manifest = flags;
        manifest["module_id"] = json!("module");
        manifest["programs"] = json!([]);
        b["config_plan"]["launch_manifests"] = json!([manifest]);
        preserved(b.clone());
        b["config_plan"]["apply"]["operation"] = json!("change");
        b["config_plan"]["apply"]["rollback_to"] = Value::Null;
        preserved(b);
    }
    let mut b = body();
    b["config_plan"]["launch_manifests"][0] = json!({"module_id":"module","programs":[{
        "input":"bin/tool","old":null,"new":{"redacted":true}
    }]});
    preserved(b);
}

#[test]
fn program_flags_refuse_false_and_every_non_true_type() {
    for flag in [
        "program_changed_since_target",
        "program_changed_since_active",
    ] {
        for value in [
            json!(false),
            Value::Null,
            json!(1),
            json!("true"),
            json!([]),
            json!({}),
        ] {
            let mut b = body();
            b["config_plan"]["launch_manifests"][0]["programs"] = json!([{
                "input":"bin/tool","old":null,"new":{"redacted":true}
            }]);
            b["config_plan"]["launch_manifests"][0][flag] = value;
            invalid(b, flag);
        }
    }
}

#[test]
fn program_flags_require_a_program_or_flag() {
    let mut b = body();
    b["config_plan"]["launch_manifests"][0] =
        json!({"module_id":"module","origin":"project","programs":[]});
    invalid(b, "at least one program or program-change flag");
}

#[test]
fn program_flags_keep_programs_required_and_entry_closed() {
    let mut b = body();
    b["config_plan"]["launch_manifests"][0]
        .as_object_mut()
        .unwrap()
        .remove("programs");
    invalid(b, "config_plan filing has invalid fields");
    let mut b = body();
    b["config_plan"]["launch_manifests"][0]["unexpected"] = json!(true);
    invalid(b, "config_plan filing has invalid fields");
}

#[test]
fn program_flags_optional_origin_still_validates_when_present() {
    for value in [Value::Null, json!(false), json!(""), json!("é".repeat(33))] {
        let mut b = body();
        b["config_plan"]["launch_manifests"][0]["origin"] = value;
        invalid(b, "origin");
    }
    let mut b = body();
    b["config_plan"]["launch_manifests"][0]["origin"] = json!("é".repeat(32));
    preserved(b);
}

#[test]
fn rollback_programs_only_allowed_for_rollback_even_when_empty() {
    for rows in [
        json!([]),
        rollback_body()["config_plan"]["apply"]["rollback_programs"].clone(),
    ] {
        let mut b = body();
        b["config_plan"]["apply"]["operation"] = json!("change");
        b["config_plan"]["apply"]["rollback_to"] = Value::Null;
        b["config_plan"]["apply"]["rollback_programs"] = rows;
        invalid(b, "rollback_programs is only allowed for rollback");
    }
}

#[test]
fn rollback_programs_requires_an_array_when_present() {
    for value in [Value::Null, json!({}), json!(false), json!("program")] {
        let mut b = body();
        b["config_plan"]["apply"]["rollback_programs"] = value;
        invalid(b, "rollback_programs");
    }
}

#[test]
fn rollback_programs_has_zero_to_64_rows() {
    for count in [0, 64, 65] {
        let mut b = rollback_body();
        let row = b["config_plan"]["apply"]["rollback_programs"][0].clone();
        b["config_plan"]["apply"]["rollback_programs"] = json!(vec![row; count]);
        if count <= 64 {
            preserved(b);
        } else {
            invalid(b, "rollback_programs");
        }
    }
}

#[test]
fn rollback_programs_rows_are_closed_and_all_members_required() {
    let original = rollback_body();
    for field in ["module_id", "input", "approved_then", "on_disk_now"] {
        let mut b = original.clone();
        b["config_plan"]["apply"]["rollback_programs"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        invalid(b, "config_plan filing has invalid fields");
    }
    for field in ["reason", "unexpected"] {
        let mut b = original.clone();
        b["config_plan"]["apply"]["rollback_programs"][0][field] = json!("Not part of the card");
        invalid(b, "config_plan filing has invalid fields");
    }
}

#[test]
fn rollback_programs_text_bounds() {
    for (field, max) in [("module_id", 128), ("input", 1024)] {
        for value in [json!(""), json!("é".repeat(max / 2 + 1))] {
            let mut b = rollback_body();
            b["config_plan"]["apply"]["rollback_programs"][0][field] = value;
            invalid(b, field);
        }
        let mut b = rollback_body();
        b["config_plan"]["apply"]["rollback_programs"][0][field] = json!("é".repeat(max / 2));
        preserved(b);
    }
}

#[test]
fn rollback_programs_values_preserve_json_with_4096_byte_caps() {
    for field in ["approved_then", "on_disk_now"] {
        let mut b = rollback_body();
        b["config_plan"]["apply"]["rollback_programs"][0][field] = json!("x".repeat(4095));
        invalid(b, field);
        for value in [
            Value::Null,
            json!({"redacted":true}),
            json!(false),
            json!([1, "value"]),
            json!("x".repeat(4094)),
        ] {
            let mut b = rollback_body();
            b["config_plan"]["apply"]["rollback_programs"][0][field] = value;
            preserved(b);
        }
    }
}

#[test]
fn route_rebind_is_a_closed_module_transition_not_next_supervision() {
    let mut b = body();
    b["config_plan"]["apply"]["modules"] = json!([{
        "module_id":"module","transition":"route_rebind","whole_generation":false,
        "waits_for_next_start":false,"waits_for_daemon_restart":false,
        "deferred_live_fields":[],"explanation":"Open connections close and reopen."
    }]);
    preserved(b.clone());
    for transition in ["next_supervision", "future"] {
        b["config_plan"]["apply"]["modules"][0]["transition"] = json!(transition);
        invalid(b.clone(), "transition");
    }
}
