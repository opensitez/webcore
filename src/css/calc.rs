//! Typed CSS calculations. Relative units remain unresolved until layout.
use super::*;
use crate::types::*;

// length, angle, time, frequency, resolution, percentage exponents.
type Dimension = [i8; 6];
const NUMBER: Dimension = [0; 6];
const LENGTH: Dimension = [1, 0, 0, 0, 0, 0];
const ANGLE: Dimension = [0, 1, 0, 0, 0, 0];
const MAX_CALC_DEPTH: usize = 128;

pub(crate) fn serialize_math_literal(value: f32, unit: &str) -> String {
    let keyword = if value.is_nan() { Some("NaN") }
        else if value == f32::INFINITY { Some("infinity") }
        else if value == f32::NEG_INFINITY { Some("-infinity") }
        else { None };
    match keyword {
        Some(keyword) if unit.is_empty() => keyword.into(),
        Some(keyword) => format!("calc({keyword} * 1{unit})"),
        None => format!("{value}{unit}"),
    }
}

/// Serialize the calculation tree without losing operation precedence or units.
/// The caller chooses specified-value or computed-value length serialization.
pub(crate) fn serialize_calculation(node: &CalcNode, length: &impl Fn(&CssLength) -> String) -> String {
    let child = |node: &CalcNode| serialize_calculation(node, length);
    match node {
        CalcNode::Value(value) => length(value),
        CalcNode::Scalar(value, unit) => serialize_math_literal(*value, match unit {
            CalcScalarUnit::Number => "",
            CalcScalarUnit::Radians => "rad",
            CalcScalarUnit::Seconds => "s",
            CalcScalarUnit::Hertz => "Hz",
            CalcScalarUnit::Dppx => "dppx",
            CalcScalarUnit::Percent => "%",
        }),
        CalcNode::Add(a, b) => format!("({} + {})", child(a), child(b)),
        CalcNode::Sub(a, b) => format!("({} - {})", child(a), child(b)),
        CalcNode::Product(a, b) => format!("({} * {})", child(a), child(b)),
        CalcNode::Quotient(a, b) => format!("({} / {})", child(a), child(b)),
        CalcNode::Mul(a, b) => format!("({} * {})", child(a), serialize_math_literal(*b, "")),
        CalcNode::Div(a, b) => format!("({} / {})", child(a), serialize_math_literal(*b, "")),
        CalcNode::Function(function, args) => {
            use CssMathFunction::*;
            let name = match function {
                Min => "min", Max => "max", Clamp => "clamp", Round(_) => "round",
                Mod => "mod", Rem => "rem", Abs => "abs", Sign => "sign",
                Sin => "sin", Cos => "cos", Tan => "tan", Asin => "asin",
                Acos => "acos", Atan => "atan", Atan2 => "atan2", Pow => "pow",
                Sqrt => "sqrt", Hypot => "hypot", Log => "log", Exp => "exp",
            };
            let mut serialized = Vec::with_capacity(args.len() + 1);
            if let Round(strategy) = function {
                serialized.push(match strategy {
                    CssRoundingStrategy::Nearest => "nearest",
                    CssRoundingStrategy::Up => "up",
                    CssRoundingStrategy::Down => "down",
                    CssRoundingStrategy::ToZero => "to-zero",
                }.into());
            }
            serialized.extend(args.iter().enumerate().map(|(i, arg)| {
                let unbounded = matches!(arg, CalcNode::Scalar(v, _) | CalcNode::Value(CssLength::Px(v))
                    if (i == 0 && *v == f32::NEG_INFINITY) || (i == 2 && *v == f32::INFINITY));
                if *function == Clamp && unbounded { "none".into() } else { child(arg) }
            }));
            format!("{name}({})", serialized.join(", "))
        }
    }
}

struct Calculation {
    node: CalcNode,
    dimension: Dimension,
}

impl Calculation {
    fn number(value: f32) -> Self {
        Self {
            node: CalcNode::Scalar(value, CalcScalarUnit::Number),
            dimension: NUMBER,
        }
    }
}

pub(crate) fn is_math_function(value: &str) -> bool {
    let Some((name, _)) = value.split_once('(') else {
        return false;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "calc"
            | "min"
            | "max"
            | "clamp"
            | "round"
            | "mod"
            | "rem"
            | "abs"
            | "sign"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "atan2"
            | "pow"
            | "sqrt"
            | "hypot"
            | "log"
            | "exp"
    )
}

