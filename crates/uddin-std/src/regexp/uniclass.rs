//! The character sets behind `\p{Name}` and the case folding of classes. The names are the ones the reference
//! accepts, spelled the way it canonicalises them; the sets come from the tables of `regex-syntax`, so they follow
//! the Unicode version of that crate, not the reference's.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use regex_syntax::hir::{Class, ClassUnicode, ClassUnicodeRange, HirKind};

use super::syntax::Rune;

const CATEGORIES: &[&str] = &[
    "C", "Cc", "Cf", "Cn", "Co", "Cs", "L", "Ll", "Lm", "Lo", "Lt", "Lu", "M", "Mc", "Me", "Mn",
    "N", "Nd", "Nl", "No", "P", "Pc", "Pd", "Pe", "Pf", "Pi", "Po", "Ps", "S", "Sc", "Sk", "Sm",
    "So", "Z", "Zl", "Zp", "Zs",
];

/// The scripts the reference can find by name. A script whose name has an underscore or a capital past the
/// first letter, such as `Old_Italic` or `SignWriting`, is not here: the reference canonicalises the name it is
/// given to a form those never take, so `\p{Old_Italic}` is an error there.
const SCRIPTS: &[&str] = &[
    "Adlam",
    "Ahom",
    "Arabic",
    "Armenian",
    "Avestan",
    "Balinese",
    "Bamum",
    "Batak",
    "Bengali",
    "Bhaiksuki",
    "Bopomofo",
    "Brahmi",
    "Braille",
    "Buginese",
    "Buhid",
    "Carian",
    "Chakma",
    "Cham",
    "Cherokee",
    "Chorasmian",
    "Common",
    "Coptic",
    "Cuneiform",
    "Cypriot",
    "Cyrillic",
    "Deseret",
    "Devanagari",
    "Dogra",
    "Duployan",
    "Elbasan",
    "Elymaic",
    "Ethiopic",
    "Georgian",
    "Glagolitic",
    "Gothic",
    "Grantha",
    "Greek",
    "Gujarati",
    "Gurmukhi",
    "Han",
    "Hangul",
    "Hanunoo",
    "Hatran",
    "Hebrew",
    "Hiragana",
    "Inherited",
    "Javanese",
    "Kaithi",
    "Kannada",
    "Katakana",
    "Kawi",
    "Kharoshthi",
    "Khmer",
    "Khojki",
    "Khudawadi",
    "Lao",
    "Latin",
    "Lepcha",
    "Limbu",
    "Lisu",
    "Lycian",
    "Lydian",
    "Mahajani",
    "Makasar",
    "Malayalam",
    "Mandaic",
    "Manichaean",
    "Marchen",
    "Medefaidrin",
    "Miao",
    "Modi",
    "Mongolian",
    "Mro",
    "Multani",
    "Myanmar",
    "Nabataean",
    "Nandinagari",
    "Newa",
    "Nko",
    "Nushu",
    "Ogham",
    "Oriya",
    "Osage",
    "Osmanya",
    "Palmyrene",
    "Phoenician",
    "Rejang",
    "Runic",
    "Samaritan",
    "Saurashtra",
    "Sharada",
    "Shavian",
    "Siddham",
    "Sinhala",
    "Sogdian",
    "Soyombo",
    "Sundanese",
    "Syriac",
    "Tagalog",
    "Tagbanwa",
    "Takri",
    "Tamil",
    "Tangsa",
    "Tangut",
    "Telugu",
    "Thaana",
    "Thai",
    "Tibetan",
    "Tifinagh",
    "Tirhuta",
    "Toto",
    "Ugaritic",
    "Vai",
    "Vithkuqi",
    "Wancho",
    "Yezidi",
    "Yi",
];

/// The long names of the categories, canonicalised, and the category each one means.
/// How many named sets a thread keeps.
const MEMO_ENTRIES: usize = 64;

