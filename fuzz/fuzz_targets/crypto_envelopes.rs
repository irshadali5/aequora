#![no_main]

use aequora_crypto::{EncryptedPayload, ProtectedPayload};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(payload) = postcard::from_bytes::<ProtectedPayload>(bytes) {
        let _ = payload.validate_structure();
    }
    if let Ok(encrypted) = postcard::from_bytes::<EncryptedPayload>(bytes) {
        let _ = encrypted.ciphertext.len();
    }
});
