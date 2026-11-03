//! The made-up catalogue and the matching rule, shared by the `catalogue` and
//! `music` programs. The data also lives in examples/typing-demo.mjs; change
//! both together. The rule is the JavaScript tracks neuron's: lowercase the
//! query, act from 3 characters, match it anywhere in "title artist". The query
//! is not trimmed, apart from the one newline `echo` or `printf` adds.

pub const TRACKS: [(&str, &str); 4] = [
    ("Glass Orchard", "Lumen Harbor"),
    ("Ninefold Morning", "Lumen Harbor"),
    ("Quiet Kettles", "Tide and Timber"),
    ("Marrow Road", "Tide and Timber"),
];

pub const ARTISTS: [&str; 2] = ["Lumen Harbor", "Tide and Timber"];

pub const LYRICS: [&str; 2] = ["carry me home to the lumen harbor", "quiet kettles on the tide"];

/// The input rule: strip one trailing newline (and a carriage return before it), nothing else.
pub fn clean(input: &str) -> &str {
    let input = input.strip_suffix('\n').unwrap_or(input);
    input.strip_suffix('\r').unwrap_or(input)
}

/// None when the query is too short to act on, else the matching titles.
/// The query must already be cleaned.
pub fn matches(query: &str) -> Option<Vec<&'static str>> {
    let q = query.to_lowercase();
    if utf16_len(&q) < 3 {
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

/// `clean`, then `matches`.
pub fn search(input: &str) -> Option<Vec<&'static str>> {
    matches(clean(input))
}

/// Length in UTF-16 code units, to agree with JavaScript's `.length`.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub fn escape(s: &str) -> String {
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

pub fn to_json(items: &[&str]) -> String {
    let parts: Vec<String> = items.iter().map(|s| escape(s)).collect();
    format!("[{}]", parts.join(","))
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
