//! Reads a query from stdin, prints a JSON array of matching track titles.
//! The catalogue is made up. The rule matches the JavaScript tracks neuron in
//! examples/typing-demo.mjs: lowercase the query, act from 3 characters, and
//! match it anywhere in "title artist".
use std::io::Read;

const TRACKS: [(&str, &str); 4] = [
    ("Glass Orchard", "Lumen Harbor"),
    ("Ninefold Morning", "Lumen Harbor"),
    ("Quiet Kettles", "Tide and Timber"),
    ("Marrow Road", "Tide and Timber"),
];

fn search(query: &str) -> Vec<&'static str> {
    let q = query.trim().to_lowercase();
    if q.chars().count() < 3 {
        return Vec::new();
    }
    TRACKS
        .iter()
        .filter(|(title, artist)| format!("{title} {artist}").to_lowercase().contains(&q))
        .map(|(title, _)| *title)
        .collect()
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
    println!("{}", to_json(&search(&input)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_case_insensitively() {
        assert_eq!(search("LUM"), vec!["Glass Orchard", "Ninefold Morning"]);
        assert_eq!(search("  quiet k\n"), vec!["Quiet Kettles"]);
    }

    #[test]
    fn needs_three_characters() {
        assert!(search("lu").is_empty());
        assert!(search("  l ").is_empty());
        assert_eq!(search("lum").len(), 2);
    }

    #[test]
    fn no_match_is_empty() {
        assert!(search("zzzz").is_empty());
        assert_eq!(to_json(&search("zzzz")), "[]");
    }

    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(to_json(&["a\"b\\c"]), r#"["a\"b\\c"]"#);
        assert_eq!(escape("x\ny"), "\"x\\ny\"");
    }
}
