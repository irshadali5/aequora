#![no_main]

use aequora_executor::plan_dependencies;
use aequora_protocol::{OperationEnvelope, SyncRequest};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(request) = postcard::from_bytes::<SyncRequest>(data) {
        let _plan = plan_dependencies(&request.operations);
    }
    if let Ok(operations) = postcard::from_bytes::<Vec<OperationEnvelope>>(data) {
        if operations.len() <= 1_024 {
            let _plan = plan_dependencies(&operations);
        }
    }
});
