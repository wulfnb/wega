//! Native typography. Parley provides font selection, Unicode shaping, bidi and
//! line breaking; neither Blitz styles nor Blitz layout are consulted here.
use super::css::{Color, ComputedStyle, TextAlign, WhiteSpace};
use super::layout::{LayoutBox, LineBox, Rect};
use parley::{FontContext, LayoutContext, PositionedLayoutItem, TextStyle};
use std::cell::{Cell, RefCell};
use wega_dom::NodeKey;

#[derive(Clone, Debug, Default, PartialEq)]
struct TextBrush {
    node: Option<NodeKey>,
    color: Color,
    hit: bool,
}

#[derive(Clone, Debug)]
pub struct GlyphRun {
    pub node: Option<NodeKey>,
    pub rect: Rect,
    pub font: parley::FontData,
    pub font_size: f32,
    pub coords: Vec<i16>,
    pub glyphs: Vec<anyrender::Glyph>,
    pub color: Color,
    pub hit: bool,
    pub skew: Option<kurbo::Affine>,
    pub embolden: kurbo::Vec2,
}

type FontSource = Option<std::sync::Arc<Vec<Vec<u8>>>>;
thread_local! {
    static TEXT: RefCell<(FontContext, LayoutContext<TextBrush>)> = RefCell::new((
        FontContext {collection:parley::fontique::Collection::new(parley::fontique::CollectionOptions{shared:false,system_fonts:false}),source_cache:Default::default()},LayoutContext::new()));
    static SHAPING_NANOS: Cell<u128> = const {Cell::new(0)};
    static FONT_SOURCE: RefCell<Option<FontSource>> = const {RefCell::new(None)};
}
/// Brokered renderers use only the host-selected font bytes. Development views
/// may discover system fonts. No downloaded page fonts enter this shared cache.
pub fn configure_fonts(source: FontSource) {
    FONT_SOURCE.with(|active| {
        let same = active
            .borrow()
            .as_ref()
            .is_some_and(|current| match (current, &source) {
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            });
        if same {
            return;
        }
        let fonts = if let Some(bytes) = &source {
            use parley::fontique::{
                Blob, Collection, CollectionOptions, GenericFamily, SourceCache,
            };
            let mut fonts = FontContext {
                collection: Collection::new(CollectionOptions {
                    shared: false,
                    system_fonts: false,
                }),
                source_cache: SourceCache::new_shared(),
            };
            for (index, font) in bytes.iter().enumerate() {
                let registered = fonts
                    .collection
                    .register_fonts(Blob::new(std::sync::Arc::new(font.clone()) as _), None);
                let ids: Vec<_> = registered.iter().map(|(id, _)| *id).collect();
                let generic = match index {
                    0 => GenericFamily::SansSerif,
                    1 => GenericFamily::Serif,
                    _ => GenericFamily::Monospace,
                };
                fonts
                    .collection
                    .append_generic_families(generic, ids.iter().copied());
                if index == 0 {
                    fonts
                        .collection
                        .append_generic_families(GenericFamily::SystemUi, ids.iter().copied());
                }
            }
            fonts
        } else {
            FontContext::new()
        };
        TEXT.with(|cx| *cx.borrow_mut() = (fonts, LayoutContext::new()));
        *active.borrow_mut() = Some(source);
    });
}

