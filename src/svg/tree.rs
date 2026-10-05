//! Native SVG tree model.
//!
//! This is intentionally small at first: it gives parser/style/paint code a
//! typed home without changing rendering behavior yet.

use super::animation::SvgAnimationElement;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub struct SvgDocument {
    pub root: SvgNode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SvgNode {
    pub kind: SvgElementKind,
    pub attributes: Vec<SvgAttribute>,
    pub children: Vec<SvgNode>,
    /// Aggregate direct text, excluding descendant text.
    pub text: String,
    /// Direct text in source order, indexed into `text` and `children`.
    /// Direct edits to `text` (including append/replacement) require clearing
    /// or rebasing these byte ranges. Child insertion/removal/reordering requires
    /// clearing or rebasing child positions. Such mutations are not detected
    /// automatically; empty metadata uses text-before-children legacy ordering.
    pub text_runs: Vec<SvgTextRun>,
    pub animation: Option<SvgAnimationElement>,
}

/// A UTF-8 byte range in a node's aggregate text, preceding an element child.
/// `before_child == children.len()` denotes trailing text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvgTextRun {
    pub range: Range<usize>,
    pub before_child: usize,
}

/// Borrowed direct content; element indices remain indices into `children`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SvgContent<'a> {
    Text(&'a str),
    Element(usize, &'a SvgNode),
}

/// Allocation-free source-order traversal of a node's direct content.
pub struct SvgContentIter<'a> {
    node: &'a SvgNode,
    child: usize,
    run: usize,
    legacy_text: bool,
}

impl<'a> Iterator for SvgContentIter<'a> {
    type Item = SvgContent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.legacy_text {
            self.legacy_text = false;
            return Some(SvgContent::Text(&self.node.text));
        }
        loop {
            if let Some(run) = self.node.text_runs.get(self.run) {
                if run.before_child <= self.child || self.child == self.node.children.len() {
                    self.run += 1;
                    if let Some(text) = self.node.text.get(run.range.clone()) {
                        if !text.is_empty() {
                            return Some(SvgContent::Text(text));
                        }
                    }
                    continue;
                }
            }
            let child = self.node.children.get(self.child)?;
            let index = self.child;
            self.child += 1;
            return Some(SvgContent::Element(index, child));
        }
    }
}

impl std::iter::FusedIterator for SvgContentIter<'_> {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvgAttribute {
    pub namespace: Option<String>,
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SvgElementKind {
    Svg,
    Group,
    Defs,
    Symbol,
    Use,
    Path,
    Rect,
    Circle,
    Ellipse,
    Line,
    Polyline,
    Polygon,
    Text,
    Tspan,
    TextPath,
    Title,
    Desc,
    Metadata,
    Anchor,
    ForeignObject,
    Image,
    LinearGradient,
    RadialGradient,
    ClipPath,
    Mask,
    Filter,
    FeGaussianBlur,
    FeOffset,
    FeDropShadow,
    FeFlood,
    FeComposite,
    FeBlend,
    FeColorMatrix,
    FeComponentTransfer,
    FeFuncR,
    FeFuncG,
    FeFuncB,
    FeFuncA,
    FeMorphology,
    FeMerge,
    FeMergeNode,
    FeImage,
    FeTile,
    FeConvolveMatrix,
    FeDisplacementMap,
    Pattern,
    Marker,
    Stop,
    Switch,
    View,
    Cursor,
    Style,
    Script,
    Animate,
    AnimateColor,
    AnimateTransform,
    AnimateMotion,
    MPath,
    Set,
    Unknown(String),
}

