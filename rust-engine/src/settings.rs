//! A neuron's own settings (decision 0022, point 6): `.env` in the directory
//! of the file that declares the neuron's program. For each name in the
//! neuron's `env` the file's value is used when the file has the name, and
//! the engine's environment's otherwise. Only listed names are ever passed,
//! and a value is never reported.

use std::path::{Path, PathBuf};

pub const FILE: &str = ".env";

/// A line that is not `NAME=value`: where, never what (the text may be a secret).
#[derive(Debug)]
pub struct Bad {
    pub file: PathBuf,
    pub line: usize,
}

impl std::fmt::Display for Bad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} line {}: not NAME=value", self.file.display(), self.line)
    }
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The file's pairs, in order. Blank lines and lines that start with `#` are
/// skipped; a value in single or double quotes loses them; there is no
/// interpolation, no `export` and no escape. No file is no settings.
pub fn read(dir: &Path) -> Result<Vec<(String, String)>, Bad> {
    let file = dir.join(FILE);
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Ok(Vec::new());
    };
    let mut pairs = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = || Bad {
            file: file.clone(),
            line: i + 1,
        };
        let (name, value) = line.split_once('=').ok_or_else(bad)?;
        let (name, mut value) = (name.trim_end(), value.trim_start());
        if !is_name(name) {
            return Err(bad());
        }
        for q in ['"', '\''] {
            if value.len() >= 2 && value.starts_with(q) && value.ends_with(q) {
                value = &value[1..value.len() - 1];
                break;
            }
        }
        pairs.push((name.to_string(), value.to_string()));
    }
    Ok(pairs)
}

/// The settings a neuron's program is started with: the file's value for each
/// of its `env` names the file has (the last one, if a name is repeated).
pub fn for_names(dir: &Path, names: &[String]) -> Result<Vec<(String, String)>, Bad> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let pairs = read(dir)?;
    Ok(names
        .iter()
        .filter_map(|n| pairs.iter().rev().find(|(k, _)| k == n).map(|(k, v)| (k.clone(), v.clone())))
        .collect())
}