fn text_style(
    style: &ComputedStyle,
    node: Option<NodeKey>,
    alpha: f32,
) -> TextStyle<'_, '_, TextBrush> {
    let (collapse, wrap) = match style.white_space {
        WhiteSpace::Normal => (
            parley::WhiteSpaceCollapse::Collapse,
            parley::TextWrapMode::Wrap,
        ),
        WhiteSpace::NoWrap => (
            parley::WhiteSpaceCollapse::Collapse,
            parley::TextWrapMode::NoWrap,
        ),
        WhiteSpace::Pre => (
            parley::WhiteSpaceCollapse::Preserve,
            parley::TextWrapMode::NoWrap,
        ),
        WhiteSpace::PreWrap => (
            parley::WhiteSpaceCollapse::Preserve,
            parley::TextWrapMode::Wrap,
        ),
        WhiteSpace::PreLine => (
            parley::WhiteSpaceCollapse::PreserveBreaks,
            parley::TextWrapMode::Wrap,
        ),
        WhiteSpace::BreakSpaces => (
            parley::WhiteSpaceCollapse::BreakSpaces,
            parley::TextWrapMode::Wrap,
        ),
    };
    let mut color = style.color;
    color.a = if style.visible {
        (color.a as f32 * alpha).round() as u8
    } else {
        0
    };
    TextStyle {
        font_family: parley::FontFamily::from(style.font_family.as_str()),
        font_size: style.font_size as f32,
        font_weight: parley::FontWeight::new(style.font_weight),
        font_style: if style.font_italic {
            parley::FontStyle::Italic
        } else {
            parley::FontStyle::Normal
        },
        line_height: parley::LineHeight::Absolute(style.line_height as f32),
        white_space_collapse: collapse,
        text_wrap_mode: wrap,
        brush: TextBrush {
            node,
            color,
            hit: style.visible && style.pointer_events,
        },
        ..Default::default()
    }
}

fn append_inline(
    builder: &mut parley::TreeBuilder<'_, TextBrush>,
    b: &LayoutBox,
    owner: Option<NodeKey>,
    alpha: f32,
) {
    if b.box_type == super::layout::BoxType::InlineBlock
        || b.form_state.is_some()
        || b.image.is_some()
    {
        let id = b.node.map(|k| k.as_u64()).unwrap_or(0);
        let vertical_align = match b.style.vertical_align {
            super::css::VerticalAlign::Baseline => parley::VerticalAlign::default(),
            super::css::VerticalAlign::Middle => parley::VerticalAlign::MIDDLE,
            super::css::VerticalAlign::Top => parley::VerticalAlign::new(
                parley::AlignmentBaseline::Baseline,
                parley::BaselineShift::Top,
            ),
            super::css::VerticalAlign::Bottom => parley::VerticalAlign::new(
                parley::AlignmentBaseline::Baseline,
                parley::BaselineShift::Bottom,
            ),
            super::css::VerticalAlign::TextTop => parley::VerticalAlign::new(
                parley::AlignmentBaseline::TextTop,
                parley::BaselineShift::Length(0.0),
            ),
            super::css::VerticalAlign::TextBottom => parley::VerticalAlign::new(
                parley::AlignmentBaseline::TextBottom,
                parley::BaselineShift::Length(0.0),
            ),
            super::css::VerticalAlign::Sub => parley::VerticalAlign::new(
                parley::AlignmentBaseline::Baseline,
                parley::BaselineShift::Sub,
            ),
            super::css::VerticalAlign::Super => parley::VerticalAlign::new(
                parley::AlignmentBaseline::Baseline,
                parley::BaselineShift::Super,
            ),
        };
        builder.push_inline_box(parley::InlineBox {
            id,
            index: 0,
            width: (b.border_box.width + b.margin.left + b.margin.right) as f32,
            height: (b.border_box.height + b.margin.top + b.margin.bottom) as f32,
            kind: parley::InlineBoxKind::InFlow,
            baseline: None,
            vertical_align,
        });
        return;
    }
    let owner = if b.box_type == super::layout::BoxType::Text {
        owner.or(b.node)
    } else {
        b.node.or(owner)
    };
    let alpha = alpha
        * if b.box_type == super::layout::BoxType::Text {
            1.0
        } else {
            b.style.opacity
        };
    let mut style = text_style(&b.style, owner, alpha);
    // A <br> is a forced break even under normal whitespace collapsing.
    if b.text_content.as_deref() == Some("\n") && b.box_type != super::layout::BoxType::Text {
        style.white_space_collapse = parley::WhiteSpaceCollapse::Preserve;
    }
    builder.push_style_span(style);
    if let Some(text) = &b.text_content {
        builder.push_text(text);
    }
    for child in &b.children {
        append_inline(builder, child, owner, alpha);
    }
    builder.pop_style_span();
}

