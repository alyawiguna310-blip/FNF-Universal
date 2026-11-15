//! A small, tolerant XML reader for Codename Engine character and stage files.
//!
//! Only what the importer needs is modelled: elements, ordered attributes, text
//! and nesting. The parser never resolves external entities or DTDs, never
//! reads a file referenced by a document, and never expands user-defined
//! entities, so a hostile XML file cannot cause file or network access
//! (XXE). Problems are collected as [`XmlIssue`]s rather than aborting.
//!
//! Unknown elements and attributes are preserved verbatim; interpreting them is
//! the caller's decision.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Deepest element nesting accepted before further nesting is reported and
/// flattened. This bounds both memory use and drop-time recursion.
pub const MAX_XML_DEPTH: usize = 256;

/// A parsed XML document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlDocument {
    /// Root element, when one was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<XmlElement>,
    /// Raw `<!DOCTYPE ...>` declaration text, kept for reference only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doctype: Option<String>,
    /// Recoverable syntax problems.
    pub issues: Vec<XmlIssue>,
}

impl XmlDocument {
    /// Parses `source`. Always returns a document.
    pub fn parse(source: &str) -> Self {
        Parser::new(source).run()
    }

    /// The document element name, when present.
    pub fn root_name(&self) -> Option<&str> {
        self.root.as_ref().map(|root| root.name.as_str())
    }
}

/// An XML element.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlElement {
    /// Element name exactly as written.
    pub name: String,
    /// Attributes in source order, including duplicates and unknown names.
    pub attributes: Vec<XmlAttribute>,
    /// Child elements in source order.
    pub children: Vec<XmlElement>,
    /// Decoded text content directly inside this element.
    pub text: String,
    /// 1-based source line of the opening tag.
    pub line: usize,
}

impl XmlElement {
    /// Attribute value, matched exactly.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| attribute.value.as_str())
    }

    /// Attribute value, matched case-insensitively.
    ///
    /// Codename Engine attributes are camel-cased, but mods are inconsistent
    /// enough that validation prefers a forgiving lookup.
    pub fn attr_ci(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name.eq_ignore_ascii_case(name))
            .map(|attribute| attribute.value.as_str())
    }

    /// Parses an attribute as `f64`, treating empty or invalid values as absent.
    pub fn attr_f64(&self, name: &str) -> Option<f64> {
        self.attr_ci(name)?.trim().parse().ok()
    }

    /// Parses an attribute as `bool` (`true`/`false`, `1`/`0`).
    pub fn attr_bool(&self, name: &str) -> Option<bool> {
        match self.attr_ci(name)?.trim() {
            value if value.eq_ignore_ascii_case("true") || value == "1" => Some(true),
            value if value.eq_ignore_ascii_case("false") || value == "0" => Some(false),
            _ => None,
        }
    }

    /// First direct child named `name`.
    pub fn child(&self, name: &str) -> Option<&XmlElement> {
        self.children.iter().find(|child| child.name == name)
    }

    /// Direct children named `name`, in source order.
    pub fn children_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a XmlElement> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// Trimmed text content.
    pub fn trimmed_text(&self) -> &str {
        self.text.trim()
    }
}

/// One attribute, preserving order and duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlAttribute {
    /// Attribute name as written.
    pub name: String,
    /// Decoded attribute value.
    pub value: String,
}

/// A recoverable XML syntax problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlIssue {
    /// 1-based source line.
    pub line: usize,
    /// Description that never includes file contents.
    pub message: String,
}

impl fmt::Display for XmlIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

struct Parser {
    characters: Vec<char>,
    position: usize,
    line: usize,
    issues: Vec<XmlIssue>,
    stack: Vec<XmlElement>,
    root: Option<XmlElement>,
    doctype: Option<String>,
    depth_reported: bool,
}

impl Parser {
    fn new(source: &str) -> Self {
        Self {
            characters: source.chars().collect(),
            position: 0,
            line: 1,
            issues: Vec::new(),
            stack: Vec::new(),
            root: None,
            doctype: None,
            depth_reported: false,
        }
    }

