//! The lexer. It reads one character at a time like the Go tokenizer it replaces, so token boundaries,
//! error texts and positions agree with it.

use crate::token::{Position, TokenKind};

pub(crate) struct Lexer<'a> {
    source: &'a str,
    /// Byte offset of the character after `current_char`.
    offset: usize,
    /// The current character; `None` at the end of input and after an error.
    current_char: Option<char>,
    /// Set once and kept: every later token is then `Illegal` with this text.
    error: Option<String>,
    position: Position,
    next_position: Position,
}

fn is_identifier_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn keyword(name: &str) -> Option<TokenKind> {
    Some(match name {
        "and" => TokenKind::And,
        "break" => TokenKind::Break,
        "catch" => TokenKind::Catch,
        "continue" => TokenKind::Continue,
        "elif" => TokenKind::Elif,
        "else" => TokenKind::Else,
        "end" => TokenKind::End,
        "false" => TokenKind::False,
        "for" => TokenKind::For,
        "fun" => TokenKind::Fun,
        "if" => TokenKind::If,
        "import" => TokenKind::Import,
        "in" => TokenKind::In,
        "memo" => TokenKind::Memo,
        "null" => TokenKind::Null,
        "not" => TokenKind::Not,
        "or" => TokenKind::Or,
        "return" => TokenKind::Return,
        "then" => TokenKind::Then,
        "true" => TokenKind::True,
        "try" => TokenKind::Try,
        "while" => TokenKind::While,
        "xor" => TokenKind::Xor,
        _ => return None,
    })
}

type Token = (Position, TokenKind, String);

