//! Filing checks for the signed scopes that approving apps display.

use serde_json::Value;

use crate::secret_use::StatementRule;

// These three constants must match the vault's location grammar.
const BASIC_USER_MAX_BYTES: usize = 128;
const BASIC_USER_PUNCTUATION: &[u8] = b"._@+-";
const POINTER_MIN_BYTE: u8 = 0x21;

// The vault owns host grammar; the apps only check the scheme and remainder.
const HTTPS_ORIGIN_PREFIX: &str = "https://";

pub(super) fn location(value: &str) -> Result<(), &'static str> {
    if value == "authorization_bearer" {
        return Ok(());
    }
    match value.split_once(':') {
        Some(("header", name)) => {
            if !(1..=64).contains(&name.len())
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            {
                return Err("use.location header name must be a 1–64 byte HTTP token");
            }
            if ["host", "cookie", "content-length", "authorization"]
                .iter()
                .any(|reserved| name.eq_ignore_ascii_case(reserved))
            {
                return Err("use.location must not name a reserved header (Host, Cookie, Content-Length or Authorization)");
            }
        }
        Some(("query", name)) => {
            if !ascii_name(name, 64, b"._-") {
                return Err("use.location query name must be 1–64 bytes of A-Za-z0-9._-");
            }
        }
        Some(("json_pointer", pointer)) => {
            if !pointer.starts_with('/') || pointer.len() > 256 {
                return Err("use.location JSON pointer must start with / and be at most 256 bytes");
            }
            if !pointer.bytes().all(|b| (POINTER_MIN_BYTE..=0x7e).contains(&b)) {
                return Err("use.location JSON pointer contains a disallowed byte");
            }
            let mut bytes = pointer.bytes();
            while let Some(b) = bytes.next() {
                if b == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
                    return Err("use.location JSON pointer must escape ~ as ~0 or ~1");
                }
            }
        }
        Some(("authorization_basic", user)) => {
            if !ascii_name(user, BASIC_USER_MAX_BYTES, BASIC_USER_PUNCTUATION) {
                return Err("use.location Basic user has an invalid length or character");
            }
        }
        _ => return Err("use.location must be authorization_bearer, header:, query:, json_pointer: or authorization_basic:"),
    }
    Ok(())
}

fn ascii_name(value: &str, max: usize, punctuation: &[u8]) -> bool {
    (1..=max).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || punctuation.contains(&b))
}

pub(super) fn flat_api_key(
    host: &str,
    path_prefix: &str,
    key_location: &str,
) -> Result<(), &'static str> {
    if host.is_empty() || host.contains('*') {
        return Err("use.host must be nonempty and contain no wildcard");
    }
    if !path_prefix_ok(path_prefix) {
        return Err("use.path_prefix must be empty or start with /");
    }
    location(key_location)
}

fn path_prefix_ok(value: &str) -> bool {
    value.is_empty() || value.starts_with('/')
}

fn wildcard(host: &str) -> bool {
    host.strip_prefix("*.")
        .is_some_and(|suffix| !suffix.is_empty() && !suffix.contains('*'))
}

pub(super) fn extended_api_key(
    rules: &[StatementRule],
    key_location: &str,
) -> Result<(), &'static str> {
    if !(1..=8).contains(&rules.len()) {
        return Err("use.rules must contain 1–8 rules");
    }
    for rule in rules {
        if rule.host.is_empty() || (rule.host.contains('*') && !wildcard(&rule.host)) {
            return Err("use.rules host must be nonempty and may use * only as one leading *. with a nonempty suffix");
        }
        if !path_prefix_ok(&rule.path_prefix) {
            return Err("use.rules path_prefix must be empty or start with /");
        }
    }
    if rules.len() == 1 && !wildcard(&rules[0].host) {
        return Err("use.rules requires a wildcard host when there is only one rule");
    }
    if rules
        .windows(2)
        .any(|pair| (&pair[0].host, &pair[0].path_prefix) >= (&pair[1].host, &pair[1].path_prefix))
    {
        return Err("use.rules must be strictly ascending by host, then path_prefix");
    }
    location(key_location)
}

pub(super) fn origins(origins: &[String]) -> Result<(), &'static str> {
    if !(2..=4).contains(&origins.len()) {
        return Err("use.origins must contain 2–4 origins");
    }
    if origins.iter().any(|origin| {
        !origin
            .strip_prefix(HTTPS_ORIGIN_PREFIX)
            .is_some_and(|host| !host.is_empty() && !host.contains('*'))
    }) {
        return Err("use.origins entries must start with https://, have a nonempty host and contain no wildcard");
    }
    if origins.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("use.origins must be strictly ascending as written");
    }
    for (i, origin) in origins.iter().enumerate() {
        let host = origin.strip_suffix(":443").unwrap_or(origin);
        if origins[..i]
            .iter()
            .any(|other| other.strip_suffix(":443").unwrap_or(other) == host)
        {
            return Err("use.origins must be distinct after removing a trailing :443");
        }
    }
    Ok(())
}

pub(super) fn methods(methods: &[String]) -> Result<(), &'static str> {
    if methods.is_empty() {
        return Err("use.methods must contain at least one method");
    }
    if methods.iter().any(|method| {
        !(1..=16).contains(&method.len()) || !method.bytes().all(|b| b.is_ascii_uppercase())
    }) {
        return Err("each use.methods entry must be 1–16 uppercase ASCII letters");
    }
    if methods.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("use.methods must be strictly ascending");
    }
    Ok(())
}

pub(super) fn shape_error(statement: &Value) -> Option<&'static str> {
    // The untagged use enum's serde error cannot pinpoint a malformed rule.
    if let Some(rules) = statement.pointer("/use/rules") {
        match rules.as_array() {
            None => return Some("use.rules must be an array"),
            Some(rules)
                if rules.iter().any(|rule| {
                    !rule.as_object().is_some_and(|rule| {
                        rule.len() == 2
                            && rule.get("host").is_some_and(Value::is_string)
                            && rule.get("path_prefix").is_some_and(Value::is_string)
                    })
                }) =>
            {
                return Some(
                    "each use.rules entry must contain exactly host and path_prefix strings",
                )
            }
            _ => {}
        }
    }
    None
}
