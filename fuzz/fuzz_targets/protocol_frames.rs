#![no_main]

use aequora_codec::{DecodeLimits, MessageKind};
use aequora_protocol::SyncRequest;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let maximum = data.len().saturating_mul(2).max(64);
    let _frame = aequora_codec::decode_frame(data, maximum);
    let _request = aequora_codec::decode_with_limits::<SyncRequest>(
        data,
        MessageKind::SyncRequest,
        DecodeLimits {
            max_wire_bytes: maximum,
            max_decompressed_bytes: maximum.saturating_mul(4),
        },
    );
});