    fn run(mut self) -> XmlDocument {
        while self.position < self.characters.len() {
            if self.peek() == Some('<') {
                self.consume_markup();
            } else {
                let text = self.take_until('<');
                self.push_text(&text);
            }
        }

        while let Some(element) = self.stack.pop() {
            let line = element.line;
            self.issue(line, format!("unclosed element <{}>", element.name));
            self.attach(element);
        }

        XmlDocument {
            root: self.root,
            doctype: self.doctype,
            issues: self.issues,
        }
    }

    fn consume_markup(&mut self) {
        if self.starts_with("<!--") {
            self.skip_past("-->");
        } else if self.starts_with("<![CDATA[") {
            self.advance(9);
            let raw = self.take_until_str("]]>");
            if let Some(current) = self.stack.last_mut() {
                current.text.push_str(&raw);
            }
        } else if self.starts_with("<!DOCTYPE") || self.starts_with("<!doctype") {
            self.read_doctype();
        } else if self.starts_with("<?") {
            self.skip_past("?>");
        } else if self.starts_with("</") {
            self.close_element();
        } else {
            self.open_element();
        }
    }

    fn read_doctype(&mut self) {
        let start = self.position;
        let mut bracket_depth = 0usize;
        while self.position < self.characters.len() {
            let character = self.characters[self.position];
            match character {
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                '>' if bracket_depth == 0 => {
                    self.advance(1);
                    let text: String = self.characters[start..self.position].iter().collect();
                    self.doctype = Some(text.trim().to_string());
                    return;
                }
                _ => {}
            }
            self.advance(1);
        }
        self.issue(
            self.line,
            "unterminated <!DOCTYPE ...> declaration".to_string(),
        );
    }

    fn open_element(&mut self) {
        let line = self.line;
        self.advance(1);
        self.skip_whitespace();
        let name = self.take_name();
        if name.is_empty() {
            self.issue(line, "element name is missing".to_string());
            self.skip_past(">");
            return;
        }

        let mut attributes = Vec::new();
        let self_closing = loop {
            self.skip_whitespace();
            match self.peek() {
                None => {
                    self.issue(line, format!("unexpected end of input in <{name}>"));
                    break true;
                }
                Some('>') => {
                    self.advance(1);
                    break false;
                }
                Some('/') => {
                    if self.peek_at(1) == Some('>') {
                        self.advance(2);
                        break true;
                    }
                    self.issue(line, format!("stray '/' in <{name}>"));
                    self.advance(1);
                }
                Some('<') => {
                    // A new tag began before this one closed; treat as unclosed.
                    self.issue(line, format!("unclosed <{name}> before next tag"));
                    break true;
                }
                Some(_) => {
                    let attribute_name = self.take_attribute_name();
                    if attribute_name.is_empty() {
                        self.issue(line, format!("malformed attribute in <{name}>"));
                        self.advance(1);
                        continue;
                    }
                    self.skip_whitespace();
                    let value = if self.peek() == Some('=') {
                        self.advance(1);
                        self.skip_whitespace();
                        self.take_attribute_value(&name, &attribute_name)
                    } else {
                        self.issue(
                            line,
                            format!("attribute '{attribute_name}' in <{name}> has no value"),
                        );
                        String::new()
                    };
                    if attributes
                        .iter()
                        .any(|existing: &XmlAttribute| existing.name == attribute_name)
                    {
                        self.issue(
                            line,
                            format!("duplicate attribute '{attribute_name}' in <{name}>"),
                        );
                    }
                    attributes.push(XmlAttribute {
                        name: attribute_name,
                        value,
                    });
                }
            }
        };

        let element = XmlElement {
            name,
            attributes,
            children: Vec::new(),
            text: String::new(),
            line,
        };

        if self_closing {
            self.attach(element);
        } else if self.stack.len() >= MAX_XML_DEPTH {
            if !self.depth_reported {
                self.depth_reported = true;
                self.issue(
                    line,
                    format!("XML nesting deeper than {MAX_XML_DEPTH} elements was flattened"),
                );
            }
            self.attach(element);
        } else {
            self.stack.push(element);
        }
    }

