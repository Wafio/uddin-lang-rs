#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use uddin::{Profile, check};

fuzz_target!(|data: &[u8]| {
    static PROFILE: OnceLock<Profile> = OnceLock::new();
    let profile = PROFILE.get_or_init(Profile::embedded);
    uddin_fuzz::within_limit_for(data.len(), || {
        std::hint::black_box(check(data, profile).is_ok());
    });
});