const ALIASES: &[(&str, &str)] = &[
    ("Casedletter", "LC"),
    ("Closepunctuation", "Pe"),
    ("Combiningmark", "M"),
    ("Connectorpunctuation", "Pc"),
    ("Control", "Cc"),
    ("Currencysymbol", "Sc"),
    ("Dashpunctuation", "Pd"),
    ("Decimalnumber", "Nd"),
    ("Enclosingmark", "Me"),
    ("Finalpunctuation", "Pf"),
    ("Format", "Cf"),
    ("Initialpunctuation", "Pi"),
    ("Letter", "L"),
    ("Letternumber", "Nl"),
    ("Lineseparator", "Zl"),
    ("Lowercaseletter", "Ll"),
    ("Mark", "M"),
    ("Mathsymbol", "Sm"),
    ("Modifierletter", "Lm"),
    ("Modifiersymbol", "Sk"),
    ("Nonspacingmark", "Mn"),
    ("Number", "N"),
    ("Openpunctuation", "Ps"),
    ("Other", "C"),
    ("Otherletter", "Lo"),
    ("Othernumber", "No"),
    ("Otherpunctuation", "Po"),
    ("Othersymbol", "So"),
    ("Paragraphseparator", "Zp"),
    ("Privateuse", "Co"),
    ("Punctuation", "P"),
    ("Separator", "Z"),
    ("Spaceseparator", "Zs"),
    ("Spacingmark", "Mc"),
    ("Surrogate", "Cs"),
    ("Symbol", "S"),
    ("Titlecaseletter", "Lt"),
    ("Unassigned", "Cn"),
    ("Uppercaseletter", "Lu"),
    ("Cntrl", "Cc"),
    ("Digit", "Nd"),
    ("Punct", "P"),
];

/// The name with its first letter in upper case, the other letters in lower case, and `_`, `-` and spaces taken
/// out.
fn canonical_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut first = true;
    for c in name.chars() {
        match c {
            '_' | '-' | ' ' => {}
            _ if first => {
                out.push(c.to_ascii_uppercase());
                first = false;
            }
            _ => out.push(c.to_ascii_lowercase()),
        }
    }
    out
}

