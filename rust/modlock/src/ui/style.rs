//! style holds what makes a Style short to write: lengths, edges and colors.
//! The Style itself, and each type it holds, is generated from the schema.

use crate::generated::{Edges, Gradient, GradientShape, GradientStop, Length, Style, Transform};

/// FIT sizes an element to its children.
pub const FIT: Length = Length::FitChildren(true);

/// FILL shares the space left in the parent's flow.
pub const FILL: Length = Length::Fill(1.0);

/// px returns a length of pixels.
pub fn px(pixels: impl Into<f64>) -> Length {
    Length::Pixels(pixels.into() as f32)
}

/// percent returns a length of a percentage of the parent.
pub fn percent(percent: impl Into<f64>) -> Length {
    Length::Percent(percent.into() as f32)
}

/// rgb returns the opaque color 0xRRGGBB as a style holds it.
pub const fn rgb(color: u32) -> u32 {
    color << 8 | 0xff
}

/// rgba returns the color 0xRRGGBBAA as a style holds it, which is the number
/// itself; it marks the number as a color where one is read.
pub const fn rgba(color: u32) -> u32 {
    color
}

/// gradient returns a gradient of colors, each as a style holds it, that run
/// along shape, spaced evenly from 0 to 1.
pub fn gradient(shape: GradientShape, colors: &[u32]) -> Gradient {
    let last = colors.len().saturating_sub(1).max(1);
    let stop = |(index, color): (usize, &u32)| GradientStop {
        at: index as f32 / last as f32,
        color: *color,
    };
    Gradient {
        shape,
        stops: colors.iter().enumerate().map(stop).collect(),
    }
}

/// Edges are distances in pixels from each side. One number is every side,
/// two are the vertical and the horizontal sides, and four are the top,
/// right, bottom and left.
impl From<f32> for Edges {
    fn from(all: f32) -> Edges {
        Edges {
            top: all,
            right: all,
            bottom: all,
            left: all,
        }
    }
}

impl From<i32> for Edges {
    fn from(all: i32) -> Edges {
        Edges::from(all as f32)
    }
}

