//! Apply a typed declaration (see `wega_css_parser::PropertyDeclaration`).
use crate::*;
use wega_css_parser::{PropertyDeclaration, Side};
use wega_css_values::RectLengths;

pub(crate) fn apply_typed(style: &mut ComputedStyle, declaration: PropertyDeclaration) {
    use PropertyDeclaration as P;
    let side = |edges: &mut RectLengths, side: Side, value| match side {
        Side::Top => edges.top = value,
        Side::Right => edges.right = value,
        Side::Bottom => edges.bottom = value,
        Side::Left => edges.left = value,
    };
    match declaration {
        P::Display(v) => style.display = v,
        P::Position(v) => style.position = v,
        P::BoxSizing(v) => style.box_sizing = v,
        P::Visibility(v) => style.visible = v,
        P::PointerEvents(v) => style.pointer_events = v,
        P::Width(v) => style.width = v,
        P::Height(v) => style.height = v,
        P::MinWidth(v) => style.min_width = v,
        P::MaxWidth(v) => style.max_width = v,
        P::MinHeight(v) => style.min_height = v,
        P::MaxHeight(v) => style.max_height = v,
        P::Margin(s, v) => side(&mut style.margin, s, v),
        P::Padding(s, v) => side(&mut style.padding, s, v),
        P::Inset(Side::Top, v) => style.top = v,
        P::Inset(Side::Right, v) => style.right = v,
        P::Inset(Side::Bottom, v) => style.bottom = v,
        P::Inset(Side::Left, v) => style.left = v,
        P::ZIndex(v) => style.z_index = v,
        P::Float(v) => style.float = v,
        P::Clear(left, right) => style.clear = (left, right),
        P::OverflowX(v) => style.overflow_x = v,
        P::OverflowY(v) => style.overflow_y = v,
    }
}
