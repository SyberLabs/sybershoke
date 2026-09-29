//! `shoke-history/v1`: one event per line, plain text, no dependencies.
//!
//! ```text
//! shoke-history/v1
//! # comments and blank lines are ignored
//! meta seed=42
//! meta deadline_ms=8000
//! 500 req id=r1 text=tokyo%20drift
//! 503 call req=r1 n=1
//! ```
//!
//! A line is either `meta key=value` or `<time_ms> <kind> key=value ...`. Kinds and keys are
//! tokens (`A-Za-z0-9_.-`). Values are percent-encoded: every byte except letters, digits and
//! `- . _ ~ : , /` becomes `%XX`, so a value never contains a space, `=` or a newline.

use std::fmt;

pub const FORMAT: &str = "shoke-history/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub t: u64,
    pub kind: String,
    pub fields: Vec<(String, String)>,
}

impl Event {
    pub fn new(t: u64, kind: &str) -> Self {
        debug_assert!(is_token(kind), "bad event kind {kind:?}");
        Event {
            t,
            kind: kind.to_string(),
            fields: Vec::new(),
        }
    }

    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        debug_assert!(is_token(key), "bad field key {key:?}");
        self.fields.push((key.to_string(), value.to_string()));
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key)?.parse().ok()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    pub meta: Vec<(String, String)>,
    pub events: Vec<Event>,
}

impl History {
    pub fn new() -> Self {
        History::default()
    }

    /// Add a meta entry. Keys may repeat (for example one `fault` line per fault).
    pub fn add_meta(&mut self, key: &str, value: impl ToString) {
        debug_assert!(is_token(key), "bad meta key {key:?}");
        self.meta.push((key.to_string(), value.to_string()));
    }

    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn meta_u64(&self, key: &str) -> Option<u64> {
        self.meta(key)?.parse().ok()
    }

    pub fn push(&mut self, event: Event) {
        self.events.push(event);
    }

    /// Order events by time. The sort is stable, so events at the same instant keep the order
    /// they were recorded in.
    pub fn sort(&mut self) {
        self.events.sort_by_key(|e| e.t);
    }