impl From<[f32; 2]> for Edges {
    fn from([vertical, horizontal]: [f32; 2]) -> Edges {
        Edges {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }
}

impl From<[i32; 2]> for Edges {
    fn from([vertical, horizontal]: [i32; 2]) -> Edges {
        Edges::from([vertical as f32, horizontal as f32])
    }
}

impl From<[f32; 4]> for Edges {
    fn from([top, right, bottom, left]: [f32; 4]) -> Edges {
        Edges {
            top,
            right,
            bottom,
            left,
        }
    }
}

impl From<[i32; 4]> for Edges {
    fn from([top, right, bottom, left]: [i32; 4]) -> Edges {
        Edges::from([top as f32, right as f32, bottom as f32, left as f32])
    }
}

/// Set is a value a member of a style takes: the member's own type, and
/// whatever stands for it. ui! sets each member written between two pairs of
/// braces through it, so a size takes a number of pixels and a color text.
pub trait Set<Member> {
    /// set_as returns the value as the member holds it.
    fn set_as(self) -> Member;
}

impl<T> Set<T> for T {
    fn set_as(self) -> T {
        self
    }
}

/// A member that may be unset takes its value alone.
impl<T> Set<Option<T>> for T {
    fn set_as(self) -> Option<T> {
        Some(self)
    }
}

/// A whole number stands for a number with a fraction.
impl Set<f32> for i32 {
    fn set_as(self) -> f32 {
        self as f32
    }
}

impl Set<Option<f32>> for i32 {
    fn set_as(self) -> Option<f32> {
        Some(self as f32)
    }
}

/// A count stands for a whole number of any size.
impl Set<i64> for i32 {
    fn set_as(self) -> i64 {
        i64::from(self)
    }
}

impl Set<i64> for u32 {
    fn set_as(self) -> i64 {
        i64::from(self)
    }
}

impl Set<i64> for usize {
    fn set_as(self) -> i64 {
        self as i64
    }
}

/// A number is a length in pixels.
impl Set<Option<Length>> for i32 {
    fn set_as(self) -> Option<Length> {
        Some(px(self))
    }
}

impl Set<Option<Length>> for f32 {
    fn set_as(self) -> Option<Length> {
        Some(px(self))
    }
}

/// One number is every edge, two the vertical and the horizontal edges, and
/// four the top, right, bottom and left.
impl Set<Option<Edges>> for i32 {
    fn set_as(self) -> Option<Edges> {
        Some(self.into())
    }
}

impl Set<Option<Edges>> for f32 {
    fn set_as(self) -> Option<Edges> {
        Some(self.into())
    }
}

impl Set<Option<Edges>> for [i32; 2] {
    fn set_as(self) -> Option<Edges> {
        Some(self.into())
    }
}

impl Set<Option<Edges>> for [f32; 2] {
    fn set_as(self) -> Option<Edges> {
        Some(self.into())
    }
}

impl Set<Option<Edges>> for [i32; 4] {
    fn set_as(self) -> Option<Edges> {
        Some(self.into())
    }
}

impl Set<Option<Edges>> for [f32; 4] {
    fn set_as(self) -> Option<Edges> {
        Some(self.into())
    }
}

/// Text is a color as "#rrggbb" or "#rrggbbaa", and no color when it is
/// neither.
impl Set<u32> for &str {
    fn set_as(self) -> u32 {
        color(self)
    }
}

impl Set<String> for &str {
    fn set_as(self) -> String {
        self.to_owned()
    }
}

/// color returns "#rrggbb" or "#rrggbbaa" as 0xRRGGBBAA, or 0 for text that
/// is not a color. A color without alpha is opaque.
pub fn color(text: &str) -> u32 {
    let digits = text.strip_prefix('#').unwrap_or(text);
    match (digits.len(), u32::from_str_radix(digits, 16)) {
        _ if digits.starts_with('+') => 0,
        (6, Ok(rgb)) => rgb << 8 | 0xff,
        (8, Ok(rgba)) => rgba,
        _ => 0,
    }
}

/// merged writes Style's merge from the names of its members, every one of
/// them: a member the schema adds does not build until it is named here.
macro_rules! merged {
    ($($member:ident),* $(,)?) => {
        impl Style {
            /// merge returns the style with each member over sets in its
            /// place, as a later style overrides an earlier one. A member over
            /// leaves as its default stays, so a member that may be unset is
            /// replaced by any value over sets, zero too, and a transform is
            /// merged part by part, so a style that scales keeps what another
            /// moved.
            pub fn merge(mut self, over: Style) -> Style {
                let Style { transform, $($member),* } = over;
                $(
                    if $member != Default::default() {
                        self.$member = $member;
                    }
                )*
                self.transform = transformed(self.transform, transform);
                self
            }
        }
    };
}

/// transformed returns the transform under with each part over sets in its
/// place: its move, its scale, its turn and its origin. Over alone is taken
/// whole, so a transform that sets nothing still stands where under has none.
fn transformed(under: Option<Transform>, over: Option<Transform>) -> Option<Transform> {
    let (mut under, over) = match (under, over) {
        (Some(under), Some(over)) => (under, over),
        (under, over) => return over.or(under),
    };
    if over.translate_x != 0.0 || over.translate_y != 0.0 {
        (under.translate_x, under.translate_y) = (over.translate_x, over.translate_y);
    }
    if over.scale_x.is_some() || over.scale_y.is_some() {
        (under.scale_x, under.scale_y) = (over.scale_x, over.scale_y);
    }
    if over.rotate != 0.0 {
        under.rotate = over.rotate;
    }
    if over.origin_x.is_some() || over.origin_y.is_some() {
        (under.origin_x, under.origin_y) = (over.origin_x, over.origin_y);
    }
    Some(under)
}

merged! {
    width, height, flow, horizontal_align, vertical_align, margin, padding, background, color,
    font_size, bold, text_align, border_radius, opacity, face, uppercase, letter_spacing, shadow,
    border_width, border_color, x, y, min_width, max_width, min_height, max_height, z_index,
    overflow_x, overflow_y, gradient, box_shadow, text_shadow, tint, saturation, brightness,
    contrast, blur, italic, weight, line_height, no_wrap, text_overflow, decoration, transition,
    clip, turn,
}
