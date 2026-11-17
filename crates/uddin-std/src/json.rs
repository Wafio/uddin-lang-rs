//! JSON: json_parse json_stringify.
//!
//! Both follow the JSON library the reference uses, not the standard one. The reader takes a number as a float
//! and turns it into an integer when that loses nothing, accepts any run of digits, signs, dots and exponent
//! letters that the reference's float parser accepts, ends at the first NUL byte, keeps invalid UTF-8 in strings
//! as it is, and reads up to 10,000 levels of nesting. The writer escapes `<`, `>`, `&` and the two line
//! separators, replaces each invalid UTF-8 byte by U+FFFD, sorts the keys of an object by their escaped text,
//! and writes a float in the fixed form between `1e-6` and `1e21` and in the exponent form (`1e-07`) outside.

use uddin_vm::{
    BuiltinDef, Count, Exc, Fresh, JSON_BYTE_COST, LimitReason, MAP_COST, Rt, Str, Stringify,
    Value, checked_product, error_text_parts, float_to_int, format_float_json, format_int,
    sum_estimate, type_name,
};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("json_parse", json_parse, 1)
        .reserve(parse_reserve)
        .fresh(Fresh::Tree),
    BuiltinDef::new("json_stringify", json_stringify, 1)
        .deep()
        .stringify(Stringify::Json),
];

/// Nesting deeper than this is refused by the reader.
const MAX_DEPTH: usize = 10_000;

/// The text of the error the writer gives for a function: the type of the field that holds it.
const FUNC_TYPE: &str =
    "func(*interpreter.interpreter, interpreter.Position, []interpreter.Value) interpreter.Value";
/// The host's request context holds a lookup function the writer cannot encode.
const HOST_TYPE: &str = "func(string) []string";

/// What a call reserves before it parses: the worst case of what the reader allocates on the way.
fn parse_reserve(rt: &mut Rt<'_>, args: &[Value]) -> Result<i64, Exc> {
    let n = match args.first() {
        Some(Value::Str(s)) => {
            sum_estimate(MAP_COST, checked_product(JSON_BYTE_COST, i64::from(s.len)))
        }
        _ => 0,
    };
    rt.reserve(n)
}

/// Why a read stopped: the reader's own error text, or an error of the run.
enum Fail {
    Json(String),
    Run(Exc),
}

impl From<Exc> for Fail {
    fn from(e: Exc) -> Fail {
        Fail::Run(e)
    }
}

/// What `%c` makes of a byte: the character of that code.
fn show(c: u8) -> char {
    char::from(c)
}

fn invalid_begin(c: u8) -> Fail {
    Fail::Json(format!(
        "invalid character '{}' looking for beginning of value",
        show(c)
    ))
}

fn unexpected_end(what: &str) -> Fail {
    Fail::Json(format!("json: {what} unexpected end of JSON input"))
}

