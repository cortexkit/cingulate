#[path = "../tests/support/phone.rs"]
#[allow(dead_code)]
mod phone;
#[path = "../tests/support/sealed_text.rs"]
mod sealed_text;
#[path = "../tests/support/mod.rs"]
mod support;

use std::fs;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hpke::rand_core::SeedableRng;
use hpke::{
    aead::ChaCha20Poly1305, kdf::HkdfSha256, kem::X25519HkdfSha256, Kem, OpModeS, Serializable,
};
use rand_chacha::ChaCha20Rng;
use serde_json::json;
use sha2::{Digest, Sha256};

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        args.is_empty() || args == ["--check"],
        "usage: generate_sealed_text_vectors [--check]"
    );
    let check = !args.is_empty();
    // Public, invented fixture seeds: reproducible HPKE, never production keys.
    let (_, public) = X25519HkdfSha256::derive_keypair(&[41; 32]);
    let mut rng = ChaCha20Rng::from_seed([42; 32]);
    let (enc, ciphertext) =
        hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256, _>(
            &OpModeS::Base,
            &public,
            b"",
            b"invented-sudo-password",
            b"",
            &mut rng,
        )
        .unwrap();
    let public = URL_SAFE_NO_PAD.encode(public.to_bytes());
    let input = json!({"id":"password","enc":URL_SAFE_NO_PAD.encode(enc.to_bytes()),"ciphertext":URL_SAFE_NO_PAD.encode(ciphertext)});
    let root = support::corpus(sealed_text::CORPUS);
    let mut files = sealed_text::generate(&public, input).await;
    files.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).expect("corpus README"),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    if check {
        let actual: std::collections::BTreeSet<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            actual,
            files.keys().cloned().collect(),
            "corpus file coverage"
        );
    }
    for (name, bytes) in &files {
        if check {
            assert_eq!(fs::read(root.join(name)).unwrap(), *bytes, "{name} differs");
        } else {
            fs::write(root.join(name), bytes).unwrap();
        }
    }
    println!(
        "{} {} files from the Cingulate handler",
        if check { "Checked" } else { "Generated" },
        files.len()
    );
}
