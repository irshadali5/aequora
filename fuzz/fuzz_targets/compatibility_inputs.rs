#![no_main]

use aequora_compat::{ClientHello, ExtensionSection, validate_extensions};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeSet;

fuzz_target!(|data: &[u8]| {
    if let Ok(hello) = postcard::from_bytes::<ClientHello>(data) {
        let _ = hello.validate();
    }
    if let Ok(extensions) = postcard::from_bytes::<Vec<ExtensionSection>>(data) {
        let _ = validate_extensions(&extensions, &BTreeSet::new());
    }
});