fn invalid_char(c: u8, what: &str) -> Fail {
    if c == 0 {
        Fail::Json(format!("json: invalid character as {what}"))
    } else {
        Fail::Json(format!("json: invalid character {} as {what}", show(c)))
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

/// Bytes a number may be made of, and bytes that may follow it.
fn is_number_byte(c: u8) -> bool {
    c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')
}

fn ends_number(c: u8) -> bool {
    matches!(
        c,
        0 | b' ' | b'\t' | b'\r' | b'\n' | b',' | b':' | b'}' | b']'
    )
}

/// The reference's float parser on a run of the bytes a number is made of: the digits with an optional sign,
/// point and exponent, rounded to the nearest float; a result too large for a float is an error.
fn parse_float(text: &[u8]) -> Result<f64, Fail> {
    let quoted = || String::from_utf8_lossy(text).into_owned();
    let fail = |why: &str| {
        Fail::Json(format!(
            "strconv.ParseFloat: parsing \"{}\": {why}",
            quoted()
        ))
    };
    let mut i = 0;
    if matches!(text.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let mut digits = 0;
    let mut dot = false;
    while i < text.len() {
        match text[i] {
            b'0'..=b'9' => digits += 1,
            b'.' if !dot => dot = true,
            _ => break,
        }
        i += 1;
    }
    if digits == 0 {
        return Err(fail("invalid syntax"));
    }
    if i < text.len() && matches!(text[i], b'e' | b'E') {
        i += 1;
        if matches!(text.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let from = i;
        while i < text.len() && text[i].is_ascii_digit() {
            i += 1;
        }
        if i == from {
            return Err(fail("invalid syntax"));
        }
    }
    if i != text.len() {
        return Err(fail("invalid syntax"));
    }
    let Ok(s) = std::str::from_utf8(text) else {
        return Err(fail("invalid syntax"));
    };
    match s.parse::<f64>() {
        Ok(f) if f.is_infinite() => Err(fail("value out of range")),
        Ok(f) => Ok(f),
        Err(_) => Err(fail("invalid syntax")),
    }
}

fn hex_value(c: u8) -> u32 {
    match c {
        b'0'..=b'9' => u32::from(c - b'0'),
        b'a'..=b'f' => u32::from(c - b'a' + 10),
        b'A'..=b'F' => u32::from(c - b'A' + 10),
        _ => 0,
    }
}

fn hex4(b: &[u8]) -> u32 {
    b.iter().fold(0, |acc, &c| acc << 4 | hex_value(c))
}

fn push_rune(out: &mut Vec<u8>, code: u32) {
    let c = char::from_u32(code).unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// The text a string literal means: its escapes replaced, a surrogate pair joined, a lone surrogate replaced by
/// U+FFFD. The literal was checked while it was scanned, so every escape is whole.
fn unescape(lit: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < lit.len() {
        let c = lit[i];
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        let e = lit[i + 1];
        if e != b'u' {
            out.push(match e {
                b'b' => 8,
                b'f' => 12,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                other => other,
            });
            i += 2;
            continue;
        }
        let mut code = hex4(&lit[i + 2..i + 6]);
        if (0xd800..0xdc00).contains(&code)
            && i + 11 < lit.len()
            && lit[i + 6] == b'\\'
            && lit[i + 7] == b'u'
        {
            let low = hex4(&lit[i + 8..i + 12]);
            if (0xdc00..0xe000).contains(&low) {
                code = ((code - 0xd800) << 10 | (low - 0xdc00)) + 0x10000;
                i += 6;
            }
        }
        push_rune(out, code);
        i += 6;
    }
}

#[derive(Clone, Copy)]
enum Frame {
    /// An array whose elements start at this index of the value stack.
    Array(usize),
    /// An object whose keys and values, in turn, start at this index.
    Object(usize),
}

struct Reader<'a, 'r> {
    rt: &'a mut Rt<'r>,
    /// The document and a NUL after it, which the reader treats as the end of the input.
    buf: &'a [u8],
    cur: usize,
    vals: Vec<Value>,
    frames: Vec<Frame>,
    text: Vec<u8>,
}

impl Reader<'_, '_> {
    fn at(&self, i: usize) -> u8 {
        self.buf.get(i).copied().unwrap_or(0)
    }

    fn skip_space(&mut self) {
        while is_space(self.at(self.cur)) {
            self.cur += 1;
        }
    }

    fn run_oom(&self) -> Exc {
        Exc::limit(LimitReason::Memory, self.rt.pos)
    }

    fn push_val(&mut self, v: Value) -> Result<(), Fail> {
        self.vals
            .try_reserve(1)
            .map_err(|_| Fail::Run(self.run_oom()))?;
        self.vals.push(v);
        Ok(())
    }

    fn push_frame(&mut self, f: Frame) -> Result<(), Fail> {
        self.frames
            .try_reserve(1)
            .map_err(|_| Fail::Run(self.run_oom()))?;
        self.frames.push(f);
        Ok(())
    }

    fn read(&mut self) -> Result<Value, Fail> {
        let mut pending = self.begin()?;
        loop {
            if let Some(v) = pending.take() {
                match self.frames.last().copied() {
                    None => return self.finish(v),
                    Some(Frame::Array(start)) => {
                        self.push_val(v)?;
                        self.skip_space();
                        match self.at(self.cur) {
                            b']' => {
                                self.cur += 1;
                                pending = Some(self.close_array(start)?);
                                self.frames.pop();
                                continue;
                            }
                            b',' => self.cur += 1,
                            c => return Err(invalid_char(c, "slice")),
                        }
                    }
                    Some(Frame::Object(start)) => {
                        self.push_val(v)?;
                        self.skip_space();
                        match self.at(self.cur) {
                            b'}' => {
                                self.cur += 1;
                                pending = Some(self.close_object(start)?);
                                self.frames.pop();
                                continue;
                            }
                            b',' => {
                                self.cur += 1;
                                self.key()?;
                            }
                            _ => {
                                return Err(Fail::Json(
                                    "expected comma after object value".to_owned(),
                                ));
                            }
                        }
                    }
                }
            }
            pending = self.begin()?;
        }
    }

    /// After the top-level value only white space may follow, up to the end.
    fn finish(&mut self, v: Value) -> Result<Value, Fail> {
        self.skip_space();
        match self.at(self.cur) {
            0 => Ok(v),
            c => Err(Fail::Json(format!(
                "invalid character '{}' after top-level value",
                show(c)
            ))),
        }
    }

    /// Reads the start of a value: a whole one, or `None` after opening an array or object that has members, with
    /// the first element (for an object, its key and the colon) consumed.
    fn begin(&mut self) -> Result<Option<Value>, Fail> {
        self.rt.work(Count::Free, 1)?;
        self.skip_space();
        match self.at(self.cur) {
            b'{' => {
                self.deeper(b'{')?;
                self.cur += 1;
                self.skip_space();
                if self.at(self.cur) == b'}' {
                    self.cur += 1;
                    let m = self.rt.heap.new_map(0).map_err(self.rt.oom())?;
                    return Ok(Some(Value::Map(m)));
                }
                let start = self.vals.len();
                self.push_frame(Frame::Object(start))?;
                self.key()?;
                Ok(None)
            }
            b'[' => {
                self.deeper(b'[')?;
                self.cur += 1;
                self.skip_space();
                if self.at(self.cur) == b']' {
                    self.cur += 1;
                    let l = self.rt.heap.new_list(0, 0).map_err(self.rt.oom())?;
                    return Ok(Some(Value::List(l)));
                }
                let start = self.vals.len();
                self.push_frame(Frame::Array(start))?;
                Ok(None)
            }
            b'-' | b'0'..=b'9' => self.number().map(Some),
            b'"' => Ok(Some(Value::Str(self.string()?))),
            b't' => {
                self.literal(b"true")?;
                Ok(Some(Value::Bool(true)))
            }
            b'f' => {
                self.literal(b"false")?;
                Ok(Some(Value::Bool(false)))
            }
            b'n' => {
                self.literal(b"null")?;
                Ok(Some(Value::Null))
            }
            c => Err(invalid_begin(c)),
        }
    }

    fn deeper(&self, open: u8) -> Result<(), Fail> {
        if self.frames.len() + 1 > MAX_DEPTH {
            return Err(Fail::Json(format!(
                "invalid character \"{}\" exceeded max depth",
                show(open)
            )));
        }
        Ok(())
    }

    /// `true`, `false` or `null`: the reader looks at the next bytes of the word, and not at what follows it.
    fn literal(&mut self, word: &[u8]) -> Result<(), Fail> {
        let name = std::str::from_utf8(word).unwrap_or("");
        if self.cur + word.len() > self.buf.len() {
            return Err(unexpected_end(name));
        }
        for (k, &w) in word.iter().enumerate().skip(1) {
            let c = self.at(self.cur + k);
            if c != w {
                return Err(invalid_char(c, name));
            }
        }
        self.cur += word.len();
        Ok(())
    }

    fn number(&mut self) -> Result<Value, Fail> {
        let start = self.cur;
        self.cur += 1;
        while is_number_byte(self.at(self.cur)) {
            self.cur += 1;
        }
        if !ends_number(self.at(self.cur)) {
            return Err(unexpected_end("float"));
        }
        let f = parse_float(&self.buf[start..self.cur])?;
        let n = float_to_int(f);
        Ok(if n as f64 == f {
            Value::Int(n)
        } else {
            Value::Float(f)
        })
    }

    /// A string literal at the cursor, as a string of the heap.
    fn string(&mut self) -> Result<Str, Fail> {
        self.cur += 1;
        let start = self.cur;
        let mut escaped = false;
        loop {
            match self.at(self.cur) {
                b'\\' => {
                    escaped = true;
                    self.cur += 1;
                    match self.at(self.cur) {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => self.cur += 1,
                        b'u' => {
                            if self.cur + 5 >= self.buf.len() {
                                return Err(unexpected_end("escaped string"));
                            }
                            for k in 1..=4 {
                                let c = self.at(self.cur + k);
                                if !c.is_ascii_hexdigit() {
                                    return Err(Fail::Json(format!(
                                        "json: invalid character {} in \\u hexadecimal character escape",
                                        show(c)
                                    )));
                                }
                            }
                            self.cur += 5;
                        }
                        _ => return Err(unexpected_end("escaped string")),
                    }
                }
                b'"' => break,
                0 => return Err(unexpected_end("string")),
                _ => self.cur += 1,
            }
        }
        let lit = &self.buf[start..self.cur];
        self.cur += 1;
        let s = if escaped {
            self.text.clear();
            self.text
                .try_reserve(lit.len())
                .map_err(|_| Fail::Run(Exc::limit(LimitReason::Memory, self.rt.pos)))?;
            unescape(lit, &mut self.text);
            self.rt.heap.new_str(&self.text)
        } else {
            self.rt.heap.new_str(lit)
        };
        s.map_err(|_| Fail::Run(Exc::limit(LimitReason::Memory, self.rt.pos)))
    }

    /// An object key and the colon after it, kept on the value stack. `null` is a key too, the empty one.
    fn key(&mut self) -> Result<(), Fail> {
        self.skip_space();
        let key = match self.at(self.cur) {
            b'[' => return Err(not_a_string("array")),
            b'{' => return Err(not_a_string("object")),
            b'-' | b'0'..=b'9' => return Err(not_a_string("number")),
            b'"' => self.string()?,
            b'n' => {
                self.literal(b"null")?;
                Str::EMPTY
            }
            c => return Err(invalid_begin(c)),
        };
        self.skip_space();
        if self.at(self.cur) != b':' {
            return Err(Fail::Json("expected colon after object key".to_owned()));
        }
        self.cur += 1;
        self.push_val(Value::Str(key))
    }

    /// The array of the elements on the value stack from `start`, of exactly their number.
    fn close_array(&mut self, start: usize) -> Result<Value, Fail> {
        let n = self.vals.len() - start;
        let list = self.rt.heap.new_list(n, n).map_err(self.rt.oom())?;
        for i in start..self.vals.len() {
            self.rt
                .heap
                .list_push(list, self.vals[i])
                .map_err(self.rt.oom())?;
        }
        self.vals.truncate(start);
        Ok(Value::List(list))
    }

    /// The object of the keys and values on the value stack from `start`; a key that comes again takes the later
    /// value.
    fn close_object(&mut self, start: usize) -> Result<Value, Fail> {
        let n = (self.vals.len() - start) / 2;
        let map = self.rt.heap.new_map(n).map_err(self.rt.oom())?;
        for i in 0..n {
            let (Value::Str(k), v) = (self.vals[start + 2 * i], self.vals[start + 2 * i + 1])
            else {
                unreachable!("an object keeps its keys and values in turn");
            };
            self.rt.heap.map_set(map, k, v).map_err(self.rt.oom())?;
        }
        self.vals.truncate(start);
        Ok(Value::Map(map))
    }
}

fn not_a_string(what: &str) -> Fail {
    Fail::Json(format!(
        "json: cannot unmarshal {what} into Go value of type string"
    ))
}

fn json_parse(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Some(&arg) = args.first() else {
        return Err(rt.arity_error("json_parse", 1, 0));
    };
    let Value::Str(s) = arg else {
        return Err(rt.type_error(format!(
            "json_parse() requires a string argument, not {}",
            type_name(arg)
        )));
    };
    rt.work(Count::Free, u64::from(s.len))?;
    let pos = rt.pos;
    let memory = || Exc::limit(LimitReason::Memory, pos);
    let mut buf = Vec::new();
    buf.try_reserve(s.len as usize + 1).map_err(|_| memory())?;
    buf.extend_from_slice(rt.heap.str_bytes(s));
    buf.push(0);
    let mut reader = Reader {
        rt,
        buf: &buf,
        cur: 0,
        vals: Vec::new(),
        frames: Vec::new(),
        text: Vec::new(),
    };
    match reader.read() {
        Ok(v) => Ok(v),
        Err(Fail::Run(e)) => Err(e),
        Err(Fail::Json(msg)) => Err(Exc::value_error(pos, format!("Invalid JSON: {msg}"))),
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// The part of a UTF-8 sequence the writer needs to tell: whole, broken, or one of the two line separators.
enum Rune {
    Valid(usize),
    Broken,
    LineSeparator,
    ParagraphSeparator,
}

/// Reads the sequence at the start of `s` as the standard library's decoder does.
fn decode_rune(s: &[u8]) -> Rune {
    let s0 = s[0];
    let (size, accept) = match s0 {
        0x00..=0x7f => return Rune::Valid(1),
        0xc2..=0xdf => (2, (0x80, 0xbf)),
        0xe0 => (3, (0xa0, 0xbf)),
        0xe1..=0xec | 0xee..=0xef => (3, (0x80, 0xbf)),
        0xed => (3, (0x80, 0x9f)),
        0xf0 => (4, (0x90, 0xbf)),
        0xf1..=0xf3 => (4, (0x80, 0xbf)),
        0xf4 => (4, (0x80, 0x8f)),
        _ => return Rune::Broken,
    };
    if s.len() < size {
        return Rune::Broken;
    }
    if s[1] < accept.0 || s[1] > accept.1 {
        return Rune::Broken;
    }
    if size == 2 {
        return Rune::Valid(2);
    }
    if !(0x80..=0xbf).contains(&s[2]) {
        return Rune::Broken;
    }
    if size == 3 {
        if s0 == 0xe2 && s[1] == 0x80 {
            match s[2] {
                0xa8 => return Rune::LineSeparator,
                0xa9 => return Rune::ParagraphSeparator,
                _ => {}
            }
        }
        return Rune::Valid(3);
    }
    if !(0x80..=0xbf).contains(&s[3]) {
        return Rune::Broken;
    }
    Rune::Valid(4)
}

fn needs_escape(c: u8) -> bool {
    c < 0x20 || matches!(c, b'"' | b'\\' | b'<' | b'>' | b'&') || c >= 0x80
}

fn push_u00(out: &mut Vec<u8>, c: u8) {
    out.extend_from_slice(b"\\u00");
    out.push(HEX[usize::from(c >> 4)]);
    out.push(HEX[usize::from(c & 15)]);
}

/// A string in quotes, escaped for HTML and cleaned of invalid UTF-8.
fn push_string(out: &mut Vec<u8>, s: &[u8]) {
    out.push(b'"');
    let (mut i, mut j) = (0, 0);
    while j < s.len() {
        let c = s[j];
        if !needs_escape(c) {
            j += 1;
            continue;
        }
        match c {
            b'\\' | b'"' => {
                out.extend_from_slice(&s[i..j]);
                out.extend_from_slice(&[b'\\', c]);
            }
            b'\n' => {
                out.extend_from_slice(&s[i..j]);
                out.extend_from_slice(b"\\n");
            }
            b'\r' => {
                out.extend_from_slice(&s[i..j]);
                out.extend_from_slice(b"\\r");
            }
            b'\t' => {
                out.extend_from_slice(&s[i..j]);
                out.extend_from_slice(b"\\t");
            }
            b'<' | b'>' | b'&' | 0..=0x1f => {
                out.extend_from_slice(&s[i..j]);
                push_u00(out, c);
            }
            _ => match decode_rune(&s[j..]) {
                Rune::Valid(size) => {
                    j += size;
                    continue;
                }
                Rune::Broken => {
                    out.extend_from_slice(&s[i..j]);
                    out.extend_from_slice(b"\\ufffd");
                }
                Rune::LineSeparator => {
                    out.extend_from_slice(&s[i..j]);
                    out.extend_from_slice(b"\\u2028");
                    i = j + 3;
                    j += 3;
                    continue;
                }
                Rune::ParagraphSeparator => {
                    out.extend_from_slice(&s[i..j]);
                    out.extend_from_slice(b"\\u2029");
                    i = j + 3;
                    j += 3;
                    continue;
                }
            },
        }
        i = j + 1;
        j += 1;
    }
    out.extend_from_slice(&s[i..]);
    out.push(b'"');
}

fn convert_failed(rt: &Rt<'_>, why: &str) -> Exc {
    Exc::value_error(rt.pos, format!("Failed to convert to JSON: {why}"))
}

/// Writes `v` after `out`. An object comes out with its keys in the byte order of their escaped text.
fn encode(rt: &mut Rt<'_>, v: Value, out: &mut Vec<u8>, depth: usize) -> Result<(), Exc> {
    if depth > 1000 {
        return Err(Exc::limit(LimitReason::ValueDepth, rt.pos));
    }
    rt.work(Count::Free, 1)?;
    let pos = rt.pos;
    let memory = move || Exc::limit(LimitReason::Memory, pos);
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Int(i) => format_int(i, out),
        Value::Float(f) if f.is_nan() => {
            return Err(convert_failed(rt, "json: unsupported value: NaN"));
        }
        Value::Float(f) if f.is_infinite() => {
            let name = if f > 0.0 { "+Inf" } else { "-Inf" };
            return Err(convert_failed(
                rt,
                &format!("json: unsupported value: {name}"),
            ));
        }
        Value::Float(f) => format_float_json(f, out),
        Value::Str(s) => {
            rt.work(Count::Free, u64::from(s.len))?;
            out.try_reserve(s.len as usize + 2).map_err(|_| memory())?;
            push_string(out, rt.heap.str_bytes(s));
        }
        Value::List(l) => {
            let n = rt.heap.list(l).items.len();
            out.push(b'[');
            for i in 0..n {
                if i > 0 {
                    out.push(b',');
                }
                let item = rt.heap.list(l).items[i];
                encode(rt, item, out, depth + 1)?;
            }
            out.push(b']');
        }
        Value::Map(m) => {
            let n = rt.heap.map_len(m);
            let mut items: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            items.try_reserve(n).map_err(|_| memory())?;
            for i in 0..n {
                let (k, e) = rt.heap.map_entries(m)[i];
                let mut key = Vec::new();
                push_string(&mut key, rt.heap.str_bytes(k));
                let mut value = Vec::new();
                encode(rt, e, &mut value, depth + 1)?;
                items.push((key, value));
            }
            items.sort_by(|a, b| a.0.cmp(&b.0));
            out.push(b'{');
            for (i, (key, value)) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(key);
                out.push(b':');
                out.extend_from_slice(value);
            }
            out.push(b'}');
        }
        Value::Func(_) | Value::Builtin(_) => {
            return Err(convert_failed(
                rt,
                &format!("json: unsupported type: {FUNC_TYPE}"),
            ));
        }
        Value::Host(_) => {
            return Err(convert_failed(
                rt,
                &format!("json: unsupported type: {HOST_TYPE}"),
            ));
        }
        // An error of a class is a struct with one field that the reference can see, its message.
        Value::Error(s, Some(_)) => match error_text_parts(rt.heap.str_bytes(s)) {
            Some((_, _, msg)) => {
                out.extend_from_slice(b"{\"Message\":");
                push_string(out, msg.as_bytes());
                out.push(b'}');
            }
            None => out.extend_from_slice(b"{}"),
        },
        Value::Set(_) | Value::Stack(_) | Value::Queue(_) | Value::Error(..) => {
            out.extend_from_slice(b"{}");
        }
    }
    Ok(())
}

fn json_stringify(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let Some(&v) = args.first() else {
        return Err(rt.arity_error("json_stringify", 1, 0));
    };
    let mut out = std::mem::take(&mut *rt.scratch);
    out.clear();
    let done = encode(rt, v, &mut out, 0);
    let made = done.and_then(|()| rt.heap.new_str(&out).map_err(rt.oom()));
    *rt.scratch = out;
    Ok(Value::Str(made?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_is_what_the_reference_reads() {
        for (text, want) in [
            ("1", 1.0),
            ("-0.5", -0.5),
            ("1.", 1.0),
            ("-.5", -0.5),
            ("1e3", 1000.0),
            ("01", 1.0),
            ("1E+2", 100.0),
            ("1e-999", 0.0),
        ] {
            assert_eq!(parse_float(text.as_bytes()).ok(), Some(want), "{text}");
        }
        for text in ["-", "1e", "1e+", "--1", "1.2.3", "e5", ".", "1e2e3", "+"] {
            assert!(parse_float(text.as_bytes()).is_err(), "{text}");
        }
        match parse_float(b"1e999") {
            Err(Fail::Json(msg)) => assert_eq!(
                msg,
                "strconv.ParseFloat: parsing \"1e999\": value out of range"
            ),
            _ => panic!("a number past the largest float is an error"),
        }
    }

    fn quoted(bytes: &[u8]) -> String {
        let mut out = Vec::new();
        push_string(&mut out, bytes);
        String::from_utf8(out).expect("the writer gives UTF-8")
    }

    #[test]
    fn a_string_is_escaped_for_html_and_cleaned_of_invalid_bytes() {
        assert_eq!(quoted(b""), "\"\"");
        assert_eq!(quoted(b"a<b>&c"), "\"a\\u003cb\\u003e\\u0026c\"");
        assert_eq!(quoted(b"\t\n\r\x01\x7f"), "\"\\t\\n\\r\\u0001\x7f\"");
        assert_eq!(quoted(b"a\xffb\xc3"), "\"a\\ufffdb\\ufffd\"");
        assert_eq!(quoted("é日".as_bytes()), "\"é日\"");
        assert_eq!(quoted("\u{2028}\u{2029}".as_bytes()), "\"\\u2028\\u2029\"");
        assert_eq!(quoted(b"\xe6\x97"), "\"\\ufffd\\ufffd\"");
    }

    #[test]
    fn escapes_become_characters_and_a_lone_surrogate_is_replaced() {
        let mut out = Vec::new();
        unescape(br"a\n\u00e9\ud83d\ude00\ud800x\/", &mut out);
        assert_eq!(out, "a\n\u{e9}\u{1f600}\u{fffd}x/".as_bytes());
    }
}