pub fn shaping_nanos() -> u128 {
    SHAPING_NANOS.with(Cell::get)
}
pub fn shape_inline(children: &[LayoutBox], width: f64, align: TextAlign) -> Vec<LineBox> {
    let start = std::time::Instant::now();
    if FONT_SOURCE.with(|s| s.borrow().is_none()) {
        configure_fonts(None);
    }
    let result = TEXT.with(|cx| {
        let (fonts, layouts) = &mut *cx.borrow_mut();
        let root = TextStyle::<TextBrush>::default();
        let mut builder = layouts.tree_builder(fonts, 1.0, true, &root);
        for child in children {
            append_inline(&mut builder, child, None, 1.0);
        }
        let (mut shaped, text) = builder.build();
        if text.is_empty() && !children.iter().any(|c| c.box_type == super::layout::BoxType::InlineBlock || c.form_state.is_some() || c.image.is_some()) {
            return Vec::new();
        }
        shaped.break_all_lines(Some(width.max(0.0) as f32));
        shaped.align(
            match align {
                TextAlign::Left => parley::Alignment::Left,
                TextAlign::Right => parley::Alignment::Right,
                TextAlign::Center => parley::Alignment::Center,
                TextAlign::Justify => parley::Alignment::Justify,
            },
            parley::AlignmentOptions::default(),
        );
        shaped
            .lines()
            .map(|line| {
                let m = line.metrics();
                let mut runs = Vec::new();
                let mut inline_boxes = Vec::new();
                for item in line.items() {
                    match item {
                        PositionedLayoutItem::GlyphRun(glyph_run) => {
                            let run = glyph_run.run();
                            let brush = &glyph_run.style().brush;
                            let synthesis = run.synthesis();
                            runs.push(GlyphRun {
                                node: brush.node,
                                rect: Rect {
                                    x: glyph_run.offset() as f64,
                                    y: m.block_min_coord as f64,
                                    width: glyph_run.advance() as f64,
                                    height: m.line_height as f64,
                                },
                                font: run.font().clone(),
                                font_size: run.font_size(),
                                coords: run
                                    .normalized_coords()
                                    .iter()
                                    .map(|c| c.to_bits())
                                    .collect(),
                                glyphs: glyph_run
                                    .positioned_glyphs()
                                    .map(|g| anyrender::Glyph {
                                        id: g.id,
                                        x: g.x,
                                        y: g.y,
                                    })
                                    .collect(),
                                color: brush.color,
                                hit: brush.hit,
                                skew: synthesis
                                    .skew()
                                    .map(|a| kurbo::Affine::skew(a.to_radians().tan() as f64, 0.0)),
                                embolden: if synthesis.embolden() {
                                    kurbo::Vec2::new(0.02 * run.font_size() as f64, 0.0)
                                } else {
                                    kurbo::Vec2::ZERO
                                },
                            });
                        }
                        PositionedLayoutItem::InlineBox(ibox) => {
                            inline_boxes.push(super::layout::PositionedInlineBox {
                                id: ibox.id,
                                rect: Rect {
                                    x: ibox.x as f64,
                                    y: ibox.y as f64,
                                    width: ibox.width as f64,
                                    height: ibox.height as f64,
                                },
                            });
                        }
                    }
                }
                LineBox {
                    rect: Rect {
                        x: m.offset as f64,
                        y: m.block_min_coord as f64,
                        width: (m.advance - m.hanging_advance) as f64,
                        height: m.line_height as f64,
                    },
                    text_runs: runs,
                    inline_boxes,
                }
            })
            .collect()
    });
    SHAPING_NANOS.with(|n| n.set(n.get() + start.elapsed().as_nanos()));
    result
}
