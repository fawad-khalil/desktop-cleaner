//! Name/path search shared by the cleaning features: plain text or a
//! regular expression, optionally case-sensitive, matched against the full
//! path (which includes the name). Searching only narrows lists; it never
//! deletes anything, and the safety rules still apply to whatever matches.

use regex::{Regex, RegexBuilder};
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Filter {
    pub pattern: String,
    /// Treat `pattern` as a regular expression rather than plain text.
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub case_sensitive: bool,
}

pub struct Matcher(Regex);

impl Matcher {
    /// `Ok(None)` for an empty pattern (matches everything).
    pub fn new(f: &Filter) -> Result<Option<Matcher>, String> {
        if f.pattern.is_empty() {
            return Ok(None);
        }
        let src = if f.regex { f.pattern.clone() } else { regex::escape(&f.pattern) };
        RegexBuilder::new(&src)
            .case_insensitive(!f.case_sensitive)
            // Keep pathological patterns from using unbounded memory.
            .size_limit(1 << 20)
            .build()
            .map(|re| Some(Matcher(re)))
            .map_err(|e| match e {
                regex::Error::Syntax(s) => s.lines().last().unwrap_or(&s).trim().to_string(),
                other => other.to_string(),
            })
    }

    pub fn is_match(&self, path: &str) -> bool {
        self.0.is_match(path)
    }
}

/// Builds a matcher from an optional filter.
pub fn matcher(f: Option<&Filter>) -> Result<Option<Matcher>, String> {
    match f {
        Some(f) => Matcher::new(f),
        None => Ok(None),
    }
}

/// Why a pattern is invalid, or `None` if it's fine (for inline errors).
pub fn check(f: &Filter) -> Option<String> {
    Matcher::new(f).err()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(p: &str, regex: bool, case_sensitive: bool) -> Filter {
        Filter { pattern: p.into(), regex, case_sensitive }
    }

    #[test]
    fn plain_text_is_literal_and_case_insensitive_by_default() {
        let m = Matcher::new(&f("a.b", false, false)).unwrap().unwrap();
        assert!(m.is_match("/x/A.B.txt"));
        assert!(!m.is_match("/x/axb"));
    }

    #[test]
    fn regex_and_case_options() {
        let m = Matcher::new(&f(r"\.(log|tmp)$", true, false)).unwrap().unwrap();
        assert!(m.is_match("/var/app/Debug.LOG"));
        assert!(!m.is_match("/var/app/log.txt"));
        let cs = Matcher::new(&f("Cache", false, true)).unwrap().unwrap();
        assert!(!cs.is_match("/cache"));
        assert!(Matcher::new(&f("", true, false)).unwrap().is_none());
    }

    #[test]
    fn invalid_regex_reports_why() {
        let e = check(&f("(unclosed", true, false)).unwrap();
        assert!(e.contains("unclosed"), "{e}");
        assert!(check(&f("(unclosed", false, false)).is_none()); // literal text is fine
    }
}