    pub fn of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Event> + 'a {
        self.events.iter().filter(move |e| e.kind == kind)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(FORMAT);
        out.push('\n');
        for (k, v) in &self.meta {
            out.push_str(&format!("meta {}={}\n", k, encode_value(v)));
        }
        for e in &self.events {
            out.push_str(&format!("{} {}", e.t, e.kind));
            for (k, v) in &e.fields {
                out.push_str(&format!(" {}={}", k, encode_value(v)));
            }
            out.push('\n');
        }
        out
    }

    pub fn from_text(text: &str) -> Result<History, ParseError> {
        let mut history = History::new();
        let mut seen_header = false;
        for (idx, raw) in text.lines().enumerate() {
            let line_no = idx + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !seen_header {
                if line != FORMAT {
                    return Err(ParseError::new(
                        line_no,
                        format!("expected header `{FORMAT}`, found `{line}`"),
                    ));
                }
                seen_header = true;
                continue;
            }
            let mut parts = line.split_whitespace();
            let first = parts.next().unwrap_or_default();
            if first == "meta" {
                let pair = parts
                    .next()
                    .ok_or_else(|| ParseError::new(line_no, "meta line needs key=value".into()))?;
                if parts.next().is_some() {
                    return Err(ParseError::new(
                        line_no,
                        "meta line takes exactly one key=value".into(),
                    ));
                }
                let (k, v) = parse_pair(pair, line_no)?;
                history.meta.push((k, v));
                continue;
            }
            let t: u64 = first.parse().map_err(|_| {
                ParseError::new(line_no, format!("expected a time in ms, found `{first}`"))
            })?;
            let kind = parts
                .next()
                .ok_or_else(|| ParseError::new(line_no, "event line needs a kind".into()))?;
            if !is_token(kind) {
                return Err(ParseError::new(line_no, format!("bad event kind `{kind}`")));
            }
            let mut event = Event {
                t,
                kind: kind.to_string(),
                fields: Vec::new(),
            };
            for pair in parts {
                let (k, v) = parse_pair(pair, line_no)?;
                event.fields.push((k, v));
            }
            history.events.push(event);
        }
        if !seen_header {
            return Err(ParseError::new(
                0,
                format!("empty input: expected header `{FORMAT}`"),
            ));
        }
        Ok(history)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: String) -> Self {
        ParseError { line, message }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for ParseError {}

fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn parse_pair(pair: &str, line_no: usize) -> Result<(String, String), ParseError> {
    let (k, v) = pair
        .split_once('=')
        .ok_or_else(|| ParseError::new(line_no, format!("expected key=value, found `{pair}`")))?;
    if !is_token(k) {
        return Err(ParseError::new(line_no, format!("bad key `{k}`")));
    }
    let value = decode_value(v).map_err(|m| ParseError::new(line_no, m))?;
    Ok((k.to_string(), value))
}

pub fn encode_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b':' | b',' | b'/')
        {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn decode_value(v: &str) -> Result<String, String> {
    let bytes = v.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = v
                .get(i + 1..i + 3)
                .ok_or_else(|| format!("truncated escape in `{v}`"))?;
            let byte =
                u8::from_str_radix(hex, 16).map_err(|_| format!("bad escape `%{hex}` in `{v}`"))?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| format!("value `{v}` is not valid UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> History {
        let mut h = History::new();
        h.add_meta("seed", 42);
        h.add_meta("note", "a b=c\n100%");
        h.push(
            Event::new(500, "req")
                .with("id", "r1")
                .with("text", "tokyo drift, é ünï = 100%\nnext line"),
        );
        h.push(Event::new(503, "call").with("req", "r1").with("n", 1));
        h.push(Event::new(900, "empty").with("v", ""));
        h
    }

    #[test]
    fn round_trips_hostile_values() {
        let h = sample();
        let text = h.to_text();
        assert!(text.starts_with("shoke-history/v1\n"));
        let back = History::from_text(&text).expect("parse");
        assert_eq!(h, back);
        assert_eq!(text, back.to_text());
    }

    #[test]
    fn one_event_per_line() {
        let text = sample().to_text();
        assert_eq!(text.lines().count(), 1 + 2 + 3);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let text = "# leading comment\n\nshoke-history/v1\n# c\nmeta a=1\n\n5 x k=v\n";
        let h = History::from_text(text).unwrap();
        assert_eq!(h.meta("a"), Some("1"));
        assert_eq!(h.events.len(), 1);
        assert_eq!(h.events[0].get("k"), Some("v"));
    }

    #[test]
    fn rejects_bad_input_with_line_numbers() {
        assert!(History::from_text("").is_err());
        assert!(History::from_text("nope\n").is_err());
        let e = History::from_text("shoke-history/v1\nabc x\n").unwrap_err();
        assert_eq!(e.line, 2);
        let e = History::from_text("shoke-history/v1\n5 x k\n").unwrap_err();
        assert_eq!(e.line, 2);
        let e = History::from_text("shoke-history/v1\n5 x k=%ZZ\n").unwrap_err();
        assert_eq!(e.line, 2);
        let e = History::from_text("shoke-history/v1\n5 x k=%4\n").unwrap_err();
        assert_eq!(e.line, 2);
        let e = History::from_text("shoke-history/v1\nmeta a=1 b=2\n").unwrap_err();
        assert_eq!(e.line, 2);
    }

    #[test]
    fn sort_is_stable() {
        let mut h = History::new();
        h.push(Event::new(5, "b"));
        h.push(Event::new(1, "a"));
        h.push(Event::new(5, "c"));
        h.sort();
        let kinds: Vec<_> = h.events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["a", "b", "c"]);
    }

    #[test]
    fn repeated_meta_keys_are_kept() {
        let mut h = History::new();
        h.add_meta("fault", "a");
        h.add_meta("fault", "b");
        let back = History::from_text(&h.to_text()).unwrap();
        assert_eq!(back.meta.len(), 2);
        assert_eq!(back.meta("fault"), Some("a"));
    }
}