pub(crate) fn parse_math_length(value: &str) -> CssLength {
    let Some(result) = MathParser::parse(value, true) else {
        return CssLength::Auto;
    };
    if result.dimension != LENGTH {
        return CssLength::Auto;
    }
    compact_length(result.node)
}

pub(crate) fn parse_calc_number(value: &str) -> Option<f32> {
    parse_numeric_dimension(value, NUMBER)
}

pub(crate) fn parse_css_number(value: &str) -> Option<f32> {
    let value = value.trim();
    value.parse::<f32>().ok().or_else(|| parse_calc_number(value))
        .filter(|n| n.is_finite())
}

pub(crate) fn parse_nonnegative_number(value: &str) -> Option<f32> {
    let number = parse_css_number(value)?;
    // Literal out-of-range values are invalid; calculations clamp at computed value time.
    if is_math_function(value.trim()) { Some(number.max(0.0)) }
    else { (number >= 0.0).then_some(number) }
}

pub(crate) fn parse_css_integer(value: &str) -> Option<i32> {
    let value = value.trim();
    if let Ok(integer) = value.parse::<i32>() { return Some(integer); }
    let number = parse_calc_number(value).filter(|n| n.is_finite())?;
    Some((f64::from(number) + 0.5).floor() as i32)
}

pub(crate) fn parse_positive_integer(value: &str) -> Option<i32> {
    let integer = parse_css_integer(value)?;
    if is_math_function(value.trim()) { Some(integer.max(1)) }
    else { (integer > 0).then_some(integer) }
}

fn parse_numeric_dimension(value: &str, dimension: Dimension) -> Option<f32> {
    if !is_math_function(value) {
        return None;
    }
    let result = MathParser::parse(value, false)?;
    if result.dimension != dimension || !context_independent(&result.node) {
        return None;
    }
    Some(result.node.resolve_vp(0.0, 0.0, 0.0, 0.0, 0.0))
}

pub(crate) fn parse_math_angle_deg(value: &str) -> Option<f32> {
    parse_numeric_dimension(value, ANGLE).map(f32::to_degrees)
}

pub(crate) fn parse_math_time_ms(value: &str) -> Option<f32> {
    parse_numeric_dimension(value, [0, 0, 1, 0, 0, 0]).map(|v| v * 1000.0)
}

pub(crate) fn parse_math_alpha(value: &str) -> Option<f32> {
    if !is_math_function(value) {
        return None;
    }
    let result = MathParser::parse(value, false)?;
    let percentage = result.dimension == [0, 0, 0, 0, 0, 1];
    if (result.dimension != NUMBER && !percentage) || !context_independent(&result.node) {
        return None;
    }
    let value = result.node.resolve_vp(0.0, 0.0, 0.0, 0.0, 0.0);
    Some(if percentage { value / 100.0 } else { value })
}

fn context_independent(node: &CalcNode) -> bool {
    match node {
        CalcNode::Scalar(_, _) => true,
        CalcNode::Value(CssLength::Px(_) | CssLength::Zero) => true,
        CalcNode::Value(_) => false,
        CalcNode::Add(a, b)
        | CalcNode::Sub(a, b)
        | CalcNode::Product(a, b)
        | CalcNode::Quotient(a, b) => context_independent(a) && context_independent(b),
        CalcNode::Mul(a, _) | CalcNode::Div(a, _) => context_independent(a),
        CalcNode::Function(_, args) => args.iter().all(context_independent),
    }
}

