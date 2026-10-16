//! Check consent request JSON files against the same decoder Cingulate runs
//! on every `consent.request`, without a daemon.
//!
//! Usage: `cargo run -p cingulate-decode --example check_consent_request -- <file-or-dir>...`
//!
//! A file may hold the bare request params or the whole `{"method", "params"}`
//! envelope. A directory is read one level deep, `*.json` files only. Envelopes
//! whose method is not `consent.request`, and replies (a top-level `result`,
//! `code` or `error`), are reported as skipped rather than decoded, so a vector
//! directory that also holds replies and other operations can be passed as is.
//!
//! This covers the request's shape and its semantic rules. It does not cover
//! the checks that need a live daemon: which caller may file which kind, and
//! the identity resolved from the route.
//!
//! Prints one line per file (`ok`, `refused: <reason>` or `skipped: <why>`)
//! and exits 1 if any file was refused or unreadable.

use cingulate_decode::ConsentRequest;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.is_empty() {
        eprintln!("usage: check_consent_request <file-or-dir>...");
        return ExitCode::from(2);
    }
    let mut files = Vec::new();
    for arg in &args {
        if arg.is_dir() {
            match std::fs::read_dir(arg) {
                Ok(entries) => {
                    let mut found: Vec<PathBuf> = entries
                        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                        .collect();
                    found.sort();
                    files.extend(found);
                }
                Err(error) => {
                    println!("{}: unreadable: {error}", arg.display());
                    return ExitCode::FAILURE;
                }
            }
        } else {
            files.push(arg.clone());
        }
    }

    let mut failed = false;
    for file in &files {
        let verdict = check(file);
        if verdict.starts_with("refused") || verdict.starts_with("unreadable") {
            failed = true;
        }
        println!("{}: {verdict}", file.display());
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn check(file: &Path) -> String {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) => return format!("unreadable: {error}"),
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => return format!("unreadable: not JSON: {error}"),
    };
    let params = match value.get("method") {
        Some(Value::String(method)) if method == "consent.request" => match value.get("params") {
            Some(params) => params.clone(),
            None => return "refused: envelope has no params".into(),
        },
        Some(Value::String(method)) => return format!("skipped: method is {method}"),
        Some(_) => return "refused: method is not a string".into(),
        None if ["result", "code", "error"]
            .iter()
            .any(|key| value.get(key).is_some()) =>
        {
            return "skipped: a reply, not a request".into();
        }
        None => value,
    };
    match ConsentRequest::decode(params) {
        Ok(_) => "ok".into(),
        Err(refusal) => format!("refused: {refusal}"),
    }
}
