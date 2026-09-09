//! Native SVG document parser.
//!
//! This parser intentionally starts with SVG document/tree concerns only. CSS
//! cascade, layout, and paint are separate phases in this module.

use super::animation::parse_animation_element;
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
                node.animation = parse_animation_element(&node);
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
                node.animation = parse_animation_element(&node);
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
    use crate::svg::{
        SvgAdditiveMode, SvgAnimateTransformType, SvgAnimationFillMode, SvgAnimationKind,
        SvgAnimationTime, SvgCalcMode, SvgElementKind, SvgRepeatCount,
    };

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
    fn parses_switch_and_view_as_typed_elements() {
        let doc =
            parse_svg_document(r#"<svg><switch><view id="v"/><cursor id="c"/><rect width="1" height="1"/></switch></svg>"#)
                .unwrap();
        assert_eq!(doc.root.children[0].kind, SvgElementKind::Switch);
        assert_eq!(doc.root.children[0].children[0].kind, SvgElementKind::View);
        assert_eq!(
            doc.root.children[0].children[1].kind,
            SvgElementKind::Cursor
        );
    }

    #[test]
    fn parses_foreign_object_as_typed_element() {
        let doc = parse_svg_document(
            r#"<svg><foreignObject width="10" height="10"><div>HTML</div></foreignObject></svg>"#,
        )
        .unwrap();
        assert_eq!(doc.root.children[0].kind, SvgElementKind::ForeignObject);
        assert_eq!(
            doc.root.children[0].children[0].kind,
            SvgElementKind::Unknown("div".to_string())
        );
    }

    #[test]
    fn parses_metadata_and_anchor_as_typed_elements() {
        let doc = parse_svg_document(
            r##"<svg><title>Name</title><desc>Desc</desc><metadata>m</metadata><a href="#x"><rect/></a></svg>"##,
        )
        .unwrap();
        assert_eq!(doc.root.children[0].kind, SvgElementKind::Title);
        assert_eq!(doc.root.children[1].kind, SvgElementKind::Desc);
        assert_eq!(doc.root.children[2].kind, SvgElementKind::Metadata);
        assert_eq!(doc.root.children[3].kind, SvgElementKind::Anchor);
    }

    #[test]
    fn parses_filter_primitives_as_typed_elements() {
        let doc = parse_svg_document(
            r#"<svg><filter><feGaussianBlur/><feOffset/><feComponentTransfer><feFuncA/></feComponentTransfer><feMerge><feMergeNode/></feMerge></filter></svg>"#,
        )
        .unwrap();
        let filter = &doc.root.children[0];
        assert_eq!(filter.kind, SvgElementKind::Filter);
        assert_eq!(filter.children[0].kind, SvgElementKind::FeGaussianBlur);
        assert_eq!(filter.children[1].kind, SvgElementKind::FeOffset);
        assert_eq!(filter.children[2].kind, SvgElementKind::FeComponentTransfer);
        assert_eq!(filter.children[2].children[0].kind, SvgElementKind::FeFuncA);
        assert_eq!(filter.children[3].kind, SvgElementKind::FeMerge);
        assert_eq!(
            filter.children[3].children[0].kind,
            SvgElementKind::FeMergeNode
        );
    }

    #[test]
    fn parses_animation_elements_as_typed_metadata() {
        let doc = parse_svg_document(
            r##"<svg>
                <rect id="r" width="10" height="10">
                  <animate attributeName="fill" from="red" to="blue" dur="750ms" begin="click;2s" repeatCount="3" fill="freeze" calcMode="discrete"/>
                  <animateTransform attributeName="transform" type="rotate" values="0;90;180" keyTimes="0;0.5;1" additive="sum"/>
                </rect>
                <animateMotion xlink:href="#r" path="M0 0L10 0" rotate="auto"/>
                <set attributeName="visibility" to="hidden" begin="indefinite"/>
            </svg>"##,
        )
        .unwrap();

        let animate = doc.root.children[0].children[0]
            .animation
            .as_ref()
            .expect("animate metadata");
        assert_eq!(animate.kind, SvgAnimationKind::Animate);
        assert_eq!(animate.target_attribute.as_deref(), Some("fill"));
        assert_eq!(animate.values.from.as_deref(), Some("red"));
        assert_eq!(animate.values.to.as_deref(), Some("blue"));
        assert_eq!(animate.dur, Some(SvgAnimationTime::Seconds(0.75)));
        assert_eq!(
            animate.begin,
            vec![
                SvgAnimationTime::Event {
                    event: "click".to_string(),
                    offset: 0.0
                },
                SvgAnimationTime::Seconds(2.0)
            ]
        );
        assert_eq!(animate.repeat_count, SvgRepeatCount::Count(3.0));
        assert_eq!(animate.fill_mode, SvgAnimationFillMode::Freeze);
        assert_eq!(animate.calc_mode, SvgCalcMode::Discrete);

        let transform = doc.root.children[0].children[1]
            .animation
            .as_ref()
            .expect("animateTransform metadata");
        assert_eq!(transform.kind, SvgAnimationKind::AnimateTransform);
        assert_eq!(
            transform.transform_type,
            Some(SvgAnimateTransformType::Rotate)
        );
        assert_eq!(transform.values.values, vec!["0", "90", "180"]);
        assert_eq!(transform.key_times, vec![0.0, 0.5, 1.0]);
        assert_eq!(transform.additive, SvgAdditiveMode::Sum);

        let motion = doc.root.children[1]
            .animation
            .as_ref()
            .expect("animateMotion metadata");
        assert_eq!(motion.kind, SvgAnimationKind::AnimateMotion);
        assert_eq!(motion.href.as_deref(), Some("#r"));
        assert_eq!(motion.path.as_deref(), Some("M0 0L10 0"));

        let set = doc.root.children[2]
            .animation
            .as_ref()
            .expect("set metadata");
        assert_eq!(set.kind, SvgAnimationKind::Set);
        assert_eq!(set.begin, vec![SvgAnimationTime::Indefinite]);
    }

    #[test]
    fn skips_comments_and_preserves_cdata_text() {
        let doc = parse_svg_document(
            "<svg><!-- hidden --><style><![CDATA[path{fill:red}]]></style></svg>",
        )
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