impl SvgElementKind {
    pub fn from_name(name: &str) -> Self {
        match name {
            "svg" => Self::Svg,
            "g" => Self::Group,
            "defs" => Self::Defs,
            "symbol" => Self::Symbol,
            "use" => Self::Use,
            "path" => Self::Path,
            "rect" => Self::Rect,
            "circle" => Self::Circle,
            "ellipse" => Self::Ellipse,
            "line" => Self::Line,
            "polyline" => Self::Polyline,
            "polygon" => Self::Polygon,
            "text" => Self::Text,
            "tspan" => Self::Tspan,
            "textPath" | "textpath" => Self::TextPath,
            "title" => Self::Title,
            "desc" => Self::Desc,
            "metadata" => Self::Metadata,
            "a" => Self::Anchor,
            "foreignObject" | "foreignobject" => Self::ForeignObject,
            "image" => Self::Image,
            "linearGradient" | "lineargradient" => Self::LinearGradient,
            "radialGradient" | "radialgradient" => Self::RadialGradient,
            "clipPath" | "clippath" => Self::ClipPath,
            "mask" => Self::Mask,
            "filter" => Self::Filter,
            "feGaussianBlur" | "fegaussianblur" => Self::FeGaussianBlur,
            "feOffset" | "feoffset" => Self::FeOffset,
            "feDropShadow" | "fedropshadow" => Self::FeDropShadow,
            "feFlood" | "feflood" => Self::FeFlood,
            "feComposite" | "fecomposite" => Self::FeComposite,
            "feBlend" | "feblend" => Self::FeBlend,
            "feColorMatrix" | "fecolormatrix" => Self::FeColorMatrix,
            "feComponentTransfer" | "fecomponenttransfer" => Self::FeComponentTransfer,
            "feFuncR" | "fefuncr" => Self::FeFuncR,
            "feFuncG" | "fefuncg" => Self::FeFuncG,
            "feFuncB" | "fefuncb" => Self::FeFuncB,
            "feFuncA" | "fefunca" => Self::FeFuncA,
            "feMorphology" | "femorphology" => Self::FeMorphology,
            "feMerge" | "femerge" => Self::FeMerge,
            "feMergeNode" | "femergenode" => Self::FeMergeNode,
            "feImage" | "feimage" => Self::FeImage,
            "feTile" | "fetile" => Self::FeTile,
            "feConvolveMatrix" | "feconvolvematrix" => Self::FeConvolveMatrix,
            "feDisplacementMap" | "fedisplacementmap" => Self::FeDisplacementMap,
            "pattern" => Self::Pattern,
            "marker" => Self::Marker,
            "stop" => Self::Stop,
            "switch" => Self::Switch,
            "view" => Self::View,
            "cursor" => Self::Cursor,
            "style" => Self::Style,
            "script" => Self::Script,
            "animate" => Self::Animate,
            "animateColor" | "animatecolor" => Self::AnimateColor,
            "animateTransform" | "animatetransform" => Self::AnimateTransform,
            "animateMotion" | "animatemotion" => Self::AnimateMotion,
            "mpath" => Self::MPath,
            "set" => Self::Set,
            other => Self::Unknown(other.to_string()),
        }
    }
}

impl SvgNode {
    pub fn new(name: &str) -> Self {
        Self {
            kind: SvgElementKind::from_name(name),
            attributes: Vec::new(),
            children: Vec::new(),
            text: String::new(),
            text_runs: Vec::new(),
            animation: None,
        }
    }

