#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The lexer takes text; the parser is what checks the bytes as UTF-8.
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    uddin_fuzz::within_limit_for(data.len(), || {
        std::hint::black_box(uddin_syntax::tokenize(text));
    });
});
