//! A small, lossless INI reader for Codename Engine configuration files.
//!
//! The format used by Codename Engine is INI-like: `[Section]` headers that may
//! carry a trailing `#` comment, `key=value` entries, `#` and `;` comments, and
//! optionally quoted values. Nothing is normalized away: section order, key
//! order, duplicates, comments attached to headers and entries, and quoting are
//! all retained so that unknown configuration survives an import unchanged.
//!
//! Parsing never fails outright. Syntax problems are collected as [`IniIssue`]s
//! with line numbers, which callers map onto import diagnostics.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Section name used for entries that appear before the first `[Section]`.
pub const GLOBAL_SECTION: &str = "";

/// A parsed configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IniDocument {
    /// Sections in file order, including duplicates and unknown sections.
    pub sections: Vec<IniSection>,
    /// Syntax problems found while parsing.
    pub issues: Vec<IniIssue>,
}

impl IniDocument {
    /// Parses `source`, collecting problems instead of failing.
    pub fn parse(source: &str) -> Self {
        let mut document = Self::default();
        let mut current: Option<usize> = None;
        let mut global_created = false;

        for (index, raw_line) in source.lines().enumerate() {
            let line_number = index + 1;
            let line = if index == 0 {
                raw_line.trim_start_matches('\u{feff}')
            } else {
                raw_line
            };
            let trimmed = line.trim();
            if trimmed.is_empty() || is_comment(trimmed) {
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix('[') {
                match rest.find(']') {
                    Some(end) => {
                        let name = rest[..end].trim();
                        let trailing = rest[end + 1..].trim();
                        let comment = if trailing.is_empty() {
                            None
                        } else if is_comment(trailing) {
                            Some(trailing[1..].trim().to_string())
                        } else {
                            document.issues.push(IniIssue {
                                line: line_number,
                                message: format!(
                                    "unexpected text after section header: {trailing}"
                                ),
                            });
                            None
                        };
                        if name.is_empty() {
                            document.issues.push(IniIssue {
                                line: line_number,
                                message: "empty section name".to_string(),
                            });
                        }
                        document.sections.push(IniSection {
                            name: name.to_string(),
                            comment,
                            line: line_number,
                            entries: Vec::new(),
                        });
                        current = Some(document.sections.len() - 1);
                    }
                    None => document.issues.push(IniIssue {
                        line: line_number,
                        message: "unterminated section header".to_string(),
                    }),
                }
                continue;
            }

            let Some(equals) = find_unquoted(trimmed, '=') else {
                document.issues.push(IniIssue {
                    line: line_number,
                    message: format!("expected key=value, found: {trimmed}"),
                });
                continue;
            };

            let key = trimmed[..equals].trim();
            if key.is_empty() {
                document.issues.push(IniIssue {
                    line: line_number,
                    message: "empty key".to_string(),
                });
                continue;
            }
            if key
                .chars()
                .any(|character| character == '"' || character == '\'')
            {
                document.issues.push(IniIssue {
                    line: line_number,
                    message: format!("quoted keys are not understood: {key}"),
                });
            }

            let (value, quoted, comment) =
                parse_value(&trimmed[equals + 1..], line_number, &mut document.issues);

            if current.is_none() {
                if !global_created {
                    document.sections.push(IniSection {
                        name: GLOBAL_SECTION.to_string(),
                        comment: Some("entries before the first section".to_string()),
                        line: line_number,
                        entries: Vec::new(),
                    });
                    global_created = true;
                }
                document.issues.push(IniIssue {
                    line: line_number,
                    message: format!("entry '{key}' appears before any section header"),
                });
            }

            let section_index = current.unwrap_or_else(|| document.sections.len() - 1);
            document.sections[section_index].entries.push(IniEntry {
                key: key.to_string(),
                value,
                quoted,
                comment,
                line: line_number,
            });
        }

        document
    }

    /// True when no section and no issue was parsed.
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty() && self.issues.is_empty()
    }

    /// The first section named `name`, if any.
    pub fn section(&self, name: &str) -> Option<&IniSection> {
        self.sections.iter().find(|section| section.name == name)
    }

    /// Every section named `name`, in file order.
    pub fn sections_named<'a, 'b>(
        &'a self,
        name: &'b str,
    ) -> impl Iterator<Item = &'a IniSection> + 'b
    where
        'a: 'b,
    {
        self.sections
            .iter()
            .filter(move |section| section.name == name)
    }

