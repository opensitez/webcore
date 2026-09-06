//! Native SVG document parser.
//!
//! This parser intentionally starts with SVG document/tree concerns only. CSS
//! cascade, layout, and paint are separate phases in this module.

use super::tree::{SvgAttribute, SvgDocument, SvgNode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvgParseError {
    pub message: String,
    pub offset: usize,
}

pub fn parse_svg_document(input: &str) -> Result<SvgDocument, SvgParseError> {
    let mut parser = Parser { input, pos: 0 };
    parser.skip_misc()?;
    let root = parser.parse_element()?;
    parser.skip_misc()?;
    Ok(SvgDocument { root })
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn parse_element(&mut self) -> Result<SvgNode, SvgParseError> {
        self.expect("<")?;
        if self.starts_with("/") {
            return Err(self.error("unexpected closing tag"));
        }
        let name = self.parse_name()?;
        let mut node = SvgNode::new(&name);

        loop {
            self.skip_ws();
            if self.consume("/>") {
                return Ok(node);
            }
            if self.consume(">") {
                break;
            }
            node.attributes.push(self.parse_attribute()?);
        }

        loop {
            if self.starts_with("</") {
                self.pos += 2;
                let close = self.parse_name()?;
                self.skip_ws();
                self.expect(">")?;
                if close != name {
                    return Err(self.error(&format!(
                        "mismatched closing tag: expected </{}>, got </{}>",
                        name, close
                    )));
                }
                return Ok(node);
            }
            if self.starts_with("<!--") {
                self.skip_comment()?;
                continue;
            }
            if self.starts_with("<![CDATA[") {
                node.text.push_str(&self.parse_cdata()?);
                continue;
            }
            if self.starts_with("<") {
                node.children.push(self.parse_element()?);
                continue;
            }
            let text = self.parse_text();
            if !text.is_empty() {
                node.text.push_str(&decode_xml_entities(&text));
            }
        }
    }

    fn parse_attribute(&mut self) -> Result<SvgAttribute, SvgParseError> {
        let raw_name = self.parse_name()?;
        self.skip_ws();
        self.expect("=")?;
        self.skip_ws();
        let value = self.parse_quoted_or_unquoted_value()?;
        let (namespace, name) = match raw_name.split_once(':') {
            Some((ns, local)) => (Some(ns.to_string()), local.to_string()),
            None => (None, raw_name),
        };
        Ok(SvgAttribute {
            namespace,
            name,
            value: decode_xml_entities(&value),
        })
    }

    fn parse_quoted_or_unquoted_value(&mut self) -> Result<String, SvgParseError> {
        let Some(ch) = self.peek_char() else {
            return Err(self.error("expected attribute value"));
        };
        if ch == '"' || ch == '\'' {
            self.pos += ch.len_utf8();
            let start = self.pos;
            while let Some(next) = self.peek_char() {
                if next == ch {
                    let out = self.input[start..self.pos].to_string();
                    self.pos += ch.len_utf8();
                    return Ok(out);
                }
                self.pos += next.len_utf8();
            }
            return Err(self.error("unterminated quoted attribute value"));
        }

        let start = self.pos;
        while let Some(next) = self.peek_char() {
            if next.is_whitespace() || next == '>' || next == '/' {
                break;
            }
            self.pos += next.len_utf8();
        }
        if self.pos == start {
            Err(self.error("expected attribute value"))
        } else {
            Ok(self.input[start..self.pos].to_string())
        }
    }

    fn parse_name(&mut self) -> Result<String, SvgParseError> {
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-' | '.') {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        if self.pos == start {
            Err(self.error("expected name"))
        } else {
            Ok(self.input[start..self.pos].to_string())
        }
    }

    fn parse_text(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.input.len() && !self.starts_with("<") {
            let ch = self.peek_char().expect("pos checked");
            self.pos += ch.len_utf8();
        }
        self.input[start..self.pos].to_string()
    }

    fn parse_cdata(&mut self) -> Result<String, SvgParseError> {
        self.expect("<![CDATA[")?;
        let start = self.pos;
        let Some(end_rel) = self.input[self.pos..].find("]]>") else {
            return Err(self.error("unterminated CDATA section"));
        };
        let end = self.pos + end_rel;
        self.pos = end + 3;
        Ok(self.input[start..end].to_string())
    }

    fn skip_misc(&mut self) -> Result<(), SvgParseError> {
        loop {
            self.skip_ws();
            if self.starts_with("<?") {
                let Some(end_rel) = self.input[self.pos..].find("?>") else {
                    return Err(self.error("unterminated processing instruction"));
                };
                self.pos += end_rel + 2;
                continue;
            }
            if self.starts_with("<!--") {
                self.skip_comment()?;
                continue;
            }
            break;
        }
        Ok(())
    }

    fn skip_comment(&mut self) -> Result<(), SvgParseError> {
        self.expect("<!--")?;
        let Some(end_rel) = self.input[self.pos..].find("-->") else {
            return Err(self.error("unterminated comment"));
        };
        self.pos += end_rel + 3;
        Ok(())
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek_char() {
            if !ch.is_whitespace() {
                break;
            }
            self.pos += ch.len_utf8();
        }
    }

    fn consume(&mut self, expected: &str) -> bool {
        if self.starts_with(expected) {
            self.pos += expected.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &str) -> Result<(), SvgParseError> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected `{}`", expected)))
        }
    }

    fn starts_with(&self, s: &str) -> bool {
        self.input[self.pos..].starts_with(s)
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn error(&self, message: &str) -> SvgParseError {
        SvgParseError {
            message: message.to_string(),
            offset: self.pos,
        }
    }
}

fn decode_xml_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::SvgElementKind;

    #[test]
    fn parses_nested_svg_elements() {
        let doc = parse_svg_document(
            r#"<svg viewBox="0 0 10 10"><g id="a"><rect width="10" height="10"/></g></svg>"#,
        )
        .unwrap();
        assert_eq!(doc.root.kind, SvgElementKind::Svg);
        assert_eq!(doc.root.children[0].kind, SvgElementKind::Group);
        assert_eq!(doc.root.children[0].children[0].kind, SvgElementKind::Rect);
    }

    #[test]
    fn preserves_namespaced_attributes() {
        let doc = parse_svg_document(r##"<svg><use xlink:href="#icon" href="#fallback"/></svg>"##)
            .unwrap();
        let attrs = &doc.root.children[0].attributes;
        assert_eq!(attrs[0].namespace.as_deref(), Some("xlink"));
        assert_eq!(attrs[0].name, "href");
        assert_eq!(attrs[0].value, "#icon");
        assert_eq!(attrs[1].namespace, None);
        assert_eq!(attrs[1].name, "href");
    }

    #[test]
    fn skips_comments_and_preserves_cdata_text() {
        let doc = parse_svg_document("<svg><!-- hidden --><style><![CDATA[path{fill:red}]]></style></svg>")
            .unwrap();
        assert_eq!(doc.root.children.len(), 1);
        assert_eq!(doc.root.children[0].kind, SvgElementKind::Style);
        assert_eq!(doc.root.children[0].text, "path{fill:red}");
    }

    #[test]
    fn reports_mismatched_closing_tag() {
        let err = parse_svg_document("<svg><g></svg>").unwrap_err();
        assert!(err.message.contains("mismatched closing tag"));
    }
}
