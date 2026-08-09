#![no_main]

use aequora_conflict::FieldSet;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((left, right)) = postcard::from_bytes::<(FieldSet, FieldSet)>(data) {
        let merged = left.merge(&right);
        let _idempotent = merged.merge(&merged);
        let _commutative = right.merge(&left);
    }
});
