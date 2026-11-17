//! What a run may be told about the request it serves: the values the builtins that read the request see, and the
//! decision a script writes. The machine knows nothing of what these mean.

use std::borrow::Cow;
use std::mem;

/// A decision a script wrote with `waf_return`, `waf_block`, `waf_allow` or `waf_log`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Log,
    Block,
}

impl Verdict {
    /// `allow`, `log` or `block`.
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Log => "log",
            Verdict::Block => "block",
        }
    }
}

/// A decoded JSON document, the form `waf_json` hands to a script. A number keeps the text it was written with.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(Vec<u8>),
    Array(Vec<Json>),
    /// In document order; for a name that appears twice the last value wins.
    Object(Vec<(Vec<u8>, Json)>),
}

impl Json {
    fn is_leaf(&self) -> bool {
        !matches!(self, Json::Array(_) | Json::Object(_))
    }
}

/// A document nested ten thousand levels deep must not overflow the stack when it is freed, so it is taken apart
/// on the heap, a level at a time.
impl Drop for Json {
    fn drop(&mut self) {
        let mut pending: Vec<Json> = Vec::new();
        let take = |node: &mut Json, pending: &mut Vec<Json>| match node {
            Json::Array(items) => pending.extend(mem::take(items)),
            Json::Object(entries) => pending.extend(mem::take(entries).into_iter().map(|(_, v)| v)),
            _ => {}
        };
        take(self, &mut pending);
        while let Some(mut node) = pending.pop() {
            if !node.is_leaf() {
                take(&mut node, &mut pending);
            }
        }
    }
}

/// The request a script is run for, as the builtins that read it see it. Every byte string is borrowed from the
/// embedder for the length of the run, and nothing is copied until a script asks for it.
pub trait RequestContext {
    fn method(&self) -> &[u8];
    fn path(&self) -> &[u8];
    fn query(&self) -> &[u8];
    fn ip(&self) -> &[u8];
    fn country(&self) -> &[u8];
    fn ua(&self) -> &[u8];
    /// The start of the body, at most 4 KiB.
    fn body(&self) -> &[u8];
    fn score(&self) -> f64;
    /// The decision so far: `ALLOW`, `LOG`, `BLOCK` or empty.
    fn action(&self) -> &[u8];
    fn header_count(&self) -> usize;
    /// The name and value of the `i`th header, for `i` below [`RequestContext::header_count`].
    fn header(&self, i: usize) -> (&[u8], &[u8]);
    fn detected_count(&self) -> usize;
    /// The `i`th detected category.
    fn detected(&self, i: usize) -> &[u8];
    /// The parsed JSON body, when there is one. Each call may build a fresh copy.
    fn json(&self) -> Option<Cow<'_, Json>>;
    /// What a rule condition field (`query:id`, `header:x-key`, `json:/a/b`) holds, joined as a condition sees it;
    /// empty when absent.
    fn field(&self, name: &[u8]) -> Cow<'_, [u8]>;
    /// Every value of a field, in order; nothing when absent.
    fn field_values(&self, name: &[u8], each: &mut dyn FnMut(&[u8]));
}