    /// Entries of every section named `name`, in file order.
    ///
    /// This mirrors how Codename Engine merges repeated sections across files.
    pub fn merged_entries<'a, 'b>(&'a self, name: &'b str) -> Vec<&'a IniEntry>
    where
        'a: 'b,
    {
        self.sections_named(name)
            .flat_map(|section| section.entries.iter())
            .collect()
    }

    /// The last value assigned to `key` in `section`, following the usual INI
    /// "later wins" convention.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .iter()
            .filter(|candidate| candidate.name == section)
            .flat_map(|candidate| candidate.entries.iter())
            .rfind(|entry| entry.key.eq_ignore_ascii_case(key))
            .map(|entry| entry.value.as_str())
    }

    /// Keys of `section` in file order, including unknown keys.
    pub fn keys<'a>(&'a self, section: &str) -> Vec<&'a str> {
        self.sections
            .iter()
            .filter(|candidate| candidate.name == section)
            .flat_map(|candidate| candidate.entries.iter())
            .map(|entry| entry.key.as_str())
            .collect()
    }

    /// Distinct section names in file order.
    pub fn section_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for section in &self.sections {
            if !names.contains(&section.name.as_str()) {
                names.push(section.name.as_str());
            }
        }
        names
    }
}

/// One `[Section]` block.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IniSection {
    /// Raw section name, or [`GLOBAL_SECTION`] for pre-section entries.
    pub name: String,
    /// Trailing `#`/`;` comment on the header line, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// 1-based source line of the header.
    pub line: usize,
    /// Entries in file order.
    pub entries: Vec<IniEntry>,
}

impl IniSection {
    /// The last value assigned to `key`, matched case-insensitively.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .rfind(|entry| entry.key.eq_ignore_ascii_case(key))
            .map(|entry| entry.value.as_str())
    }
}

/// One `key=value` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IniEntry {
    /// Key as written, without surrounding whitespace.
    pub key: String,
    /// Value with surrounding whitespace, quotes and inline comments removed.
    pub value: String,
    /// True when the value was quoted in the source.
    pub quoted: bool,
    /// Trailing inline comment, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// 1-based source line.
    pub line: usize,
}

/// A recoverable syntax problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IniIssue {
    /// 1-based source line.
    pub line: usize,
    /// Description that never includes file contents.
    pub message: String,
}

impl fmt::Display for IniIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

fn is_comment(trimmed: &str) -> bool {
    trimmed.starts_with('#') || trimmed.starts_with(';')
}

/// Finds `needle` outside of single or double quotes.
fn find_unquoted(source: &str, needle: char) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (index, character) in source.char_indices() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character == needle => return Some(index),
            None => {}
        }
    }
    None
}