fn illegal(position: Position, message: impl Into<String>) -> Token {
    (position, TokenKind::Illegal, message.into())
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(src: &'a str) -> Self {
        let mut lexer = Lexer {
            source: src,
            offset: 0,
            current_char: None,
            error: None,
            position: Position::START,
            next_position: Position::START,
        };
        lexer.advance();
        lexer
    }

    fn advance(&mut self) {
        self.position = self.next_position;
        let Some(&byte) = self.source.as_bytes().get(self.offset) else {
            self.current_char = None;
            return;
        };
        let c = if byte < 0x80 {
            char::from(byte)
        } else {
            match self.source[self.offset..].chars().next() {
                Some(c) => c,
                None => {
                    self.current_char = None;
                    return;
                }
            }
        };
        // The Go decoder reports a literal U+FFFD like a bad byte, so a source holding one is refused too.
        if c == '\u{FFFD}' {
            self.current_char = None;
            self.error = Some(format!("invalid UTF-8 byte 0x{byte:02x}"));
            return;
        }
        if c == '\n' {
            self.next_position.line += 1;
            self.next_position.column = 1;
        } else {
            self.next_position.column += 1;
        }
        self.current_char = Some(c);
        self.offset += c.len_utf8();
    }

    fn peek_byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.offset).copied()
    }

    fn skip_trivia(&mut self) {
        loop {
            while matches!(self.current_char, Some(' ' | '\t' | '\r' | '\n')) {
                self.advance();
            }
            if self.current_char == Some('/') {
                match self.peek_byte() {
                    Some(b'/') => {
                        self.advance();
                        self.advance();
                        while self.current_char.is_some_and(|c| c != '\n') {
                            self.advance();
                        }
                        self.advance();
                        continue;
                    }
                    Some(b'*') => {
                        self.advance();
                        self.advance();
                        loop {
                            if self.current_char.is_none() {
                                self.error = Some("unterminated multiline comment".to_owned());
                                return;
                            }
                            if self.current_char == Some('*') && self.peek_byte() == Some(b'/') {
                                self.advance();
                                self.advance();
                                break;
                            }
                            self.advance();
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            break;
        }
    }

    /// `first` already consumed, then `=` makes `with_eq`.
    fn optional_equal_suffix(&mut self, with_eq: TokenKind, plain: TokenKind) -> TokenKind {
        if self.current_char == Some('=') {
            self.advance();
            with_eq
        } else {
            plain
        }
    }

    /// The position, kind and value of the next token. Only names, numbers and strings carry a value, and
    /// an `Illegal` token carries its message.
    pub(crate) fn next_token(&mut self) -> Token {
        self.skip_trivia();
        let Some(ch) = self.current_char else {
            return match &self.error {
                Some(message) => (self.position, TokenKind::Illegal, message.clone()),
                None => (self.position, TokenKind::Eof, String::new()),
            };
        };
        let position = self.position;
        let start = self.offset - ch.len_utf8();
        self.advance();

        if is_identifier_start(ch) {
            while self
                .current_char
                .is_some_and(|c| is_identifier_start(c) || c.is_ascii_digit())
            {
                self.advance();
            }
            let end = self
                .current_char
                .map_or(self.offset, |c| self.offset - c.len_utf8());
            let name = &self.source[start..end];
            return match keyword(name) {
                Some(token) => (position, token, String::new()),
                None => (position, TokenKind::Name, name.to_owned()),
            };
        }

        let token = match ch {
            ':' => TokenKind::Colon,
            ',' => TokenKind::Comma,
            '/' => self.optional_equal_suffix(TokenKind::SlashEqual, TokenKind::Slash),
            '{' => TokenKind::OpenBrace,
            '[' => TokenKind::OpenBracket,
            '(' => TokenKind::OpenParen,
            '-' => self.optional_equal_suffix(TokenKind::MinusEqual, TokenKind::Minus),
            '%' => self.optional_equal_suffix(TokenKind::PercentEqual, TokenKind::Percent),
            '+' => self.optional_equal_suffix(TokenKind::PlusEqual, TokenKind::Plus),
            '}' => TokenKind::CloseBrace,
            ']' => TokenKind::CloseBracket,
            ')' => TokenKind::CloseParen,
            '*' => {
                if self.current_char == Some('*') {
                    self.advance();
                    TokenKind::StarStar
                } else {
                    self.optional_equal_suffix(TokenKind::StarEqual, TokenKind::Star)
                }
            }
            '?' => TokenKind::Question,
            '=' => self.optional_equal_suffix(TokenKind::EqualEqual, TokenKind::Assign),
            '!' => {
                if self.current_char == Some('=') {
                    self.advance();
                    TokenKind::BangEqual
                } else {
                    let next = self.current_char.unwrap_or('\u{FFFD}');
                    return illegal(position, format!("expected != instead of !{next}"));
                }
            }
            '<' => self.optional_equal_suffix(TokenKind::Le, TokenKind::Lt),
            '>' => self.optional_equal_suffix(TokenKind::Ge, TokenKind::Gt),
            '.' => {
                if self.current_char == Some('.') {
                    self.advance();
                    if self.current_char != Some('.') {
                        return illegal(position, "unexpected ..");
                    }
                    self.advance();
                    TokenKind::Ellipsis
                } else {
                    TokenKind::Dot
                }
            }
            '0'..='9' => return self.number(position, ch),
            '"' | '\'' => return self.quoted(position, ch),
            '`' => return self.raw_string(position),
            _ => return illegal(position, format!("unexpected {ch}")),
        };
        (position, token, String::new())
    }

    /// Digits with `_` separators, an optional fraction and an optional exponent. The value has the
    /// separators removed. Converting it to a number is the parser's job.
    fn number(&mut self, position: Position, first: char) -> Token {
        let mut text = String::from(first);
        let mut is_float = false;
        let mut last_was_underscore = false;

        while let Some(c) = self.current_char {
            if !(c.is_ascii_digit() || c == '.' || c == '_') {
                break;
            }
            if c == '.' {
                if is_float {
                    return illegal(position, "unexpected second '.' in number");
                }
                if last_was_underscore {
                    return illegal(position, "digit separator '_' cannot be followed by '.'");
                }
                is_float = true;
                text.push(c);
                last_was_underscore = false;
            } else if c == '_' {
                if last_was_underscore {
                    return illegal(position, "consecutive digit separators '_' are not allowed");
                }
                last_was_underscore = true;
            } else {
                text.push(c);
                last_was_underscore = false;
            }
            self.advance();
        }
        if last_was_underscore {
            return illegal(position, "number cannot end with digit separator '_'");
        }

        if let Some(e @ ('e' | 'E')) = self.current_char {
            is_float = true;
            text.push(e);
            self.advance();
            if let Some(sign @ ('+' | '-')) = self.current_char {
                text.push(sign);
                self.advance();
            }
            if !self.current_char.is_some_and(|c| c.is_ascii_digit()) {
                return illegal(position, "expected digit after exponent");
            }
            last_was_underscore = false;
            while let Some(c) = self.current_char {
                if !(c.is_ascii_digit() || c == '_') {
                    break;
                }
                if c == '_' {
                    if last_was_underscore {
                        return illegal(
                            position,
                            "consecutive digit separators '_' are not allowed in exponent",
                        );
                    }
                    last_was_underscore = true;
                } else {
                    text.push(c);
                    last_was_underscore = false;
                }
                self.advance();
            }
            if last_was_underscore {
                return illegal(position, "exponent cannot end with digit separator '_'");
            }
        }

        (
            position,
            if is_float {
                TokenKind::Float
            } else {
                TokenKind::Int
            },
            text,
        )
    }

    /// A string in `quote`. Escapes: `\n \t \r \\` and the quote itself. A raw newline is an error.
    fn quoted(&mut self, position: Position, quote: char) -> Token {
        let mut text = String::new();
        while self.current_char != Some(quote) {
            let Some(mut c) = self.current_char else {
                return illegal(position, "didn't find end quote in string");
            };
            if c == '\r' || c == '\n' {
                return illegal(position, "can't have newline in string");
            }
            if c == '\\' {
                self.advance();
                c = match self.current_char {
                    Some(e) if e == quote || e == '\\' => e,
                    Some('t') => '\t',
                    Some('r') => '\r',
                    Some('n') => '\n',
                    other => {
                        let shown = other.unwrap_or('\u{FFFD}');
                        return illegal(position, format!("invalid string escape \\{shown}"));
                    }
                };
            }
            text.push(c);
            self.advance();
        }
        self.advance();
        (position, TokenKind::Str, text)
    }

    /// A backtick string: no escapes, newlines allowed.
    fn raw_string(&mut self, position: Position) -> Token {
        let mut text = String::new();
        while self.current_char != Some('`') {
            let Some(c) = self.current_char else {
                return illegal(position, "didn't find end backtick in multiline string");
            };
            text.push(c);
            self.advance();
        }
        self.advance();
        (position, TokenKind::Str, text)
    }
}

/// Every token of a source, up to and including the first `Eof` or `Illegal`. It exists so a fuzz target can
/// drive the lexer on its own; the parser reads tokens one at a time.
#[doc(hidden)]
pub fn tokenize(src: &str) -> Vec<(Position, TokenKind, String)> {
    let mut lexer = Lexer::new(src);
    let mut out = Vec::new();
    loop {
        let token = lexer.next_token();
        let done = matches!(token.1, TokenKind::Eof | TokenKind::Illegal);
        out.push(token);
        if done {
            return out;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_all(src: &str) -> Vec<(TokenKind, String)> {
        let mut lexer = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let (position, token, value) = lexer.next_token();
            assert!(
                position.line >= 1 && position.column >= 1,
                "bad position {position:?} in {src:?}"
            );
            let stop = matches!(token, TokenKind::Eof | TokenKind::Illegal);
            out.push((token, value));
            if stop {
                return out;
            }
        }
    }

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex_all(src).into_iter().map(|(t, _)| t).collect()
    }

    fn first(src: &str) -> (TokenKind, String) {
        lex_all(src).remove(0)
    }

    #[test]
    fn token_streams() {
        use TokenKind::*;
        let cases: &[(&str, &[TokenKind], &[&str])] = &[
            ("x = 5", &[Name, Assign, Int, Eof], &["x", "", "5", ""]),
            (
                "if (x > 10) then: print(x) end",
                &[
                    If, OpenParen, Name, Gt, Int, CloseParen, Then, Colon, Name, OpenParen, Name,
                    CloseParen, End, Eof,
                ],
                &[
                    "", "", "x", "", "10", "", "", "", "print", "", "x", "", "", "",
                ],
            ),
            (
                "// This is a comment\nx = 5",
                &[Name, Assign, Int, Eof],
                &["x", "", "5", ""],
            ),
            ("\"Hello, World!\"", &[Str, Eof], &["Hello, World!", ""]),
            (
                "x == y and z != w",
                &[Name, EqualEqual, Name, And, Name, BangEqual, Name, Eof],
                &["x", "", "y", "", "z", "", "w", ""],
            ),
            (
                "arr[0] = 42",
                &[Name, OpenBracket, Int, CloseBracket, Assign, Int, Eof],
                &["arr", "", "0", "", "", "42", ""],
            ),
            (
                "fun add(a, b): return a + b end",
                &[
                    Fun, Name, OpenParen, Name, Comma, Name, CloseParen, Colon, Return, Name, Plus,
                    Name, End, Eof,
                ],
                &[
                    "", "add", "", "a", "", "b", "", "", "", "a", "", "b", "", "",
                ],
            ),
            (
                "try: x = 1/0 catch (err): print(err) end",
                &[
                    Try, Colon, Name, Assign, Int, Slash, Int, Catch, OpenParen, Name, CloseParen,
                    Colon, Name, OpenParen, Name, CloseParen, End, Eof,
                ],
                &[
                    "", "", "x", "", "1", "", "0", "", "", "err", "", "", "print", "", "err", "",
                    "", "",
                ],
            ),
            (
                "power = a ** b",
                &[Name, Assign, Name, StarStar, Name, Eof],
                &["power", "", "a", "", "b", ""],
            ),
            (
                "result = 2 ** 3 ** 2",
                &[Name, Assign, Int, StarStar, Int, StarStar, Int, Eof],
                &["result", "", "2", "", "3", "", "2", ""],
            ),
            (
                "/* This is a multiline comment */ x = 5",
                &[Name, Assign, Int, Eof],
                &["x", "", "5", ""],
            ),
            (
                "x = 5 /* inline comment */ + 3",
                &[Name, Assign, Int, Plus, Int, Eof],
                &["x", "", "5", "", "3", ""],
            ),
            (
                "/* \n multiline \n comment \n */ x = 10",
                &[Name, Assign, Int, Eof],
                &["x", "", "10", ""],
            ),
            (
                "// single line\n/* multiline */ x = 1",
                &[Name, Assign, Int, Eof],
                &["x", "", "1", ""],
            ),
            (
                "\n\t// This is a comment\n\tx = 5 // end-of-line comment\n\t// Another\n\ty = 10\n\t",
                &[Name, Assign, Int, Name, Assign, Int, Eof],
                &["x", "", "5", "y", "", "10", ""],
            ),
        ];
        for (src, want_kinds, want_values) in cases {
            let got = lex_all(src);
            let got_kinds: Vec<TokenKind> = got.iter().map(|(t, _)| *t).collect();
            let got_values: Vec<&str> = got.iter().map(|(_, v)| v.as_str()).collect();
            assert_eq!(&got_kinds, want_kinds, "kinds of {src:?}");
            assert_eq!(&got_values, want_values, "values of {src:?}");
        }
    }

    #[test]
    fn illegal_character_carries_a_message() {
        let (token, value) = first("@#$");
        assert_eq!(token, TokenKind::Illegal);
        assert_eq!(value, "unexpected @");
        assert_eq!(
            first("!x"),
            (TokenKind::Illegal, "expected != instead of !x".to_owned())
        );
        assert_eq!(kinds("a .. b"), [TokenKind::Name, TokenKind::Illegal]);
    }

    #[test]
    fn string_literal_values() {
        let cases = [
            (r#""Simple string""#, "Simple string"),
            (
                r#""String with \"escaped quotes\"""#,
                r#"String with "escaped quotes""#,
            ),
            (r#""String with \n newline""#, "String with \n newline"),
            (r#""String with \t tab""#, "String with \t tab"),
            (r#""String with \\ backslash""#, "String with \\ backslash"),
            (r#""cr\r""#, "cr\r"),
            (r"'single \'quoted\' \\ \n'", "single 'quoted' \\ \n"),
            (r"'bad \''", "bad '"),
            (r#"'a "b" c'"#, r#"a "b" c"#),
            (r#""a 'b' c""#, "a 'b' c"),
            (
                "`raw \\n no escape\nsecond line`",
                "raw \\n no escape\nsecond line",
            ),
            ("\"héllo wörld\"", "héllo wörld"),
        ];
        for (src, want) in cases {
            assert_eq!(first(src), (TokenKind::Str, want.to_owned()), "{src}");
        }
    }

    #[test]
    fn string_errors() {
        let cases = [
            ("\"open", "didn't find end quote in string"),
            ("'open", "didn't find end quote in string"),
            ("\"a\nb\"", "can't have newline in string"),
            ("\"a\rb\"", "can't have newline in string"),
            (r#""bad \q""#, "invalid string escape \\q"),
            (r#""bad \'""#, "invalid string escape \\'"),
            ("`open", "didn't find end backtick in multiline string"),
        ];
        for (src, want) in cases {
            assert_eq!(first(src), (TokenKind::Illegal, want.to_owned()), "{src:?}");
        }
    }

    #[test]
    fn numbers_keep_digits_and_drop_separators() {
        let cases = [
            ("123", TokenKind::Int, "123"),
            ("0", TokenKind::Int, "0"),
            ("3.14", TokenKind::Float, "3.14"),
            ("0.5", TokenKind::Float, "0.5"),
            ("5.", TokenKind::Float, "5."),
            ("-42", TokenKind::Minus, ""),
            ("1e5", TokenKind::Float, "1e5"),
            ("2.5e-3", TokenKind::Float, "2.5e-3"),
            ("9.461e15", TokenKind::Float, "9.461e15"),
            ("1E10", TokenKind::Float, "1E10"),
            ("3.14E+2", TokenKind::Float, "3.14E+2"),
            ("6.022e-23", TokenKind::Float, "6.022e-23"),
            ("1_000", TokenKind::Int, "1000"),
            ("1_000_000", TokenKind::Int, "1000000"),
            ("3_14.15_92", TokenKind::Float, "314.1592"),
            ("1_000.5_00", TokenKind::Float, "1000.500"),
            ("1_23e4_56", TokenKind::Float, "123e456"),
            ("9_87.6_54e-3_21", TokenKind::Float, "987.654e-321"),
        ];
        for (src, token, value) in cases {
            let got = first(src);
            assert_eq!(got.0, token, "{src}");
            if !value.is_empty() {
                assert_eq!(got.1, value, "{src}");
            }
        }
        // A name glued to a number is a separate token, except `e`, which starts an exponent.
        assert_eq!(
            kinds("1abc"),
            [TokenKind::Int, TokenKind::Name, TokenKind::Eof]
        );
        assert_eq!(
            first("1else"),
            (
                TokenKind::Illegal,
                "expected digit after exponent".to_owned()
            )
        );
    }

    #[test]
    fn malformed_numbers_are_refused() {
        let cases = [
            ("1__000", "consecutive digit separators '_' are not allowed"),
            ("1_000_", "number cannot end with digit separator '_'"),
            ("1_.5", "digit separator '_' cannot be followed by '.'"),
            (
                "1e2__3",
                "consecutive digit separators '_' are not allowed in exponent",
            ),
            ("1e2_", "exponent cannot end with digit separator '_'"),
            ("1.2.3", "unexpected second '.' in number"),
            ("1..2", "unexpected second '.' in number"),
            ("1e", "expected digit after exponent"),
            ("1e+", "expected digit after exponent"),
            ("1e_5", "expected digit after exponent"),
        ];
        for (src, want) in cases {
            assert_eq!(first(src), (TokenKind::Illegal, want.to_owned()), "{src}");
        }
    }

    #[test]
    fn unclosed_block_comment_is_illegal() {
        let got = lex_all("x = 5 /* this comment never ends");
        let got_kinds: Vec<TokenKind> = got.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            got_kinds,
            [
                TokenKind::Name,
                TokenKind::Assign,
                TokenKind::Int,
                TokenKind::Illegal
            ]
        );
        assert_eq!(got[3].1, "unterminated multiline comment");
        // `/*/` does not close itself.
        assert_eq!(last_value("/*/"), "unterminated multiline comment");
        assert_eq!(kinds("/**/ x"), [TokenKind::Name, TokenKind::Eof]);
    }

    fn last_value(src: &str) -> String {
        lex_all(src).pop().expect("one token").1
    }

    #[test]
    fn comment_edge_cases() {
        assert_eq!(kinds("//"), [TokenKind::Eof]);
        assert_eq!(kinds("// no newline at the end"), [TokenKind::Eof]);
        assert_eq!(
            kinds("a / b"),
            [
                TokenKind::Name,
                TokenKind::Slash,
                TokenKind::Name,
                TokenKind::Eof
            ]
        );
        assert_eq!(
            kinds("a /"),
            [TokenKind::Name, TokenKind::Slash, TokenKind::Eof]
        );
        assert_eq!(
            kinds("a /= 2"),
            [
                TokenKind::Name,
                TokenKind::SlashEqual,
                TokenKind::Int,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn operators_and_keywords() {
        use TokenKind::*;
        assert_eq!(
            kinds("+ += - -= * *= ** / /= % %= == != <= >= < > = ? : , . ... ( ) [ ] { }"),
            [
                Plus,
                PlusEqual,
                Minus,
                MinusEqual,
                Star,
                StarEqual,
                StarStar,
                Slash,
                SlashEqual,
                Percent,
                PercentEqual,
                EqualEqual,
                BangEqual,
                Le,
                Ge,
                Lt,
                Gt,
                Assign,
                Question,
                Colon,
                Comma,
                Dot,
                Ellipsis,
                OpenParen,
                CloseParen,
                OpenBracket,
                CloseBracket,
                OpenBrace,
                CloseBrace,
                Eof
            ]
        );
        assert_eq!(
            kinds(
                "and break catch continue elif else end false for fun if import in memo null not or return \
                 then true try while xor"
            ),
            [
                And, Break, Catch, Continue, Elif, Else, End, False, For, Fun, If, Import, In,
                Memo, Null, Not, Or, Return, Then, True, Try, While, Xor, Eof
            ]
        );
        // Keywords are case sensitive and a longer name is a name.
        assert_eq!(kinds("And iff _x x_1"), [Name, Name, Name, Name, Eof]);
    }

    #[test]
    fn positions_count_characters() {
        let mut lexer = Lexer::new("a\n  \"é\" b");
        assert_eq!(lexer.next_token().0, Position { line: 1, column: 1 });
        assert_eq!(lexer.next_token().0, Position { line: 2, column: 3 });
        // The string is three characters and four bytes, so `b` is at column 7.
        assert_eq!(lexer.next_token().0, Position { line: 2, column: 7 });
        assert_eq!(lexer.next_token().0, Position { line: 2, column: 8 });
    }

    #[test]
    fn replacement_character_is_refused_like_a_bad_byte() {
        let got = lex_all("x = \"a\u{FFFD}b\"");
        let last = got.last().expect("tokens");
        assert_eq!(last.0, TokenKind::Illegal);
        let got = lex_all("x // \u{FFFD}\ny = 1");
        assert_eq!(got.last().expect("tokens").0, TokenKind::Illegal);
        assert_eq!(
            first("\u{FFFD}"),
            (TokenKind::Illegal, "invalid UTF-8 byte 0xef".to_owned())
        );
    }
}
