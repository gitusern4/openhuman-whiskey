use super::{decrypt_handoff_blob, key_bytes_from_string};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;

// We need these for internal encryption in tests to ensure fixtures match the implementation
use aes_gcm::aead::generic_array::typenum::U16;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::aes::Aes256;
use aes_gcm::AesGcm;
type Aes256Gcm16 = AesGcm<Aes256, U16>;

#[test]
fn decodes_base64url_no_pad() {
    // A 32-byte key that, when base64url-encoded, contains both `-` and `_`.
    let raw = [
        0xff_u8, 0xfb, 0xef, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa,
        0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a,
        0x0b, 0x0c, 0x0d,
    ];
    let url_key = URL_SAFE_NO_PAD.encode(raw);
    assert!(url_key.contains('-') || url_key.contains('_'));
    let decoded = key_bytes_from_string(&url_key).unwrap();
    assert_eq!(decoded, raw);
}

#[test]
fn decodes_standard_base64() {
    let raw = [0x41_u8; 32];
    let std_key = STANDARD.encode(raw);
    let decoded = key_bytes_from_string(&std_key).unwrap();
    assert_eq!(decoded, raw);
}

#[test]
fn decodes_raw_32_byte_key() {
    let raw = "abcdefghijklmnopqrstuvwxyz012345";
    assert_eq!(raw.len(), 32);
    let decoded = key_bytes_from_string(raw).unwrap();
    assert_eq!(decoded, raw.as_bytes());
}

#[test]
fn trims_whitespace() {
    let raw = [0x42_u8; 32];
    let url_key = format!("  {}\n", URL_SAFE_NO_PAD.encode(raw));
    let decoded = key_bytes_from_string(&url_key).unwrap();
    assert_eq!(decoded, raw);
}

#[test]
fn rejects_wrong_length() {
    let err = key_bytes_from_string("tooshort").unwrap_err();
    assert!(err.to_string().contains("must decode to 32 raw bytes"));
}

#[test]
fn decrypts_valid_blob() {
    let key_bytes = [0x42u8; 32];
    let iv_bytes = [0x24u8; 16];
    let plain = "hello world";

    let cipher = Aes256Gcm16::new_from_slice(&key_bytes).unwrap();
    let nonce = aes_gcm::aead::generic_array::GenericArray::from_slice(&iv_bytes);
    let encrypted = cipher.encrypt(nonce, plain.as_bytes()).unwrap();
    let (ciphertext, tag) = encrypted.split_at(encrypted.len() - 16);

    let mut combined = Vec::new();
    combined.extend_from_slice(&iv_bytes);
    combined.extend_from_slice(tag);
    combined.extend_from_slice(ciphertext);

    let b64 = STANDARD.encode(combined);
    let key_str = STANDARD.encode(key_bytes);

    let decrypted = decrypt_handoff_blob(&b64, &key_str).unwrap();
    assert_eq!(decrypted, plain);
}

#[test]
fn decrypt_fails_if_too_short() {
    let key = "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=";
    let b64 = STANDARD.encode([0u8; 31]);
    let err = decrypt_handoff_blob(&b64, key).unwrap_err();
    assert!(err.to_string().contains("encrypted payload too short"));
}

#[test]
fn decrypt_fails_on_invalid_base64() {
    let key = "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=";
    let err = decrypt_handoff_blob("not-base64!!!", key).unwrap_err();
    assert!(err.to_string().contains("base64-decode encrypted payload"));
}

#[test]
fn decrypt_fails_on_wrong_key() {
    let key_bytes = [0x42u8; 32];
    let iv_bytes = [0x24u8; 16];
    let plain = "hello world";

    let cipher = Aes256Gcm16::new_from_slice(&key_bytes).unwrap();
    let nonce = aes_gcm::aead::generic_array::GenericArray::from_slice(&iv_bytes);
    let encrypted = cipher.encrypt(nonce, plain.as_bytes()).unwrap();
    let (ciphertext, tag) = encrypted.split_at(encrypted.len() - 16);

    let mut combined = Vec::new();
    combined.extend_from_slice(&iv_bytes);
    combined.extend_from_slice(tag);
    combined.extend_from_slice(ciphertext);

    let b64 = STANDARD.encode(combined);
    let wrong_key = STANDARD.encode([0u8; 32]);

    let err = decrypt_handoff_blob(&b64, &wrong_key).unwrap_err();
    assert!(err.to_string().contains("AES-GCM decrypt failed"));
}

#[test]
fn decrypt_fails_on_tampered_ciphertext() {
    let key_bytes = [0x42u8; 32];
    let iv_bytes = [0x24u8; 16];
    let plain = "hello world";

    let cipher = Aes256Gcm16::new_from_slice(&key_bytes).unwrap();
    let nonce = aes_gcm::aead::generic_array::GenericArray::from_slice(&iv_bytes);
    let encrypted = cipher.encrypt(nonce, plain.as_bytes()).unwrap();
    let (ciphertext, tag) = encrypted.split_at(encrypted.len() - 16);

    let mut combined = Vec::new();
    combined.extend_from_slice(&iv_bytes);
    combined.extend_from_slice(tag);
    combined.extend_from_slice(ciphertext);

    // Tamper with the last byte of ciphertext
    let last = combined.len() - 1;
    combined[last] ^= 0xFF;

    let b64 = STANDARD.encode(combined);
    let key_str = STANDARD.encode(key_bytes);

    let err = decrypt_handoff_blob(&b64, &key_str).unwrap_err();
    assert!(err.to_string().contains("AES-GCM decrypt failed"));
}

#[test]
fn decrypt_fails_on_invalid_utf8() {
    let key_bytes = [0x42u8; 32];
    let iv_bytes = [0x24u8; 16];
    let plain = [0xFFu8, 0xFE, 0xFD];

    let cipher = Aes256Gcm16::new_from_slice(&key_bytes).unwrap();
    let nonce = aes_gcm::aead::generic_array::GenericArray::from_slice(&iv_bytes);
    let encrypted = cipher.encrypt(nonce, plain.as_ref()).unwrap();
    let (ciphertext, tag) = encrypted.split_at(encrypted.len() - 16);

    let mut combined = Vec::new();
    combined.extend_from_slice(&iv_bytes);
    combined.extend_from_slice(tag);
    combined.extend_from_slice(ciphertext);

    let b64 = STANDARD.encode(combined);
    let key_str = STANDARD.encode(key_bytes);

    let err = decrypt_handoff_blob(&b64, &key_str).unwrap_err();
    assert!(err.to_string().contains("handoff plaintext is not UTF-8"));
}
