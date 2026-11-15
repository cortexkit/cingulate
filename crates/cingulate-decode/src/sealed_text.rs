use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{ConsentRequest, Refusal};

pub const SEALED_TEXT_SUITE: &str = "X25519-HKDF-SHA256/HKDF-SHA256/ChaCha20-Poly1305";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SealedText {
    pub kind: String,
    pub id: String,
    pub label: String,
    pub max_bytes: u16,
    pub hpke: HpkeKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HpkeKey {
    pub suite: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SealedTextAnswer {
    pub id: String,
    pub enc: String,
    pub ciphertext: String,
}

fn decoded(value: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(value).ok()
}

impl ConsentRequest {
    pub(crate) fn validate_inputs(&self) -> Result<(), Refusal> {
        let Some(inputs) = &self.inputs else {
            return Ok(());
        };
        if self.kind != "consent" || inputs.len() > 1 {
            return Err(Refusal::invalid(
                "inputs require consent kind and at most one sealed_text",
            ));
        }
        for input in inputs {
            if input.kind != "sealed_text"
                || input.id.trim().is_empty()
                || input.label.trim().is_empty()
                || !(1..=4096).contains(&input.max_bytes)
                || input.hpke.suite != SEALED_TEXT_SUITE
                || !decoded(&input.hpke.public_key).is_some_and(|bytes| bytes.len() == 32)
            {
                return Err(Refusal::invalid("invalid sealed_text input"));
            }
        }
        Ok(())
    }

    /// Validate only the envelope; the recipient alone can open the plaintext.
    pub fn answer_inputs(
        &self,
        choice: &str,
        value: Option<&Value>,
    ) -> Result<Vec<SealedTextAnswer>, Refusal> {
        let invalid = || {
            Refusal::new(
                "elicitation_invalid_answer",
                "invalid sealed_text answer inputs",
            )
        };
        let inputs: Vec<SealedTextAnswer> = value
            .map(|value| serde_json::from_value(value.clone()).map_err(|_| invalid()))
            .transpose()?
            .unwrap_or_default();
        let approving = self
            .options
            .iter()
            .any(|option| option.id == choice && option.effect == "grant");
        let required = self
            .inputs
            .as_ref()
            .and_then(|inputs| inputs.first())
            .filter(|_| approving);
        match required {
            Some(spec) if inputs.len() == 1 => {
                let answer = &inputs[0];
                if answer.id != spec.id
                    || !decoded(&answer.enc).is_some_and(|bytes| bytes.len() == 32)
                    || !decoded(&answer.ciphertext).is_some_and(|bytes| {
                        (17..=usize::from(spec.max_bytes) + 16).contains(&bytes.len())
                    })
                {
                    return Err(invalid());
                }
            }
            None if inputs.is_empty() => {}
            _ => return Err(invalid()),
        }
        Ok(inputs)
    }
}

/// Length prefixes keep the choice and sealed byte strings unambiguous. Only
/// the answer row retains this digest after the encrypted payload is released.
pub fn sealed_answer_fingerprint(choice: &str, inputs: &[SealedTextAnswer]) -> String {
    let mut hash = Sha256::new();
    let mut part = |bytes: &[u8]| {
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    };
    part(choice.as_bytes());
    for input in inputs {
        part(input.id.as_bytes());
        part(&decoded(&input.enc).expect("validated enc"));
        part(&decoded(&input.ciphertext).expect("validated ciphertext"));
    }
    format!("{:x}", hash.finalize())
}