// Preserve the compact linear representation and percentage-only results used
// by existing consumers; nonlinear and context-dependent products keep their AST.
fn compact_length(node: CalcNode) -> CssLength {
    let node = match node {
        CalcNode::Function(CssMathFunction::Min, args) => {
            return CssLength::Min(Box::new(args.into_iter().map(compact_length).collect()));
        }
        CalcNode::Function(CssMathFunction::Max, args) => {
            return CssLength::Max(Box::new(args.into_iter().map(compact_length).collect()));
        }
        CalcNode::Function(CssMathFunction::Clamp, args) => {
            let mut args = args.into_iter().map(compact_length);
            return CssLength::Clamp(Box::new([
                args.next().unwrap(),
                args.next().unwrap(),
                args.next().unwrap(),
            ]));
        }
        other => other,
    };
    fn coefficients(node: &CalcNode) -> Option<[f32; 6]> {
        let mut out = [0.0; 6];
        match node {
            CalcNode::Value(value) => match value {
                CssLength::Percent(v) => out[0] = *v,
                CssLength::Px(v) => out[1] = *v,
                CssLength::Em(v) => out[2] = *v,
                CssLength::Rem(v) => out[3] = *v,
                CssLength::Vw(v) => out[4] = *v,
                CssLength::Vh(v) => out[5] = *v,
                CssLength::Zero => {}
                _ => return None,
            },
            CalcNode::Add(a, b) | CalcNode::Sub(a, b) => {
                let left = coefficients(a)?;
                let right = coefficients(b)?;
                let sign = if matches!(node, CalcNode::Sub(..)) {
                    -1.0
                } else {
                    1.0
                };
                for i in 0..6 {
                    out[i] = left[i] + sign * right[i];
                }
            }
            CalcNode::Mul(a, scalar) | CalcNode::Div(a, scalar) => {
                if !scalar.is_finite() || *scalar == 0.0 && matches!(node, CalcNode::Div(..)) {
                    return None;
                }
                out = coefficients(a)?;
                let factor = if matches!(node, CalcNode::Div(..)) {
                    1.0 / scalar
                } else {
                    *scalar
                };
                for v in &mut out {
                    *v *= factor;
                }
            }
            _ => return None,
        }
        out.iter().all(|v| v.is_finite()).then_some(out)
    }
    if let Some(c) = coefficients(&node) {
        if c.iter().filter(|v| **v != 0.0).count() <= 1 {
            if c[0] != 0.0 {
                return CssLength::Percent(c[0]);
            }
            if c[2] != 0.0 {
                return CssLength::Em(c[2]);
            }
            if c[3] != 0.0 {
                return CssLength::Rem(c[3]);
            }
            if c[4] != 0.0 {
                return CssLength::Vw(c[4]);
            }
            if c[5] != 0.0 {
                return CssLength::Vh(c[5]);
            }
            return CssLength::Px(c[1]);
        }
        return CssLength::Calc(Box::new(c));
    }
    CssLength::CalcExpr(Box::new(node))
}

struct MathParser<'a> {
    input: &'a str,
    pos: usize,
    percentage_hint: Option<Dimension>,
    depth: usize,
}

impl<'a> MathParser<'a> {
    fn parse(input: &'a str, length_percentage: bool) -> Option<Calculation> {
        Self::parse_hint(input, length_percentage.then_some(LENGTH))
    }

