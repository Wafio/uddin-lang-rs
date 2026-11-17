#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    uddin_fuzz::within_limit_for(data.len(), || {
        std::hint::black_box(uddin_syntax::parse_source(data).is_ok());
    });
});
