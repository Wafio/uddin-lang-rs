//! The searches that go past the first match: all matches, split and replace, each as a search for them would
//! do it, one leftmost-first search after another, moving on by a character after an empty match.

use regex_automata::meta;
use regex_automata::util::captures::Captures;
use uddin_vm::{Exc, Meter};

use super::expand;
use super::limits::{Limit, charge_work, out_of_memory};
use super::search::{Found, Hay};

/// One text searched with one matcher.
pub(super) struct Searcher<'a> {
    pub hay: &'a Hay<'a>,
    pub re: &'a meta::Regex,
    pub cache: &'a mut meta::Cache,
    pub meter: &'a mut Meter,
    /// The instructions of the program, which the first search is charged by, and of the program of the searches
    /// that resume in the middle of the text.
    pub program: u64,
    pub after_program: u64,
    /// Builds the error for a limit the call reaches.
    pub limit: Limit<'a>,
}

impl Searcher<'_> {
    /// How far a search that ended its match at `end` has read: a search that found nothing has read the whole
    /// text, and one that found a match reads on for two more characters before every other way is shut.
    fn reached(&self, end: Option<usize>) -> usize {
        end.map_or(self.hay.len(), |e| self.hay.advance(e, 3))
    }

    /// Charges a search from `from` that read up to `reached`: a search that resumes reads the character before
    /// `from` too, as context, and costs one more unit to set up.
    fn charge(&mut self, from: usize, reached: usize) -> Result<(), Exc> {
        let (program, start) = if from == 0 {
            (self.program, 0)
        } else {
            (self.after_program, from - self.hay.width_before(from))
        };
        let bytes = reached.saturating_sub(start) as u64 + 1;
        charge_work(self.meter, self.limit, program.saturating_mul(bytes))
    }

    /// The leftmost match at or after `from`.
    pub(super) fn next(&mut self, from: usize) -> Result<Option<(usize, usize)>, Exc> {
        let found = self.hay.find(self.re, self.cache, from);
        self.charge(from, self.reached(found.map(|(_, end)| end)))?;
        Ok(found)
    }

    /// [`Searcher::next`] with the spans of the groups.
    fn next_with_groups(&mut self, from: usize, caps: &mut Captures) -> Result<Option<Found>, Exc> {
        let found = self.hay.captures(self.re, self.cache, caps, from);
        self.charge(from, self.reached(found.as_ref().map(|f| f.end)))?;
        Ok(found)
    }

    /// Every match, left to right: an empty match right after the previous match is skipped, and after an empty
    /// match the search moves on by one character. At most `limit` matches.
    fn each(
        &mut self,
        limit: usize,
        mut deliver: impl FnMut(usize, usize) -> Result<(), Exc>,
    ) -> Result<(), Exc> {
        let end = self.hay.len();
        let (mut pos, mut count, mut prev_end) = (0usize, 0usize, usize::MAX);
        while count < limit && pos <= end {
            let Some((start, stop)) = self.next(pos)? else {
                break;
            };
            let mut accept = true;
            if stop == pos {
                if start == prev_end {
                    accept = false;
                }
                let width = if pos < end { self.hay.width_at(pos) } else { 0 };
                pos = if width > 0 { pos + width } else { end + 1 };
            } else {
                pos = stop;
            }
            prev_end = stop;
            if accept {
                deliver(start, stop)?;
                count += 1;
            }
        }
        Ok(())
    }

    fn matches(&mut self) -> Result<Vec<(usize, usize)>, Exc> {
        let mut found: Vec<(usize, usize)> = Vec::new();
        let oom = out_of_memory(self.limit);
        let limit = self.hay.len() + 1;
        self.each(limit, |a, b| {
            found.try_reserve(1).map_err(|_| oom.clone())?;
            found.push((a, b));
            Ok(())
        })?;
        Ok(found)
    }

    /// The spans of every match, as `regex_find_all` returns them.
    pub(super) fn find_all(&mut self) -> Result<Vec<(u32, u32)>, Exc> {
        let found = self.matches()?;
        spans(found.into_iter(), self.limit)
    }

    /// The spans of the parts the text splits into at each match. `pattern_empty` is whether the pattern is the
    /// empty one.
    pub(super) fn split(&mut self, pattern_empty: bool) -> Result<Vec<(u32, u32)>, Exc> {
        let len = self.hay.len();
        // The empty text, split by a pattern that is not empty, is one empty part.
        if !pattern_empty && len == 0 {
            return spans([(0, 0)].into_iter(), self.limit);
        }
        let found = self.matches()?;
        let mut parts: Vec<(usize, usize)> = Vec::new();
        let (mut beg, mut end) = (0usize, 0usize);
        for (a, b) in found {
            end = a;
            if b != 0 {
                parts
                    .try_reserve(1)
                    .map_err(|_| out_of_memory(self.limit))?;
                parts.push((beg, end));
            }
            beg = b;
        }
        if end != len {
            parts
                .try_reserve(1)
                .map_err(|_| out_of_memory(self.limit))?;
            parts.push((beg, len));
        }
        spans(parts.into_iter(), self.limit)
    }

    /// `src` with every match replaced by `template`; `$1`, `${name}` and `$$` in the template stand for what
    /// they stand for when `expands`. An empty match right after a match is not replaced.
    pub(super) fn replace(
        &mut self,
        src: &[u8],
        template: &[u8],
        names: &[Option<Box<str>>],
    ) -> Result<Vec<u8>, Exc> {
        let expands = template.contains(&b'$');
        let mut caps = self.re.create_captures();
        let mut out: Vec<u8> = Vec::new();
        let (mut last_end, mut pos) = (0usize, 0usize);
        while pos <= src.len() {
            let found = if expands {
                self.next_with_groups(pos, &mut caps)?
            } else {
                self.next(pos)?.map(|(start, end)| Found {
                    start,
                    end,
                    groups: Vec::new(),
                })
            };
            let Some(m) = found else {
                break;
            };
            out.try_reserve(m.start - last_end + template.len())
                .map_err(|_| out_of_memory(self.limit))?;
            out.extend_from_slice(&src[last_end..m.start]);
            if m.end > last_end || m.start == 0 {
                if expands {
                    expand::expand(&mut out, template, src, names, &m.groups);
                } else {
                    out.extend_from_slice(template);
                }
            }
            last_end = m.end;
            let width = self.hay.width_at(pos);
            if pos + width > m.end {
                pos += width;
            } else if pos + 1 > m.end {
                pos += 1;
            } else {
                pos = m.end;
            }
        }
        out.try_reserve(src.len() - last_end)
            .map_err(|_| out_of_memory(self.limit))?;
        out.extend_from_slice(&src[last_end..]);
        Ok(out)
    }
}

fn spans(
    parts: impl Iterator<Item = (usize, usize)>,
    limit: Limit<'_>,
) -> Result<Vec<(u32, u32)>, Exc> {
    let mut out = Vec::new();
    for (a, b) in parts {
        out.try_reserve(1).map_err(|_| out_of_memory(limit))?;
        out.push((a as u32, b as u32));
    }
    Ok(out)
}
