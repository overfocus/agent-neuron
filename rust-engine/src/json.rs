// A closed JSON value with parser and writer, standard library only.
//
// The engine reads network files, fixture files and protocol lines as JSON and
// writes events as JSON. Amounts in budgets are integers the JSON readers hold
// exactly (whole numbers up to 2^53 - 1), so numbers keep an integer form when
// the text has one.
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        // A JSON integer: an exact integer, however written. 3.0 counts
        // (a schema "integer" admits it); 3.5 does not.
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 && f.is_finite() => Some(*f as i64),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        self.as_i64().and_then(|i| u64::try_from(i).ok())
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(items) => Some(items),
            _ => None,
        }
    }
    pub fn is_obj(&self) -> bool {
        matches!(self, Value::Obj(_))
    }
    pub fn keys(&self) -> Vec<&str> {
        match self {
            Value::Obj(entries) => entries.iter().map(|(k, _)| k.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    pub fn deep_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => *b == *a as f64,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Arr(a), Value::Arr(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.deep_eq(y)),
            (Value::Obj(a), Value::Obj(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v.deep_eq(v2)))
            }
            _ => false,
        }
    }

    /// Does `self` (an event) carry every field of `subset` with an equal value?
    pub fn covers(&self, subset: &Value) -> bool {
        match (self, subset) {
            (Value::Obj(a), Value::Obj(b)) => b.iter().all(|(k, v)| a.iter().any(|(k2, v2)| k == k2 && v2.deep_eq(v))),
            _ => self.deep_eq(subset),
        }
    }
}

pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser {
        bytes: text.as_bytes(),
        pos: 0,
    };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(format!("trailing bytes at byte {}", p.pos));
    }
    Ok(v)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
    fn eat(&mut self, b: u8) -> Result<(), String> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", b as char, self.pos))
        }
    }
    fn literal(&mut self, word: &str, v: Value) -> Result<Value, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(v)
        } else {
            Err(format!("bad literal at byte {}", self.pos))
        }
    }
    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(format!("unexpected byte at {}", self.pos)),
        }
    }
    fn object(&mut self) -> Result<Value, String> {
        self.eat(b'{')?;
        let mut entries: Vec<(String, Value)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Obj(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.eat(b':')?;
            self.skip_ws();
            let v = self.value()?;
            entries.push((key, v));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Obj(entries));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.pos)),
            }
        }
    }
    fn array(&mut self) -> Result<Value, String> {
        self.eat(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Arr(items));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.pos)),
            }
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or_else(|| "unterminated string".to_string())?;
            self.pos += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.peek().ok_or_else(|| "unterminated escape".to_string())?;
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let ch = if (0xd800..0xdc00).contains(&hi) {
                                // A surrogate pair: expect \uDC00-\uDFFF next.
                                if self.peek() != Some(b'\\') {
                                    return Err("lone high surrogate".into());
                                }
                                self.pos += 1;
                                self.eat(b'u')?;
                                let lo = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&lo) {
                                    return Err("bad low surrogate".into());
                                }
                                let c = 0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00);
                                char::from_u32(c).ok_or("bad surrogate pair")?
                            } else if (0xdc00..0xe000).contains(&hi) {
                                return Err("lone low surrogate".into());
                            } else {
                                char::from_u32(hi).ok_or("bad \\u escape")?
                            };
                            out.push(ch);
                        }
                        _ => return Err(format!("bad escape \\{}", e as char)),
                    }
                }
                _ => {
                    // Collect the raw UTF-8 sequence starting at c.
                    let start = self.pos - 1;
                    let len = utf8_len(c);
                    self.pos = start + len;
                    if self.pos > self.bytes.len() {
                        return Err("truncated UTF-8".into());
                    }
                    let piece = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| "bad UTF-8".to_string())?;
                    out.push_str(piece);
                }
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        if self.pos + 4 > self.bytes.len() {
            return Err("short \\u escape".into());
        }
        let s = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4]).map_err(|_| "bad \\u".to_string())?;
        let v = u32::from_str_radix(s, 16).map_err(|_| "bad \\u".to_string())?;
        self.pos += 4;
        Ok(v)
    }
    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| "bad number".to_string())?;
        if !is_float {
            if let Ok(i) = text.parse::<i64>() {
                return Ok(Value::Int(i));
            }
        }
        text.parse::<f64>().map(Value::Float).map_err(|_| format!("bad number {}", text))
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

/// Compact JSON, in the form the fixtures and traces use: no spaces, keys in
/// insertion order (they were parsed in document order, so this reproduces a
/// file's order). Only for printing differences; comparison never stringifies.
pub fn write(v: &Value) -> String {
    let mut out = String::new();
    write_into(v, &mut out);
    out
}

fn write_into(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(i) => {
            let _ = write!(out, "{}", i);
        }
        Value::Float(f) => {
            let _ = write!(out, "{}", f);
        }
        Value::Str(s) => write_str(s, out),
        Value::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_into(item, out);
            }
            out.push(']');
        }
        Value::Obj(entries) => {
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write_into(v, out);
            }
            out.push('}');
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A helper for building objects: the engine writes events in a fixed field
/// order, which is also the order the fixtures write them.
pub struct Obj(pub Vec<(String, Value)>);

impl Obj {
    pub fn new() -> Self {
        Obj(Vec::new())
    }
    pub fn set(mut self, key: &str, v: Value) -> Self {
        self.0.push((key.to_string(), v));
        self
    }
    pub fn set_if(self, key: &str, v: Option<Value>) -> Self {
        match v {
            Some(v) => self.set(key, v),
            None => self,
        }
    }
    pub fn int(self, key: &str, v: i64) -> Self {
        self.set(key, Value::Int(v))
    }
    pub fn str(self, key: &str, v: impl Into<String>) -> Self {
        self.set(key, Value::Str(v.into()))
    }
    pub fn build(self) -> Value {
        Value::Obj(self.0)
    }
}

impl Default for Obj {
    fn default() -> Self {
        Self::new()
    }
}