fn ranges_of(pattern: &str) -> Option<Vec<(Rune, Rune)>> {
    let hir = regex_syntax::ParserBuilder::new()
        .build()
        .parse(pattern)
        .ok()?;
    match hir.kind() {
        HirKind::Class(Class::Unicode(c)) => Some(
            c.iter()
                .map(|r| (Rune::from(r.start()), Rune::from(r.end())))
                .collect(),
        ),
        // A set of one character comes back as a literal.
        HirKind::Literal(l) => {
            let mut chars = std::str::from_utf8(&l.0).ok()?.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(vec![(Rune::from(c), Rune::from(c))]),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The set named `name` as flat low-high pairs, and `-1` when the name stands for everything but the set.
fn lookup_uncached(name: &str) -> Option<(Vec<Rune>, i8)> {
    let name = canonical_name(name);
    let flat = |v: Vec<(Rune, Rune)>| v.into_iter().flat_map(|(a, b)| [a, b]).collect::<Vec<_>>();
    let category = |c: &str| -> Option<Vec<Rune>> {
        if c == "Cs" {
            return Some(vec![0xD800, 0xDFFF]);
        }
        ranges_of(&format!("\\p{{{c}}}")).map(flat)
    };
    match name.as_str() {
        "Any" => return Some((vec![0, super::syntax::MAX_RUNE], 1)),
        "Assigned" => return category("Cn").map(|c| (c, -1)),
        "Ascii" => return Some((vec![0, 0x7F], 1)),
        "Lc" => return category("LC").map(|c| (c, 1)),
        _ => {}
    }
    if CATEGORIES.contains(&name.as_str()) {
        return category(&name).map(|c| (c, 1));
    }
    if SCRIPTS.contains(&name.as_str()) {
        return ranges_of(&format!("\\p{{{name}}}")).map(|v| (flat(v), 1));
    }
    let target = ALIASES.iter().find(|(a, _)| *a == name)?.1;
    category(target).map(|c| (c, 1))
}

/// A named set, shared, and the sign of the name.
type Named = (Rc<[Rune]>, i8);

thread_local! {
    static MEMO: RefCell<HashMap<(String, bool), Named>> = RefCell::new(HashMap::new());
}

/// The set named `name` as sorted, merged low-high pairs, closed under case folding when `fold` is set, and `-1`
/// when the name stands for everything but the set. A thread keeps the sets it asked for last: a pattern may name
/// the same set a thousand times, and the sets of a script are far bigger than the name.
pub(super) fn lookup(name: &str, fold: bool) -> Option<Named> {
    let key = (canonical_name(name), fold);
    if let Some(hit) = MEMO.with(|m| m.borrow().get(&key).cloned()) {
        return Some(hit);
    }
    let (mut flat, sign) = lookup_uncached(&key.0)?;
    if fold {
        let pairs: Vec<(Rune, Rune)> = flat.chunks_exact(2).map(|c| (c[0], c[1])).collect();
        flat.extend(fold_closure(&pairs).into_iter().flat_map(|(a, b)| [a, b]));
    }
    super::syntax::clean_class(&mut flat);
    let made: Named = (flat.into(), sign);
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() >= MEMO_ENTRIES {
            m.clear();
        }
        m.insert(key, made.clone());
    });
    Some(made)
}

/// Whether `c` is a letter or a decimal digit, as the template of a replacement reads a group name.
pub(super) fn is_letter_or_digit(c: Rune) -> bool {
    static SET: OnceLock<Vec<(Rune, Rune)>> = OnceLock::new();
    if c < 0x80 {
        return matches!(c, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A);
    }
    let set = SET.get_or_init(|| {
        let mut all = ranges_of("\\p{L}").unwrap_or_default();
        all.extend(ranges_of("\\p{Nd}").unwrap_or_default());
        all.sort_unstable();
        all
    });
    let i = set.partition_point(|&(_, hi)| hi < c);
    set.get(i).is_some_and(|&(lo, _)| lo <= c)
}

/// `ranges` and every character that folds to one of them under simple case folding.
pub(super) fn fold_closure(ranges: &[(Rune, Rune)]) -> Vec<(Rune, Rune)> {
    let mut class = ClassUnicode::empty();
    for &(lo, hi) in ranges {
        // Surrogates are not characters; the parts of a range on either side of them are.
        for (a, b) in [(lo, hi.min(0xD7FF)), (lo.max(0xE000), hi)] {
            if a <= b
                && let (Some(a), Some(b)) = (char::from_u32(a), char::from_u32(b))
            {
                class.push(ClassUnicodeRange::new(a, b));
            }
        }
    }
    if class.try_case_fold_simple().is_err() {
        return ranges.to_vec();
    }
    let mut out: Vec<(Rune, Rune)> = class
        .iter()
        .map(|r| (Rune::from(r.start()), Rune::from(r.end())))
        .collect();
    out.extend_from_slice(ranges);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalises_names_as_the_reference_does() {
        assert_eq!(canonical_name("lu"), "Lu");
        assert_eq!(canonical_name("GREEK"), "Greek");
        assert_eq!(canonical_name("Old_Italic"), "Olditalic");
        assert_eq!(canonical_name("uppercase letter"), "Uppercaseletter");
        assert_eq!(canonical_name("_x"), "X");
    }

    #[test]
    fn finds_the_names_the_reference_finds() {
        for name in [
            "Lu", "lu", "L", "Greek", "greek", "Letter", "Any", "Ascii", "Assigned", "Lc", "LC",
            "Cs",
        ] {
            assert!(lookup(name, false).is_some(), "{name}");
        }
        for name in [
            "Old_Italic",
            "OldItalic",
            "SignWriting",
            "",
            "L&",
            "Foo",
            "Cased Letter2",
        ] {
            assert!(lookup(name, false).is_none(), "{name}");
        }
        assert_eq!(
            lookup("Ascii", false).map(|(c, s)| (c.to_vec(), s)),
            Some((vec![0, 0x7F], 1))
        );
        let folded = lookup("Lu", true).expect("known").0;
        assert!(folded.chunks_exact(2).any(|c| c[0] <= 0x61 && 0x61 <= c[1]));
    }

    #[test]
    fn folds_to_the_other_cases() {
        let got = fold_closure(&[(0x6B, 0x6B)]);
        let has = |c: Rune| got.iter().any(|&(a, b)| a <= c && c <= b);
        assert!(has(0x4B) && has(0x6B) && has(0x212A));
        assert!(!has(0x6C));
    }
}
