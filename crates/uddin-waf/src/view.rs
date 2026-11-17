//! A request the embedder describes by pointing at its own data.

use std::borrow::Cow;

use uddin_vm::{Json, RequestContext};

/// Answers the value of a rule condition field for the name it is given.
pub type FieldFn<'a> = &'a dyn Fn(&[u8]) -> Vec<u8>;

/// Calls its second argument with every value of the field it is named.
pub type FieldValuesFn<'a> = &'a dyn Fn(&[u8], &mut dyn FnMut(&[u8]));

/// The request of a run, as slices of what the embedder already holds. Nothing is copied when it is made, and a
/// byte is copied only when a script reads it.
#[derive(Clone, Copy)]
pub struct RequestView<'a> {
    pub method: &'a [u8],
    pub path: &'a [u8],
    pub query: &'a [u8],
    pub ip: &'a [u8],
    pub country: &'a [u8],
    pub ua: &'a [u8],
    /// The start of the body, at most 4 KiB.
    pub body: &'a [u8],
    pub score: f64,
    /// The decision so far: `ALLOW`, `LOG`, `BLOCK` or empty.
    pub action: &'a [u8],
    /// Name and value of each header. A name that occurs twice in other letter case is found at its first place.
    pub headers: &'a [(&'a [u8], &'a [u8])],
    pub detected: &'a [&'a [u8]],
    pub json: Option<&'a Json>,
    /// The value of a rule condition field; absent means every field is empty.
    pub field: Option<FieldFn<'a>>,
    /// Every value of a rule condition field, one call of the callback for each.
    pub field_values: Option<FieldValuesFn<'a>>,
}

impl Default for RequestView<'_> {
    fn default() -> Self {
        RequestView {
            method: b"",
            path: b"",
            query: b"",
            ip: b"",
            country: b"",
            ua: b"",
            body: b"",
            score: 0.0,
            action: b"",
            headers: &[],
            detected: &[],
            json: None,
            field: None,
            field_values: None,
        }
    }
}

impl RequestContext for RequestView<'_> {
    fn method(&self) -> &[u8] {
        self.method
    }

    fn path(&self) -> &[u8] {
        self.path
    }

    fn query(&self) -> &[u8] {
        self.query
    }

    fn ip(&self) -> &[u8] {
        self.ip
    }

    fn country(&self) -> &[u8] {
        self.country
    }

    fn ua(&self) -> &[u8] {
        self.ua
    }

    fn body(&self) -> &[u8] {
        self.body
    }

    fn score(&self) -> f64 {
        self.score
    }

    fn action(&self) -> &[u8] {
        self.action
    }

    fn header_count(&self) -> usize {
        self.headers.len()
    }

    fn header(&self, i: usize) -> (&[u8], &[u8]) {
        self.headers[i]
    }

    fn detected_count(&self) -> usize {
        self.detected.len()
    }

    fn detected(&self, i: usize) -> &[u8] {
        self.detected[i]
    }

    fn json(&self) -> Option<Cow<'_, Json>> {
        self.json.map(Cow::Borrowed)
    }

    fn field(&self, name: &[u8]) -> Cow<'_, [u8]> {
        match self.field {
            Some(f) => Cow::Owned(f(name)),
            None => Cow::Borrowed(b""),
        }
    }

    fn field_values(&self, name: &[u8], each: &mut dyn FnMut(&[u8])) {
        if let Some(f) = self.field_values {
            f(name, each);
        }
    }
}
