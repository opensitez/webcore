//! Native SVG tree model.
//!
//! This is intentionally small at first: it gives parser/style/paint code a
//! typed home without changing rendering behavior yet.

#[derive(Clone, Debug, PartialEq)]
pub struct SvgDocument {
    pub root: SvgNode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SvgNode {
    pub kind: SvgElementKind,
    pub attributes: Vec<SvgAttribute>,
    pub children: Vec<SvgNode>,
    pub text: String,
}

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
    Image,
    LinearGradient,
    RadialGradient,
    ClipPath,
    Mask,
    Pattern,
    Marker,
    Style,
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
            "image" => Self::Image,
            "linearGradient" | "lineargradient" => Self::LinearGradient,
            "radialGradient" | "radialgradient" => Self::RadialGradient,
            "clipPath" | "clippath" => Self::ClipPath,
            "mask" => Self::Mask,
            "pattern" => Self::Pattern,
            "marker" => Self::Marker,
            "style" => Self::Style,
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