/// Splits a raw value into its content, whether it was quoted, and any inline
/// comment. `#` and `;` only start a comment when they are separated from the
/// value by whitespace, so `#8E0533` stays a colour value.
fn parse_value(
    raw: &str,
    line: usize,
    issues: &mut Vec<IniIssue>,
) -> (String, bool, Option<String>) {
    let characters: Vec<char> = raw.chars().collect();
    let mut value = String::new();
    let mut quote: Option<char> = None;
    let mut quoted = false;
    let mut comment: Option<String> = None;
    let mut index = 0;

    while index < characters.len() {
        let character = characters[index];
        match quote {
            Some(open) => {
                if character == open {
                    quote = None;
                } else {
                    value.push(character);
                }
            }
            None => {
                if character == '"' || character == '\'' {
                    quote = Some(character);
                    quoted = true;
                } else if character == '#' || character == ';' {
                    let preceded_by_space =
                        value.is_empty() || value.ends_with(char::is_whitespace);
                    let followed_by_space = characters
                        .get(index + 1)
                        .is_none_or(|next| next.is_whitespace());
                    if preceded_by_space && followed_by_space {
                        comment = Some(
                            characters[index + 1..]
                                .iter()
                                .collect::<String>()
                                .trim()
                                .to_string(),
                        );
                        break;
                    }
                    value.push(character);
                } else {
                    value.push(character);
                }
            }
        }
        index += 1;
    }

    if let Some(open) = quote {
        issues.push(IniIssue {
            line,
            message: format!("unterminated quoted value (missing {open})"),
        });
    }

    (value.trim().to_string(), quoted, comment)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODPACK: &str = "\u{feff}[Common] # This section applies the 'MOD_' prefix.\r\n\
# a comment line\r\n\
API_VERSION=3\r\n\
DOWNLOAD_LINK=\"https://gamebanana.com/mods/728206\"\r\n\
;MOD_ICON64=\"path/to/icon64\"\r\n\
\r\n\
[Flags] # No prefix here.\r\n\
DISABLE_WARNING_SCREEN=true\r\n\
VERSION_MESSAGE=\"VS Var Overhaul Demo v1.1\" # inline note\r\n\
\r\n\
[StateRedirects]\r\n\
TitleState=\"newTitle\"\r\n\
FreeplayState=\"NewFreeplay\"\r\n";

    #[test]
    fn parses_sections_quotes_and_comments() {
        let document = IniDocument::parse(MODPACK);
        assert!(document.issues.is_empty(), "{:?}", document.issues);
        assert_eq!(
            document.section_names(),
            vec!["Common", "Flags", "StateRedirects"]
        );
        assert_eq!(document.get("Common", "API_VERSION"), Some("3"));
        assert_eq!(
            document.get("Common", "DOWNLOAD_LINK"),
            Some("https://gamebanana.com/mods/728206")
        );
        assert_eq!(
            document.get("Flags", "VERSION_MESSAGE"),
            Some("VS Var Overhaul Demo v1.1")
        );
        assert_eq!(
            document.get("StateRedirects", "FreeplayState"),
            Some("NewFreeplay")
        );
    }

    #[test]
    fn preserves_inline_section_comments_and_quoting_flags() {
        let document = IniDocument::parse(MODPACK);
        let common = document.section("Common").unwrap();
        assert_eq!(
            common.comment.as_deref(),
            Some("This section applies the 'MOD_' prefix.")
        );
        let link = common
            .entries
            .iter()
            .find(|entry| entry.key == "DOWNLOAD_LINK")
            .unwrap();
        assert!(link.quoted);
        assert_eq!(link.value, "https://gamebanana.com/mods/728206");

        let flags = document.section("Flags").unwrap();
        let version = flags
            .entries
            .iter()
            .find(|entry| entry.key == "VERSION_MESSAGE")
            .unwrap();
        assert_eq!(version.comment.as_deref(), Some("inline note"));
    }

    #[test]
    fn keeps_comment_markers_inside_unspaced_values() {
        let document = IniDocument::parse("[Flags]\nCOLOR=#8E0533\nA=one#two\nB=one ; note\n");
        assert_eq!(document.get("Flags", "COLOR"), Some("#8E0533"));
        assert_eq!(document.get("Flags", "A"), Some("one#two"));
        assert_eq!(document.get("Flags", "B"), Some("one"));
        let entry = document
            .section("Flags")
            .unwrap()
            .entries
            .iter()
            .find(|entry| entry.key == "B")
            .unwrap();
        assert_eq!(entry.comment.as_deref(), Some("note"));
    }

    #[test]
    fn keeps_unknown_sections_keys_and_duplicates() {
        let document = IniDocument::parse(
            "[Custom]\nfoo=1\nbar=2\nfoo=3\n[Discord]\nCLIENT_ID=\"123\"\n[Custom]\nbaz=4\n",
        );
        assert_eq!(document.sections.len(), 3);
        assert_eq!(document.get("Custom", "foo"), Some("3"), "later wins");
        assert_eq!(document.keys("Custom"), vec!["foo", "bar", "foo", "baz"]);
        assert_eq!(document.get("Discord", "CLIENT_ID"), Some("123"));
        assert_eq!(document.merged_entries("Custom").len(), 4);
    }

    #[test]
    fn reports_malformed_lines_without_dropping_the_document() {
        let document = IniDocument::parse(
            "orphan=1\n[Broken\n[Sec] trailing\nno_equals_here\n=novalue\nQUOTED=\"open\nGOOD=fine\n",
        );
        let messages: Vec<_> = document
            .issues
            .iter()
            .map(|issue| issue.message.as_str())
            .collect();
        assert!(messages
            .iter()
            .any(|message| message.contains("before any section")));
        assert!(messages
            .iter()
            .any(|message| message.contains("unterminated section")));
        assert!(messages
            .iter()
            .any(|message| message.contains("after section header")));
        assert!(messages
            .iter()
            .any(|message| message.contains("expected key=value")));
        assert!(messages.iter().any(|message| message.contains("empty key")));
        assert!(messages
            .iter()
            .any(|message| message.contains("unterminated quoted")));
        assert_eq!(document.get("Sec", "GOOD"), Some("fine"));
        assert_eq!(document.get(GLOBAL_SECTION, "orphan"), Some("1"));
    }

    #[test]
    fn empty_input_yields_empty_document() {
        let document = IniDocument::parse("");
        assert!(document.is_empty());
    }
}
