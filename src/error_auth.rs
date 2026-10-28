//! Signed collector requests. No shared secret is embedded in the browser.
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex<const N: usize>(value: &str) -> Result<[u8; N], String> {
    if value.len() != N * 2 || !value.is_ascii() {
        return Err("Invalid hexadecimal value".into());
    }
    let mut bytes = [0; N];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)
            .map_err(|_| "Invalid hexadecimal value")?;
    }
    Ok(bytes)
}
pub fn body_hash(body: &[u8]) -> String {
    hex(&Sha256::digest(body))
}
pub fn canonical(client: &str, timestamp: &str, nonce: &str, body: &[u8]) -> Vec<u8> {
    format!(
        "wega-errors-v1\nPOST\n/v1/errors\n{client}\n{timestamp}\n{nonce}\n{}",
        body_hash(body)
    )
    .into_bytes()
}
pub fn sign(seed: &[u8; 32], client: &str, timestamp: &str, nonce: &str, body: &[u8]) -> String {
    hex(&SigningKey::from_bytes(seed)
        .sign(&canonical(client, timestamp, nonce, body))
        .to_bytes())
}
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}
pub fn verify(
    key: &[u8; 32],
    signature: &str,
    client: &str,
    timestamp: &str,
    nonce: &str,
    body: &[u8],
) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(key) else {
        return false;
    };
    let Ok(signature) = unhex::<64>(signature) else {
        return false;
    };
    key.verify_strict(
        &canonical(client, timestamp, nonce, body),
        &Signature::from_bytes(&signature),
    )
    .is_ok()
}
