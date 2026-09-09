//! Native SVG tree model.
//!
//! This is intentionally small at first: it gives parser/style/paint code a
//! typed home without changing rendering behavior yet.

use super::animation::SvgAnimationElement;

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
    pub animation: Option<SvgAnimationElement>,
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
            animation: None,
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
