//! The replacement template of `regex_replace`: `$1`, `${1}`, `$name` and `${name}` stand for the text of a
//! group, `$$` for a dollar sign. A `$` that does not start one of these is copied as it is. A name is the
//! longest run of letters, digits and underscores, so `$1x` is the group called `1x`, not group 1 and an `x`.

use super::syntax::decode;
use super::uniclass::is_letter_or_digit;

type Group = Option<(usize, usize)>;

/// The name or number at the start of `s`, which follows a `$`: the name, the number (-1 when the name is not
/// one), and the rest of the template. `None` when there is no name.
fn extract(s: &[u8]) -> Option<(&[u8], i64, &[u8])> {
    let (brace, body) = match s.first()? {
        b'{' => (true, &s[1..]),
        _ => (false, s),
    };
    let mut i = 0;
    while i < body.len() {
        let (c, w) = decode(&body[i..]);
        if !is_letter_or_digit(c) && c != 0x5F {
            break;
        }
        i += w;
    }
    if i == 0 {
        return None;
    }
    let name = &body[..i];
    let mut end = i;
    if brace {
        if i >= body.len() || body[i] != b'}' {
            return None;
        }
        end += 1;
    }
    let mut num: i64 = 0;
    for &b in name {
        if !b.is_ascii_digit() || num >= 100_000_000 {
            num = -1;
            break;
        }
        num = num * 10 + i64::from(b - b'0');
    }
    if name[0] == b'0' && name.len() > 1 {
        num = -1;
    }
    Some((name, num, &body[end..]))
}

/// Appends `template` to `out` with its references replaced by the text of the groups of one match. `names`
/// and `groups` are indexed by group, the whole match first.
pub(super) fn expand(
    out: &mut Vec<u8>,
    mut template: &[u8],
    src: &[u8],
    names: &[Option<Box<str>>],
    groups: &[Group],
) {
    while !template.is_empty() {
        let Some(dollar) = template.iter().position(|&b| b == b'$') else {
            break;
        };
        out.extend_from_slice(&template[..dollar]);
        template = &template[dollar + 1..];
        if template.first() == Some(&b'$') {
            out.push(b'$');
            template = &template[1..];
            continue;
        }
        let Some((name, num, rest)) = extract(template) else {
            out.push(b'$');
            continue;
        };
        template = rest;
        if num >= 0 {
            if let Some(Some((a, b))) = groups.get(num as usize) {
                out.extend_from_slice(&src[*a..*b]);
            }
        } else {
            for (i, group_name) in names.iter().enumerate() {
                let same = group_name.as_deref().is_some_and(|g| g.as_bytes() == name);
                if let (true, Some(Some((a, b)))) = (same, groups.get(i)) {
                    out.extend_from_slice(&src[*a..*b]);
                    break;
                }
            }
        }
    }
    out.extend_from_slice(template);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(template: &str, src: &str, groups: &[Group], names: &[Option<Box<str>>]) -> String {
        let mut out = Vec::new();
        expand(&mut out, template.as_bytes(), src.as_bytes(), names, groups);
        String::from_utf8(out).expect("utf-8")
    }

    #[test]
    fn expands_numbers_names_and_dollars() {
        let names: Vec<Option<Box<str>>> = vec![None, Some("first".into()), None];
        let groups = [Some((0, 5)), Some((0, 2)), None];
        let src = "abcde";
        assert_eq!(
            run("[$1][${1}][$first][${first}]", src, &groups, &names),
            "[ab][ab][ab][ab]"
        );
        assert_eq!(run("$0 $$ $2 $9 $", src, &groups, &names), "abcde $   $");
        assert_eq!(run("$1x $ {1}", src, &groups, &names), " $ {1}");
        assert_eq!(run("${1", src, &groups, &names), "${1");
        assert_eq!(run("$01", src, &groups, &names), "");
    }
}
