#![no_main]

use std::cell::RefCell;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use uddin_waf::{End, Profile, RequestView, WafRunner, WafScript, parse_json, profile};

const MAX_OPS: u64 = 20_000;

thread_local! {
    static RUNNER: RefCell<WafRunner> = RefCell::new(WafRunner::new());
}

fuzz_target!(|data: &[u8]| {
    static PROFILE: OnceLock<Profile> = OnceLock::new();
    let profile = PROFILE.get_or_init(profile);
    let Ok(script) = WafScript::compile(data, profile) else {
        return;
    };
    let mut limits = profile.limits();
    limits.max_ops = MAX_OPS;
    // The request is made of the input, so that the `waf_*` builtins find something to read.
    let (head, tail) = data.split_at(data.len().min(64));
    let headers: [(&[u8], &[u8]); 3] = [
        (b"Content-Type", head),
        (b"X-Forwarded-For", tail),
        (head, b"1.2.3.4"),
    ];
    let detected: [&[u8]; 2] = [head, b"sqli"];
    let json = parse_json(data);
    let field = |name: &[u8]| name.iter().chain(head).copied().collect::<Vec<u8>>();
    let values = |name: &[u8], each: &mut dyn FnMut(&[u8])| {
        each(name);
        each(head);
    };
    let view = RequestView {
        method: b"GET",
        path: head,
        query: tail,
        ip: b"203.0.113.9",
        country: b"ID",
        ua: head,
        body: data,
        score: 1.5,
        action: b"LOG",
        headers: &headers,
        detected: &detected,
        json: json.as_ref(),
        field: Some(&field),
        field_values: Some(&values),
    };
    RUNNER.with_borrow_mut(|runner| {
        uddin_fuzz::within(uddin_fuzz::VM_CASE_LIMIT, || {
            let run = runner.run(&script, &view, &limits, None);
            assert!(
                runner.ops() <= MAX_OPS + 1,
                "the operation limit let {} operations through",
                runner.ops()
            );
            assert_ne!(run.end, End::Panic, "the interpreter panicked");
        });
    });
});
