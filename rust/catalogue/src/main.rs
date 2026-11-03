//! Reads a query from stdin and prints a JSON array of matching track titles.
//! A query under 3 characters prints nothing (the neuron stays quiet). The
//! catalogue is made up and also lives in examples/typing-demo.mjs; change both
//! together. The rule is the JavaScript tracks neuron's: lowercase the query,
//! act from 3 characters, match it anywhere in "title artist". The query is not
//! trimmed, apart from the one newline `echo` or `printf` adds.
use std::io::{Read, Write};

const TRACKS: [(&str, &str); 4] = [
    ("Glass Orchard", "Lumen Harbor"),
    ("Ninefold Morning", "Lumen Harbor"),
    ("Quiet Kettles", "Tide and Timber"),
    ("Marrow Road", "Tide and Timber"),
];

/// None when the query is too short to act on, else the matching titles.
fn search(query: &str) -> Option<Vec<&'static str>> {
    let query = query.strip_suffix('\n').unwrap_or(query);
    let query = query.strip_suffix('\r').unwrap_or(query);
    let q = query.to_lowercase();
    // UTF-16 code units, to agree with JavaScript's `.length`.
    if q.encode_utf16().count() < 3 {
        return None;
    }
    Some(
        TRACKS
            .iter()
            .filter(|(title, artist)| format!("{title} {artist}").to_lowercase().contains(&q))
            .map(|(title, _)| *title)
            .collect(),
    )
}

fn escape(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn to_json(items: &[&str]) -> String {
    let parts: Vec<String> = items.iter().map(|s| escape(s)).collect();
    format!("[{}]", parts.join(","))
}

fn main() {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("cannot read stdin: {e}");
        std::process::exit(1);
    }
    if let Some(hits) = search(&input) {
        if writeln!(std::io::stdout().lock(), "{}", to_json(&hits)).is_err() {
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_case_insensitively() {
        assert_eq!(search("LUM"), Some(vec!["Glass Orchard", "Ninefold Morning"]));
        assert_eq!(search("quiet k\n"), Some(vec!["Quiet Kettles"]));
        assert_eq!(search("quiet k\r\n"), Some(vec!["Quiet Kettles"]));
    }

    #[test]
    fn needs_three_characters() {
        assert_eq!(search("lu"), None);
        assert_eq!(search("lu\n"), None);
        assert!(search("lum").is_some());
        // an emoji is two UTF-16 units, as in JavaScript
        assert!(search("\u{1F600}x").is_some());
    }

    #[test]
    fn spaces_are_kept() {
        assert_eq!(search(" lum "), Some(vec![]));
        assert_eq!(search("  l "), Some(vec![]));
        assert_eq!(search(" l"), None);
    }

    #[test]
    fn no_match_is_empty() {
        assert_eq!(search("zzzz"), Some(vec![]));
        assert_eq!(to_json(&[]), "[]");
    }

    #[test]
    fn escapes_quotes_backslashes_and_control_characters() {
        assert_eq!(to_json(&["a\"b\\c"]), r#"["a\"b\\c"]"#);
        assert_eq!(escape("x\ny"), "\"x\\ny\"");
        assert_eq!(escape("\u{1}"), "\"\\u0001\"");
    }
}