    fn take_attribute_value(&mut self, element: &str, attribute: &str) -> String {
        match self.peek() {
            Some(quote @ ('"' | '\'')) => {
                self.advance(1);
                let mut raw = String::new();
                loop {
                    match self.peek() {
                        None => {
                            self.issue(
                                self.line,
                                format!(
                                    "unterminated value for attribute '{attribute}' in <{element}>"
                                ),
                            );
                            break;
                        }
                        Some(character) if character == quote => {
                            self.advance(1);
                            break;
                        }
                        Some(character) => {
                            raw.push(character);
                            self.advance(1);
                        }
                    }
                }
                self.decode_entities(&raw)
            }
            Some(_) => {
                self.issue(
                    self.line,
                    format!("attribute '{attribute}' in <{element}> is not quoted"),
                );
                let raw = self.take_until_any(&[' ', '\t', '\r', '\n', '>', '<', '/']);
                self.decode_entities(&raw)
            }
            None => {
                self.issue(
                    self.line,
                    format!("attribute '{attribute}' in <{element}> has no value"),
                );
                String::new()
            }
        }
    }

    fn close_element(&mut self) {
        let line = self.line;
        self.advance(2);
        self.skip_whitespace();
        let name = self.take_name();
        self.skip_whitespace();
        if self.peek() == Some('>') {
            self.advance(1);
        } else {
            self.issue(line, format!("malformed closing tag for </{name}>"));
            self.skip_past(">");
        }

        if name.is_empty() {
            return;
        }

        match self.stack.iter().rposition(|element| element.name == name) {
            Some(index) => {
                while self.stack.len() > index + 1 {
                    if let Some(orphan) = self.stack.pop() {
                        self.issue(
                            orphan.line,
                            format!("element <{}> was not closed before </{name}>", orphan.name),
                        );
                        self.attach(orphan);
                    }
                }
                if let Some(element) = self.stack.pop() {
                    self.attach(element);
                }
            }
            None => self.issue(line, format!("unexpected closing tag </{name}>")),
        }
    }

    fn attach(&mut self, element: XmlElement) {
        if let Some(parent) = self.stack.last_mut() {
            parent.children.push(element);
        } else if self.root.is_none() {
            self.root = Some(element);
        } else {
            self.issue(
                element.line,
                format!("multiple root elements; <{}> was ignored", element.name),
            );
        }
    }

    fn push_text(&mut self, raw: &str) {
        if raw.trim().is_empty() {
            return;
        }
        let decoded = self.decode_entities(raw);
        match self.stack.last_mut() {
            Some(element) => element.text.push_str(&decoded),
            None => {
                let snippet = decoded.trim();
                let snippet: String = snippet.chars().take(24).collect();
                self.issue(
                    self.line,
                    format!("text outside the root element was ignored: {snippet}"),
                );
            }
        }
    }

    /// Decodes the five predefined entities and numeric character references.
    ///
    /// Unknown entities are reported and left literal; nothing is ever fetched
    /// or expanded from a DTD.
    fn decode_entities(&mut self, raw: &str) -> String {
        if !raw.contains('&') {
            return self.strip_nul(raw);
        }

        let characters: Vec<char> = raw.chars().collect();
        let mut output = String::with_capacity(raw.len());
        let mut index = 0;
        while index < characters.len() {
            let character = characters[index];
            if character != '&' {
                output.push(character);
                index += 1;
                continue;
            }

            let limit = (index + 2 + 10).min(characters.len());
            let mut end = None;
            for (cursor, character) in characters.iter().enumerate().take(limit).skip(index + 1) {
                if *character == ';' {
                    end = Some(cursor);
                    break;
                }
                if !character.is_ascii_alphanumeric() && *character != '#' {
                    break;
                }
            }

            let Some(end) = end else {
                output.push('&');
                index += 1;
                continue;
            };

            let name: String = characters[index + 1..end].iter().collect();
            match decode_entity(&name) {
                Some(decoded) => {
                    output.push(decoded);
                    index = end + 1;
                }
                None => {
                    self.issue(self.line, format!("unknown entity '&{name};'"));
                    output.push('&');
                    index += 1;
                }
            }
        }
        self.strip_nul(&output)
    }