    /// Append direct text at the current element-child position, merging
    /// adjacent runs. Preexisting legacy text retains its before-child ordering.
    pub fn append_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        if self.text_runs.is_empty() && start != 0 {
            self.text_runs.push(SvgTextRun {
                range: 0..start,
                before_child: 0,
            });
        }
        self.text.push_str(text);
        let before_child = self.children.len();
        if let Some(last) = self.text_runs.last_mut() {
            if last.before_child == before_child && last.range.end == start {
                last.range.end = self.text.len();
                return;
            }
        }
        self.text_runs.push(SvgTextRun {
            range: start..self.text.len(),
            before_child,
        });
    }

    /// Replace aggregate direct text and discard its previous interleaving.
    /// The replacement is yielded before all existing element children; children
    /// and their indices remain unchanged. Later `append_text` calls record text
    /// after the current children while retaining this replacement before them.
    pub fn replace_text(&mut self, text: &str) {
        self.text.clear();
        self.text.push_str(text);
        self.text_runs.clear();
    }

    /// Iterate direct text and elements in source order without changing child
    /// indices. Legacy text with empty run metadata is yielded before children.
    /// Invalid manually supplied text ranges are skipped rather than panicking.
    pub fn content(&self) -> SvgContentIter<'_> {
        SvgContentIter {
            node: self,
            child: 0,
            run: 0,
            legacy_text: self.text_runs.is_empty() && !self.text.is_empty(),
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attr| attr.namespace.is_none() && attr.name == name)
            .map(|attr| attr.value.as_str())
    }

    pub fn attr_ascii_case_insensitive(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attr| attr.namespace.is_none() && attr.name.eq_ignore_ascii_case(name))
            .map(|attr| attr.value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_text_merges_only_at_the_same_child_position() {
        let mut node = SvgNode::new("text");
        node.append_text("");
        assert!(node.text_runs.is_empty());
        node.append_text("a");
        node.append_text("\u{e9}");
        node.children.push(SvgNode::new("tspan"));
        node.append_text("z");
        assert_eq!(node.text, "a\u{e9}z");
        assert_eq!(
            node.text_runs,
            vec![
                SvgTextRun {
                    range: 0..3,
                    before_child: 0
                },
                SvgTextRun {
                    range: 3..4,
                    before_child: 1
                },
            ]
        );
        assert_eq!(
            node.content().collect::<Vec<_>>(),
            vec![
                SvgContent::Text("a\u{e9}"),
                SvgContent::Element(0, &node.children[0]),
                SvgContent::Text("z"),
            ]
        );
    }

    #[test]
    fn legacy_text_precedes_children_and_survives_appending() {
        let mut node = SvgNode::new("text");
        node.text = "legacy".into();
        node.children.push(SvgNode::new("tspan"));
        assert_eq!(
            node.content().collect::<Vec<_>>(),
            vec![
                SvgContent::Text("legacy"),
                SvgContent::Element(0, &node.children[0]),
            ]
        );
        node.append_text("tail");
        assert_eq!(
            node.content().collect::<Vec<_>>(),
            vec![
                SvgContent::Text("legacy"),
                SvgContent::Element(0, &node.children[0]),
                SvgContent::Text("tail"),
            ]
        );
        let clone = node.clone();
        assert_eq!(node, clone);
    }

    #[test]
    fn unicode_replacement_clears_old_ranges_and_new_append_keeps_order() {
        let mut node = SvgNode::new("text");
        node.append_text("before");
        node.children.push(SvgNode::new("tspan"));
        node.append_text("after");
        node.replace_text("\u{e9}\u{1f642}");
        assert!(node.text_runs.is_empty());
        assert_eq!(
            node.content().collect::<Vec<_>>(),
            vec![
                SvgContent::Text("\u{e9}\u{1f642}"),
                SvgContent::Element(0, &node.children[0]),
            ]
        );
        node.append_text("\u{3bb}");
        assert_eq!(node.text, "\u{e9}\u{1f642}\u{3bb}");
        assert_eq!(
            node.text_runs,
            vec![
                SvgTextRun {
                    range: 0..6,
                    before_child: 0
                },
                SvgTextRun {
                    range: 6..8,
                    before_child: 1
                },
            ]
        );
        assert_eq!(
            node.content().collect::<Vec<_>>(),
            vec![
                SvgContent::Text("\u{e9}\u{1f642}"),
                SvgContent::Element(0, &node.children[0]),
                SvgContent::Text("\u{3bb}"),
            ]
        );
        node.replace_text("");
        assert!(node.text_runs.is_empty());
        assert_eq!(
            node.content().collect::<Vec<_>>(),
            vec![SvgContent::Element(0, &node.children[0])]
        );
        node.append_text("new");
        assert_eq!(
            node.content().collect::<Vec<_>>(),
            vec![
                SvgContent::Element(0, &node.children[0]),
                SvgContent::Text("new"),
            ]
        );
    }

    #[test]
    fn empty_nodes_and_invalid_manual_ranges_are_bounded() {
        let mut node = SvgNode::new("text");
        let mut empty = node.content();
        assert!(empty.next().is_none());
        assert!(empty.next().is_none());
        node.text = "\u{e9}".into();
        node.text_runs.push(SvgTextRun {
            range: 1..2,
            before_child: 0,
        });
        node.children.push(SvgNode::new("tspan"));
        assert_eq!(
            node.content().collect::<Vec<_>>(),
            vec![SvgContent::Element(0, &node.children[0])]
        );
    }
}
