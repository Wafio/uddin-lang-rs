//! Searching text the way the reference reads it: as characters, where each byte that is not valid UTF-8 is the
//! replacement character U+FFFD. The matcher sees valid text only, so a text with invalid bytes is copied once
//! with each such byte widened to the three bytes of U+FFFD, and offsets are carried back to the original text.

use std::borrow::Cow;

use regex_automata::util::captures::Captures;
use regex_automata::{Input, meta};

use super::syntax::decode;

const REPLACEMENT: [u8; 3] = [0xEF, 0xBF, 0xBD];

pub(super) struct Hay<'a> {
    orig: &'a [u8],
    text: Cow<'a, [u8]>,
    /// Offsets in the original text of the bytes that are not valid UTF-8.
    bad: Vec<u32>,
}

/// The span of a match in the original text, and the spans of its groups when they were asked for.
pub(super) struct Found {
    pub start: usize,
    pub end: usize,
    pub groups: Vec<Option<(usize, usize)>>,
}

impl<'a> Hay<'a> {
    /// `None` when the copy of a text with invalid bytes cannot be allocated.
    pub(super) fn new(orig: &'a [u8]) -> Option<Hay<'a>> {
        if std::str::from_utf8(orig).is_ok() {
            return Some(Hay {
                orig,
                text: Cow::Borrowed(orig),
                bad: Vec::new(),
            });
        }
        let mut bad = Vec::new();
        let mut text = Vec::new();
        text.try_reserve(orig.len() + 64).ok()?;
        let mut i = 0;
        while i < orig.len() {
            match std::str::from_utf8(&orig[i..]) {
                Ok(_) => {
                    text.try_reserve(orig.len() - i).ok()?;
                    text.extend_from_slice(&orig[i..]);
                    break;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    let n = e.error_len().unwrap_or(orig.len() - i - valid);
                    text.try_reserve(valid + 3 * n).ok()?;
                    bad.try_reserve(n).ok()?;
                    text.extend_from_slice(&orig[i..i + valid]);
                    for k in 0..n {
                        bad.push((i + valid + k) as u32);
                        text.extend_from_slice(&REPLACEMENT);
                    }
                    i += valid + n;
                }
            }
        }
        Some(Hay {
            orig,
            text: Cow::Owned(text),
            bad,
        })
    }

    pub(super) fn len(&self) -> usize {
        self.orig.len()
    }

    fn to_text(&self, at: usize) -> usize {
        at + 2 * self.bad.partition_point(|&b| (b as usize) < at)
    }

    fn to_orig(&self, at: usize) -> usize {
        // The i-th bad byte starts at `bad[i] + 2 * i` in the copy.
        let (mut lo, mut hi) = (0, self.bad.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if (self.bad[mid] as usize) + 2 * mid < at {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        at - 2 * lo
    }

    /// The width in bytes of the character at `at`, as the reference decodes it: 0 at the end of the text and 1
    /// for a byte that is not valid UTF-8.
    pub(super) fn width_at(&self, at: usize) -> usize {
        if at >= self.orig.len() {
            return 0;
        }
        if self.bad.binary_search(&(at as u32)).is_ok() {
            return 1;
        }
        decode(&self.orig[at..]).1
    }

    /// The width of the character that ends at `at`, which is 1 when the bytes before `at` are not a whole
    /// character.
    pub(super) fn width_before(&self, at: usize) -> usize {
        if at == 0 {
            return 0;
        }
        let end = at.min(self.orig.len());
        let last = end - 1;
        if self.orig[last] < 0x80 {
            return 1;
        }
        let lim = end.saturating_sub(4);
        let mut start = last;
        while start > lim {
            start -= 1;
            if self.orig[start] & 0xC0 != 0x80 {
                break;
            }
        }
        let (_, size) = decode(&self.orig[start..end]);
        if start + size == end { size } else { 1 }
    }

    fn input(&self, from: usize) -> Input<'_> {
        Input::new(self.text.as_ref()).range(self.to_text(from)..)
    }

    /// Where the first match ends, if the text has one: the earliest end of any match, which is where a search
    /// that only asks whether there is one can stop.
    pub(super) fn earliest_end(&self, re: &meta::Regex, cache: &mut meta::Cache) -> Option<usize> {
        let input = self.input(0).earliest(true);
        re.search_half_with(cache, &input)
            .map(|m| self.to_orig(m.offset()))
    }

    /// The offset `chars` characters after `at`, or the end of the text.
    pub(super) fn advance(&self, mut at: usize, chars: usize) -> usize {
        for _ in 0..chars {
            match self.width_at(at) {
                0 => break,
                w => at += w,
            }
        }
        at
    }

    /// The leftmost match that starts at or after `from`. The text before `from` is context for the assertions
    /// of the pattern, so `\b` and `^` see the real previous character.
    pub(super) fn find(
        &self,
        re: &meta::Regex,
        cache: &mut meta::Cache,
        from: usize,
    ) -> Option<(usize, usize)> {
        let m = re.search_with(cache, &self.input(from))?;
        Some((self.to_orig(m.start()), self.to_orig(m.end())))
    }

    /// [`Hay::find`] with the spans of the groups; `groups` has one entry per group, the whole match first.
    pub(super) fn captures(
        &self,
        re: &meta::Regex,
        cache: &mut meta::Cache,
        caps: &mut Captures,
        from: usize,
    ) -> Option<Found> {
        re.search_captures_with(cache, &self.input(from), caps);
        let whole = caps.get_match()?;
        let groups = (0..caps.group_len())
            .map(|i| {
                caps.get_group(i)
                    .map(|s| (self.to_orig(s.start), self.to_orig(s.end)))
            })
            .collect();
        Some(Found {
            start: self.to_orig(whole.start()),
            end: self.to_orig(whole.end()),
            groups,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_survive_the_widening_of_invalid_bytes() {
        let h = Hay::new(b"a\xFFb\xC3").expect("allocates");
        assert_eq!(h.len(), 4);
        assert_eq!(h.text.as_ref(), "a\u{FFFD}b\u{FFFD}".as_bytes());
        for (orig, text) in [(0, 0), (1, 1), (2, 4), (3, 5), (4, 8)] {
            assert_eq!(h.to_text(orig), text);
            assert_eq!(h.to_orig(text), orig);
        }
        assert_eq!(
            (h.width_at(0), h.width_at(1), h.width_at(3), h.width_at(4)),
            (1, 1, 1, 0)
        );
    }

    #[test]
    fn finds_the_character_before_a_position() {
        let h = Hay::new("aé日\u{1F600}\u{80}".as_bytes()).expect("allocates");
        let ends = [1, 3, 6, 10, 12];
        let widths: Vec<usize> = ends.iter().map(|&e| h.width_before(e)).collect();
        assert_eq!(widths, [1, 2, 3, 4, 2]);
        let h = Hay::new(b"a\xE6\x97").expect("allocates");
        assert_eq!(
            (h.width_before(3), h.width_before(2), h.width_before(0)),
            (1, 1, 0)
        );
    }

    #[test]
    fn splits_a_bad_run_into_single_bytes() {
        let h = Hay::new(b"\xE2\x82").expect("allocates");
        assert_eq!(h.text.as_ref(), "\u{FFFD}\u{FFFD}".as_bytes());
    }
}