    fn strip_nul(&mut self, raw: &str) -> String {
        if !raw.contains('\0') {
            return raw.to_string();
        }
        self.issue(self.line, "NUL byte in text was removed".to_string());
        raw.replace('\0', "")
    }

    fn issue(&mut self, line: usize, message: String) {
        self.issues.push(XmlIssue { line, message });
    }

    fn peek(&self) -> Option<char> {
        self.characters.get(self.position).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.characters.get(self.position + offset).copied()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        prefix.chars().enumerate().all(|(offset, expected)| {
            self.characters.get(self.position + offset).copied() == Some(expected)
        })
    }

    fn advance(&mut self, count: usize) {
        for _ in 0..count {
            if let Some(character) = self.characters.get(self.position) {
                if *character == '\n' {
                    self.line += 1;
                }
                self.position += 1;
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.peek() {
            if character.is_whitespace() {
                self.advance(1);
            } else {
                break;
            }
        }
    }

    fn take_until(&mut self, terminator: char) -> String {
        let mut output = String::new();
        while let Some(character) = self.peek() {
            if character == terminator {
                break;
            }
            output.push(character);
            self.advance(1);
        }
        output
    }

    fn take_until_any(&mut self, terminators: &[char]) -> String {
        let mut output = String::new();
        while let Some(character) = self.peek() {
            if terminators.contains(&character) {
                break;
            }
            output.push(character);
            self.advance(1);
        }
        output
    }

    fn take_until_str(&mut self, terminator: &str) -> String {
        let mut output = String::new();
        while self.position < self.characters.len() {
            if self.starts_with(terminator) {
                self.advance(terminator.chars().count());
                break;
            }
            if let Some(character) = self.peek() {
                output.push(character);
                self.advance(1);
            }
        }
        output
    }

    fn skip_past(&mut self, terminator: &str) {
        let _ = self.take_until_str(terminator);
    }

    fn take_name(&mut self) -> String {
        self.take_until_any(&[' ', '\t', '\r', '\n', '>', '<', '/', '='])
    }

    fn take_attribute_name(&mut self) -> String {
        self.take_until_any(&[' ', '\t', '\r', '\n', '>', '<', '/', '='])
    }
}

fn decode_entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let numeric = name.strip_prefix('#')?;
            let code = match numeric.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => numeric.parse::<u32>().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codename_character_document() {
        let document = XmlDocument::parse(
            "<!DOCTYPE codename-engine-character>\n\
<character x=\"-30\" y=\"-90\" sprite=\"Var\" icon=\"var\" color=\"#5B5B5B\">\n\
\t<anim name=\"idle\" anim=\"Var_Idle\" x=\"-10\" y=\"-65\"/>\n\
\t<anim name=\"singUP\" anim=\"Var_Up\" x=\"15\" y=\"-56\"/>\n\
</character>\n",
        );
        assert!(document.issues.is_empty(), "{:?}", document.issues);
        assert_eq!(
            document.doctype.as_deref(),
            Some("<!DOCTYPE codename-engine-character>")
        );
        let root = document.root.as_ref().unwrap();
        assert_eq!(root.name, "character");
        assert_eq!(root.attr("sprite"), Some("Var"));
        assert_eq!(root.attr_f64("x"), Some(-30.0));
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].attr("anim"), Some("Var_Idle"));
        assert_eq!(root.child("anim").unwrap().attr_f64("y"), Some(-65.0));
    }

    #[test]
    fn preserves_unknown_attributes_and_element_order() {
        let document = XmlDocument::parse(
            r#"<stage name="castle" mystery="42" applyStageMatrix="true"><sprite name="a"/><girlfriend x="1"/><sprite name="b"/></stage>"#,
        );
        let root = document.root.unwrap();
        assert_eq!(root.attr("mystery"), Some("42"));
        assert_eq!(root.attr_bool("applyStageMatrix"), Some(true));
        let names: Vec<_> = root
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        assert_eq!(names, vec!["sprite", "girlfriend", "sprite"]);
    }

    #[test]
    fn handles_comments_cdata_entities_and_quotes() {
        let document = XmlDocument::parse(
            r#"<?xml version="1.0"?><!-- leading comment --><root a='single' b="double &amp; amp"><!-- inner --><text><![CDATA[raw <tag> & stuff]]></text><esc>&lt;&#65;&#x42;</esc></root>"#,
        );
        assert!(document.issues.is_empty(), "{:?}", document.issues);
        let root = document.root.unwrap();
        assert_eq!(root.attr("a"), Some("single"));
        assert_eq!(root.attr("b"), Some("double & amp"));
        assert_eq!(root.child("text").unwrap().text, "raw <tag> & stuff");
        assert_eq!(
            root.child("esc").unwrap().text,
            "<AB",
            "&lt; decodes to '<' and the numeric references decode to 'A' and 'B'"
        );
    }

    #[test]
    fn recovers_from_mismatched_and_unclosed_tags() {
        let document = XmlDocument::parse("<root><a><b></a></root>");
        let root = document.root.as_ref().unwrap();
        assert_eq!(root.name, "root");
        assert!(document
            .issues
            .iter()
            .any(|issue| issue.message.contains("was not closed before")));
        assert!(root.child("a").unwrap().child("b").is_some());
    }

    #[test]
    fn reports_unquoted_attributes_and_unknown_entities() {
        let document = XmlDocument::parse(r#"<root a=1 b="&weird;">x</root>"#);
        assert!(document
            .issues
            .iter()
            .any(|issue| issue.message.contains("not quoted")));
        assert!(document
            .issues
            .iter()
            .any(|issue| issue.message.contains("unknown entity")));
        let root = document.root.unwrap();
        assert_eq!(root.attr("a"), Some("1"));
        assert_eq!(root.attr("b"), Some("&weird;"));
    }

    #[test]
    fn reports_duplicate_attributes_and_multiple_roots() {
        let document = XmlDocument::parse(r#"<root a="1" a="2"></root><second/>"#);
        let root = document.root.as_ref().unwrap();
        assert_eq!(root.attr("a"), Some("1"), "first value kept");
        assert!(document
            .issues
            .iter()
            .any(|issue| issue.message.contains("duplicate attribute")));
        assert!(document
            .issues
            .iter()
            .any(|issue| issue.message.contains("multiple root elements")));
    }

    #[test]
    fn caps_nesting_depth() {
        let source = format!(
            "{}{}",
            "<a>".repeat(MAX_XML_DEPTH + 8),
            "</a>".repeat(MAX_XML_DEPTH + 8)
        );
        let document = XmlDocument::parse(&source);
        assert!(document
            .issues
            .iter()
            .any(|issue| issue.message.contains("flattened")));
    }

    #[test]
    fn does_not_expand_external_entities() {
        let document = XmlDocument::parse(
            r#"<!DOCTYPE root [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><root>&xxe;</root>"#,
        );
        let root = document.root.unwrap();
        assert_eq!(root.text, "&xxe;");
        assert!(document
            .issues
            .iter()
            .any(|issue| issue.message.contains("unknown entity")));
        assert_eq!(
            document.doctype.as_deref(),
            Some(r#"<!DOCTYPE root [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>"#),
            "the declaration is stored as inert text"
        );
    }
}
