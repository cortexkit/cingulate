use std::{collections::BTreeMap, time::Duration};

use ck_cingulate::Stamp;
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;

use crate::support::{app_body, stamp, Fixture};

pub const CORPUS: &str = "consent-card-text-v1";

fn flow_install_body() -> Value {
    let mut body = app_body();
    body["kind"] = json!("flow_install");
    body["title"] = json!("Install morning summary");
    body["prompt"] = json!("Allow this scheduled summary flow?");
    body["flow_install"] = json!({"flow":"morning-summary","manifest":{"schedule":"daily"}});
    body["target"] = json!({"kind":"flow","label":"morning-summary"});
    body["scope"] = json!({"flow_id":"morning-summary"});
    // A realistic multi-line script, so the phone's collapsed source block is
    // exercised with the kind of text it will really receive.
    body["source"] = json!({
        "language":"javascript",
        "text":"export default flow({\n  schedule: { at: \"08:00\", tz: \"Europe/Madrid\" },\n  async run(ctx) {\n    const board = await ctx.board.read();\n    await ctx.inbox.send({ title: \"Morning summary\", body: board.digest() });\n  },\n});\n"
    });
    body["dry_run_text"] = json!({
        "summary":"In the last 24 h it would have run once, at 08:00. The run stopped at its first read, because a flow that isn't approved yet doesn't read live data.",
        "would":["Read the agent's board", "Send to the inbox: 'Morning summary'"],
        "problems":[]
    });
    body["facts"] = json!([
        {"label":"Runs","value":"Every day at 08:00"},
        {"label":"Can wake","value":"Yes, at the scheduled time"},
        {"label":"Token limit","value":"5,000 per run"}
    ]);
    body
}

fn package_body() -> Value {
    let request: Value = serde_json::from_slice(include_bytes!(
        "../../../../test-vectors/consent-package-card-v1/request.json"
    ))
    .unwrap();
    let mut body = request["params"].clone();
    body["source"] = json!({
        "language":"javascript",
        "text":"export default flow({\n  schedule: { interval: \"15m\" },\n  async run(ctx) {\n    await ctx.wake({ digest: \"Nudge: check the open asks\" });\n  },\n});\n"
    });
    body["dry_run_text"] = json!({
        "summary":"In the last 24 h it would have run 96 times, every 15 minutes.",
        "would":["Wake its own agent: 'Nudge: check the open asks'"],
        "problems":["The flow's manifest sets no token limit for a wake."]
    });
    body["facts"] = json!([
        {"label":"Runs","value":"Every 15 minutes"},
        {"label":"Can wake","value":"Yes, its own agent"},
        {"label":"Token limit","value":"Not configured"}
    ]);
    body
}

async fn invoke(
    fixture: &Fixture,
    stamp: &Stamp,
    method: &str,
    params: Value,
) -> (Vec<u8>, HandlerOutcome) {
    let request = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_secs(1),
        fixture.handler.handle_stamped(stamp, &request),
    )
    .await
    .expect("card-text handler operation exceeded 1 s");
    (request, outcome)
}

fn successful(outcome: HandlerOutcome, method: &str) -> Vec<u8> {
    match outcome {
        HandlerOutcome::Response(bytes) => bytes,
        other => panic!("{method}: {other:?}"),
    }
}

fn refusal(outcome: HandlerOutcome, method: &str) -> Vec<u8> {
    match outcome {
        HandlerOutcome::Error { code, message } => {
            serde_json::to_vec(&json!({"code":code,"message":message})).unwrap()
        }
        other => panic!("{method}: expected refusal, got {other:?}"),
    }
}

async fn add_card(files: &mut BTreeMap<String, Vec<u8>>, prefix: &str, body: Value) {
    let approved_choice = body["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["effect"] == "grant")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let fixture = Fixture::open(None).await;
    let core = stamp("reserved:prefrontal-core");
    let (request, outcome) = invoke(&fixture, &core, "consent.request", body).await;
    let reply = successful(outcome, "consent.request");
    let id = serde_json::from_slice::<Value>(&reply).unwrap()["result"]["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(id, "el_1");
    files.insert(format!("{prefix}-request.json"), request);
    files.insert(format!("{prefix}-request-reply.json"), reply);

    let phone = crate::phone::phone_stamp();
    let (_, outcome) = invoke(&fixture, &phone, "consent.list_pending", json!({})).await;
    files.insert(
        format!("{prefix}-list-pending.json"),
        successful(outcome, "consent.list_pending"),
    );
    let (_, outcome) = invoke(&fixture, &phone, "consent.get", json!({"elicitationId":id})).await;
    files.insert(
        format!("{prefix}-phone-get.json"),
        successful(outcome, "consent.get"),
    );
    let (_, outcome) = invoke(&fixture, &core, "consent.get", json!({"elicitation_id":id})).await;
    files.insert(
        format!("{prefix}-requester-get.json"),
        successful(outcome, "consent.get"),
    );
    let (_, outcome) = invoke(
        &fixture,
        &phone,
        "consent.answer",
        json!({"elicitationId":id,"choiceId":approved_choice}),
    )
    .await;
    files.insert(
        format!("{prefix}-answer.json"),
        successful(outcome, "consent.answer"),
    );
}

async fn add_refusal(
    files: &mut BTreeMap<String, Vec<u8>>,
    prefix: &str,
    requester: &Stamp,
    body: Value,
) {
    let fixture = Fixture::open(None).await;
    let (request, outcome) = invoke(&fixture, requester, "consent.request", body).await;
    files.insert(format!("{prefix}-request.json"), request);
    files.insert(
        format!("{prefix}-reply.json"),
        refusal(outcome, "consent.request"),
    );
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    add_card(&mut files, "flow-install", flow_install_body()).await;
    add_card(&mut files, "package", package_body()).await;

    let mut consent = app_body();
    consent["dry_run_text"] = json!({"summary":"Nothing to schedule","would":[],"problems":[]});
    add_refusal(
        &mut files,
        "dry-run-text-on-consent",
        &stamp("direct"),
        consent,
    )
    .await;

    let mut over_cap = flow_install_body();
    over_cap["dry_run_text"]["would"] = json!(vec!["Schedule a run"; 9]);
    add_refusal(
        &mut files,
        "would-over-cap",
        &stamp("reserved:prefrontal-core"),
        over_cap,
    )
    .await;
    files
}
