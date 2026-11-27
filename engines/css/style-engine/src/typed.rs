//! Apply a typed declaration (see `wega_css_parser::PropertyDeclaration`).
//!
//! Each arm matches the string arm of `apply_declaration` it replaces: specified values
//! that depend on the element (`em`, `rem`) are resolved here against its font size.
use crate::*;
use wega_css_parser::{LineHeight, PropertyDeclaration, Side};
use wega_css_values::{Length, RectLengths};

fn index(side: Side) -> usize {
    match side {
        Side::Top => 0,
        Side::Right => 1,
        Side::Bottom => 2,
        Side::Left => 3,
    }
}

pub(crate) fn apply_typed(style: &mut ComputedStyle, declaration: PropertyDeclaration) {
    use PropertyDeclaration as P;
    let edge = |edges: &mut RectLengths, side: Side, value| match side {
        Side::Top => edges.top = value,
        Side::Right => edges.right = value,
        Side::Bottom => edges.bottom = value,
        Side::Left => edges.left = value,
    };
    let (font, root) = (style.font_size, style.root_font_size);
    let px = |l: Length| l.resolve(0.0, font, root).max(0.0);
    // A gap's fixed part in px and its percentage part as a fraction, resolved at layout.
    let gap = |l: Length| {
        let fixed = l.resolve(0.0, font, root);
        (fixed, (l.resolve(100.0, font, root) - fixed) / 100.0)
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
        P::Margin(s, v) => edge(&mut style.margin, s, v),
        P::Padding(s, v) => edge(&mut style.padding, s, v),
        P::Inset(Side::Top, v) => style.top = v,
        P::Inset(Side::Right, v) => style.right = v,
        P::Inset(Side::Bottom, v) => style.bottom = v,
        P::Inset(Side::Left, v) => style.left = v,
        P::ZIndex(v) => style.z_index = v,
        P::Float(v) => style.float = v,
        P::Clear(left, right) => style.clear = (left, right),
        P::OverflowX(v) => style.overflow_x = v,
        P::OverflowY(v) => style.overflow_y = v,
        P::Color(v) => style.color = v,
        P::BackgroundColor(v) => style.background_color = v,
        P::BackgroundPosition(v) => style.background_position = v,
        P::BackgroundSize(v) => style.background_size = v,
        P::BackgroundRepeat(v) => style.background_repeat = v,
        P::BackgroundClipText(v) => style.background_clip_text = v,
        P::ClipPath(v) => style.clip_path = v.map(|v| *v),
        P::Opacity(v) => style.opacity = v,
        P::BorderWidth(s, v) => border_side(style, index(s)).width = px(v),
        P::BorderColor(s, v) => border_side(style, index(s)).color = v,
        P::BorderStyleHidden(s, v) => style.border_hidden[index(s)] = v,
        P::BorderSpacing(v) => style.border_spacing = px(v),
        P::FontFamily(v) => style.font_family = v,
        P::FontWeight(v) => style.font_weight = v,
        P::FontItalic(v) => style.font_italic = v,
        P::LineHeight(LineHeight::Normal) => style.line_height_factor = Some(1.25),
        P::LineHeight(LineHeight::Factor(f)) => style.line_height_factor = Some(f),
        P::LineHeight(LineHeight::Length(l)) => {
            let h = l.resolve(font, font, root);
            if h >= 0.0 && h.is_finite() {
                style.line_height = h;
                style.line_height_factor = None;
            }
        }
        P::WhiteSpace(v) => style.white_space = v,
        P::TextAlign(v) => style.text_align = v,
        P::VerticalAlign(v) => style.vertical_align = v,
        P::Cursor(v) => style.cursor = v,
        P::FlexDirection(v) => style.flex_direction = v,
        P::JustifyContent(v) => style.justify_content = v,
        P::AlignItems(v) => style.align_items = v,
        P::FlexGrow(v) => style.flex_grow = v,
        P::FlexShrink(v) => style.flex_shrink = v,
        P::FlexBasis(v) => style.flex_basis = v,
        P::FlexWrap(v) => {
            style.flex_wrap = v.wrap;
            style.flex_wrap_reverse = v.reverse;
            style.flex_balance = v.balance;
        }
        P::AlignContent(v) => style.align_content = v,
        P::WritingMode(v) => style.writing_mode = v,
        P::DirectionRtl(v) => style.direction_rtl = v,
        P::Gap(row, column) => {
            (style.row_gap, style.row_gap_percent) = gap(row);
            (style.column_gap, style.column_gap_percent) = gap(column);
        }
        P::RowGap(v) => (style.row_gap, style.row_gap_percent) = gap(v),
        P::ColumnGap(v) => (style.column_gap, style.column_gap_percent) = gap(v),
        P::GridTemplateColumns(v) => style.grid_template_columns = v,
        P::GridTemplateRows(v) => style.grid_template_rows = v,
        P::GridTemplateAreas(v) => style.grid_template_areas = v,
        P::Transform(v) => style.transform = v.map(|v| *v),
        P::TransformOrigin(v) => style.transform_origin = v,
        P::ObjectFit(v) => style.object_fit = v,
        P::ObjectPosition(v) => style.object_position = v,
        P::BoxShadow(v) => style.box_shadow = v.map(|v| *v),
    }
}