    fn parse_hint(input: &'a str, percentage_hint: Option<Dimension>) -> Option<Calculation> {
        let mut parser = Self {
            input,
            pos: 0,
            percentage_hint,
            depth: 0,
        };
        let result = parser.sum()?;
        parser.whitespace();
        (parser.pos == input.len()).then_some(result)
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }
    fn whitespace(&mut self) -> bool {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 12))
        {
            self.pos += 1;
        }
        self.pos != start
    }
    fn take(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Option<Calculation> {
        let mut left = self.product()?;
        loop {
            let spaced = self.whitespace();
            let op = self.peek();
            if !matches!(op, Some(b'+' | b'-')) {
                break;
            }
            if !spaced {
                return None;
            }
            self.pos += 1;
            if !self.whitespace() {
                return None;
            }
            let right = self.product()?;
            if left.dimension != right.dimension {
                return None;
            }
            left.node = if op == Some(b'+') {
                CalcNode::Add(Box::new(left.node), Box::new(right.node))
            } else {
                CalcNode::Sub(Box::new(left.node), Box::new(right.node))
            };
        }
        Some(left)
    }

    fn product(&mut self) -> Option<Calculation> {
        let mut left = self.atom()?;
        loop {
            let before_space = self.pos;
            self.whitespace();
            let op = self.peek();
            if !matches!(op, Some(b'*' | b'/')) {
                self.pos = before_space;
                break;
            }
            self.pos += 1;
            let right = self.atom()?;
            let left_dimension = left.dimension;
            for i in 0..6 {
                left.dimension[i] = if op == Some(b'*') {
                    left.dimension[i].checked_add(right.dimension[i])?
                } else {
                    left.dimension[i].checked_sub(right.dimension[i])?
                };
            }
            left.node = if right.dimension == NUMBER {
                if let CalcNode::Scalar(n, CalcScalarUnit::Number) = right.node {
                    if op == Some(b'*') {
                        CalcNode::Mul(Box::new(left.node), n)
                    } else if n.is_finite() && n != 0.0 {
                        CalcNode::Div(Box::new(left.node), n)
                    } else {
                        CalcNode::Quotient(
                            Box::new(left.node),
                            Box::new(CalcNode::Scalar(n, CalcScalarUnit::Number)),
                        )
                    }
                } else if op == Some(b'*') {
                    CalcNode::Product(Box::new(left.node), Box::new(right.node))
                } else {
                    CalcNode::Quotient(Box::new(left.node), Box::new(right.node))
                }
            } else if op == Some(b'*') && left_dimension == NUMBER {
                if let CalcNode::Scalar(n, CalcScalarUnit::Number) = left.node {
                    CalcNode::Mul(Box::new(right.node), n)
                } else {
                    CalcNode::Product(Box::new(left.node), Box::new(right.node))
                }
            } else if op == Some(b'*') {
                CalcNode::Product(Box::new(left.node), Box::new(right.node))
            } else {
                CalcNode::Quotient(Box::new(left.node), Box::new(right.node))
            };
        }
        Some(left)
    }

    fn atom(&mut self) -> Option<Calculation> {
        self.whitespace();
        // Bound recursion for hostile stylesheets, rather than exhausting the stack.
        if self.depth >= MAX_CALC_DEPTH {
            return None;
        }
        self.depth += 1;
        let result = self.atom_inner();
        self.depth -= 1;
        result
    }

    fn atom_inner(&mut self) -> Option<Calculation> {
        if self.take(b'(') {
            let result = self.sum()?;
            self.whitespace();
            return self.take(b')').then_some(result);
        }
        let start = self.pos;
        if self.peek().is_some_and(|b| b.is_ascii_alphabetic())
            || self.input[self.pos..]
                .to_ascii_lowercase()
                .starts_with("-infinity")
        {
            while self
                .peek()
                .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                self.pos += 1;
            }
            let name = self.input[start..self.pos].to_ascii_lowercase();
            if self.take(b'(') {
                if name == "env" {
                    let mut nesting = 1;
                    while let Some(b) = self.peek() {
                        self.pos += 1;
                        if b == b'(' {
                            nesting += 1;
                        }
                        if b == b')' {
                            nesting -= 1;
                        }
                        if nesting == 0 {
                            let value = parse_length(&self.input[start..self.pos]);
                            return (!matches!(value, CssLength::Auto)).then_some(Calculation {
                                node: CalcNode::Value(value),
                                dimension: LENGTH,
                            });
                        }
                    }
                    return None;
                }
                return self.function(&name);
            }
            return Some(Calculation::number(match name.as_str() {
                "pi" => std::f32::consts::PI,
                "e" => std::f32::consts::E,
                "infinity" => f32::INFINITY,
                "-infinity" => f32::NEG_INFINITY,
                "nan" => f32::NAN,
                _ => return None,
            }));
        }
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let digits = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        let mut has_digit = self.pos > digits;
        if self.take(b'.') {
            let fractional = self.pos;
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == fractional {
                return None;
            }
            has_digit = true;
        }
        if !has_digit {
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let exponent = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let digits = self.pos;
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == digits {
                self.pos = exponent;
            }
        }
        let number: f32 = self.input[start..self.pos].parse().ok()?;
        let unit_start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        self.take(b'%');
        let unit = self.input[unit_start..self.pos].to_ascii_lowercase();
        if unit.is_empty() {
            return Some(Calculation::number(number));
        }
        let (dimension, scalar) = match unit.as_str() {
            "rad" => (ANGLE, Some(number)),
            "deg" => (ANGLE, Some(number.to_radians())),
            "grad" => (ANGLE, Some(number * std::f32::consts::PI / 200.0)),
            "turn" => (ANGLE, Some(number * std::f32::consts::TAU)),
            "s" => ([0, 0, 1, 0, 0, 0], Some(number)),
            "ms" => ([0, 0, 1, 0, 0, 0], Some(number / 1000.0)),
            "hz" => ([0, 0, 0, 1, 0, 0], Some(number)),
            "khz" => ([0, 0, 0, 1, 0, 0], Some(number * 1000.0)),
            "dppx" | "x" => ([0, 0, 0, 0, 1, 0], Some(number)),
            "dpi" => ([0, 0, 0, 0, 1, 0], Some(number / 96.0)),
            "dpcm" => ([0, 0, 0, 0, 1, 0], Some(number * 2.54 / 96.0)),
            "%" if self.percentage_hint.is_none() => ([0, 0, 0, 0, 0, 1], Some(number)),
            _ => (LENGTH, None),
        };
        let node = if let Some(n) = scalar {
            let unit = match dimension {
                ANGLE => CalcScalarUnit::Radians,
                [0, 0, 1, 0, 0, 0] => CalcScalarUnit::Seconds,
                [0, 0, 0, 1, 0, 0] => CalcScalarUnit::Hertz,
                [0, 0, 0, 0, 1, 0] => CalcScalarUnit::Dppx,
                _ => CalcScalarUnit::Percent,
            };
            CalcNode::Scalar(n, unit)
        } else {
            let value = parse_length(&self.input[start..self.pos]);
            if matches!(value, CssLength::Auto) { return None; }
            CalcNode::Value(value)
        };
        Some(Calculation {
            node,
            dimension,
        })
    }

    fn function(&mut self, name: &str) -> Option<Calculation> {
        use CssMathFunction::*;
        let mut strategy = CssRoundingStrategy::Nearest;
        self.whitespace();
        if name == "round" {
            let start = self.pos;
            while self
                .peek()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'-')
            {
                self.pos += 1;
            }
            strategy = match &self.input[start..self.pos].to_ascii_lowercase()[..] {
                "nearest" => CssRoundingStrategy::Nearest,
                "up" => CssRoundingStrategy::Up,
                "down" => CssRoundingStrategy::Down,
                "to-zero" => CssRoundingStrategy::ToZero,
                _ => {
                    self.pos = start;
                    CssRoundingStrategy::Nearest
                }
            };
            if self.pos != start {
                self.whitespace();
                if !self.take(b',') {
                    return None;
                }
            }
        }
        let mut args: Vec<Calculation> = Vec::new();
        let mut unbounded = Vec::new();
        loop {
            self.whitespace();
            if name == "clamp"
                && self.input[self.pos..]
                    .get(..4)
                    .is_some_and(|s| s.eq_ignore_ascii_case("none"))
            {
                self.pos += 4;
                if !matches!(args.len(), 0 | 2) {
                    return None;
                }
                unbounded.push(args.len());
                args.push(Calculation::number(if args.is_empty() {
                    f32::NEG_INFINITY
                } else {
                    f32::INFINITY
                }));
            } else {
                args.push(self.sum()?);
            }
            self.whitespace();
            if self.take(b')') {
                break;
            }
            if !self.take(b',') {
                return None;
            }
        }
        if name == "calc" {
            return (args.len() == 1).then(|| args.remove(0));
        }
        let function = match name {
            "min" => Min,
            "max" => Max,
            "clamp" => Clamp,
            "round" => Round(strategy),
            "mod" => Mod,
            "rem" => Rem,
            "abs" => Abs,
            "sign" => Sign,
            "sin" => Sin,
            "cos" => Cos,
            "tan" => Tan,
            "asin" => Asin,
            "acos" => Acos,
            "atan" => Atan,
            "atan2" => Atan2,
            "pow" => Pow,
            "sqrt" => Sqrt,
            "hypot" => Hypot,
            "log" => Log,
            "exp" => Exp,
            _ => return None,
        };
        let count = args.len();
        let valid_count = match function {
            Min | Max | Hypot => count > 0,
            Clamp => count == 3,
            Round(_) | Log => (1..=2).contains(&count),
            Mod | Rem | Atan2 | Pow => count == 2,
            _ => count == 1,
        };
        if !valid_count {
            return None;
        }
        for index in unbounded {
            args[index].dimension = args[1].dimension;
            if args[1].dimension == LENGTH {
                let value = if index == 0 { f32::NEG_INFINITY } else { f32::INFINITY };
                args[index].node = CalcNode::Value(CssLength::Px(value));
            }
        }
        let input_dimension = args[0].dimension;
        let all_same = args.iter().all(|a| a.dimension == input_dimension);
        let dimension = match function {
            Min | Max | Clamp | Round(_) | Mod | Rem | Hypot => {
                if !all_same
                    || matches!(function, Round(_)) && count == 1 && input_dimension != NUMBER
                {
                    return None;
                }
                input_dimension
            }
            Abs => input_dimension,
            Sign => NUMBER,
            Sin | Cos | Tan => {
                if input_dimension != NUMBER && input_dimension != ANGLE {
                    return None;
                }
                NUMBER
            }
            Asin | Acos | Atan => {
                if input_dimension != NUMBER {
                    return None;
                }
                ANGLE
            }
            Atan2 => {
                if !all_same {
                    return None;
                }
                ANGLE
            }
            Pow | Sqrt | Log | Exp => {
                if !args.iter().all(|a| a.dimension == NUMBER) {
                    return None;
                }
                NUMBER
            }
        };
        Some(Calculation {
            dimension,
            node: CalcNode::Function(function, args.into_iter().map(|a| a.node).collect()),
        })
    }
}
