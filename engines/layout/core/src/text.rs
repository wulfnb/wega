//! Native typography. Parley provides font selection, Unicode shaping, bidi and
//! line breaking; neither Blitz styles nor Blitz layout are consulted here.
use crate::{Color, ComputedStyle, TextAlign, WhiteSpace};
use crate::{LayoutBox, LineBox, Rect};
use parley::{FontContext, LayoutContext, PositionedLayoutItem, TextStyle};
use std::cell::{Cell, RefCell};
use wega_dom::NodeKey;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TextBrush {
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
        let fonts = font_context(source.as_deref().map(Vec::as_slice));
        TEXT.with(|cx| *cx.borrow_mut() = (fonts, LayoutContext::new()));
        *active.borrow_mut() = Some(source);
    });
}

pub fn sync_font_context(doc_fonts: std::sync::Arc<std::sync::Mutex<FontContext>>) {
    if let Ok(guard) = doc_fonts.lock() {
        TEXT.with(|cx| {
            cx.borrow_mut().0 = guard.clone();
        });
    }
}

pub fn register_font_bytes(
    family_name: &str,
    bytes: &[u8],
    weight: Option<u16>,
    style: Option<parley::fontique::FontStyle>,
) {
    use parley::fontique::{Blob, FontInfoOverride, FontWeight};
    let font = Blob::new(std::sync::Arc::new(bytes.to_vec()) as _);
    let info = FontInfoOverride {
        family_name: Some(family_name),
        weight: weight.map(|w| FontWeight::new(w as f32)),
        style,
        ..Default::default()
    };
    TEXT.with(|cx| {
        let mut text = cx.borrow_mut();
        text.0.collection.register_fonts(font, Some(info));
    });
}

pub(crate) fn text_style(
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

pub(crate) fn last_baseline(b: &LayoutBox) -> Option<f64> {
    if let Some(line) = b.lines.last() {
        return Some(b.content_box().y + line.baseline);
    }
    b.children
        .iter()
        .rev()
        .filter(|c| {
            !matches!(
                c.style.position,
                crate::Position::Absolute | crate::Position::Fixed
            )
        })
        .find_map(last_baseline)
}

pub(crate) fn append_inline(
    builder: &mut parley::TreeBuilder<'_, TextBrush>,
    b: &LayoutBox,
    owner: Option<NodeKey>,
    alpha: f32,
) {
    if b.box_type != crate::BoxType::Text
        && matches!(
            b.style.position,
            crate::Position::Absolute | crate::Position::Fixed
        )
    {
        return;
    }
    if b.is_atomic_inline() || b.form_state.is_some() || b.image.is_some() || b.svg.is_some() {
        let id = b.node.map(|k| k.as_u64()).unwrap_or(0);
        let vertical_align = match b.style.vertical_align {
            crate::VerticalAlign::Baseline => parley::VerticalAlign::default(),
            crate::VerticalAlign::Middle => parley::VerticalAlign::MIDDLE,
            crate::VerticalAlign::Top => parley::VerticalAlign::new(
                parley::AlignmentBaseline::Baseline,
                parley::BaselineShift::Top,
            ),
            crate::VerticalAlign::Bottom => parley::VerticalAlign::new(
                parley::AlignmentBaseline::Baseline,
                parley::BaselineShift::Bottom,
            ),
            crate::VerticalAlign::TextTop => parley::VerticalAlign::new(
                parley::AlignmentBaseline::TextTop,
                parley::BaselineShift::Length(0.0),
            ),
            crate::VerticalAlign::TextBottom => parley::VerticalAlign::new(
                parley::AlignmentBaseline::TextBottom,
                parley::BaselineShift::Length(0.0),
            ),
            crate::VerticalAlign::Sub => parley::VerticalAlign::new(
                parley::AlignmentBaseline::Baseline,
                parley::BaselineShift::Sub,
            ),
            crate::VerticalAlign::Super => parley::VerticalAlign::new(
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
            baseline: if b.is_atomic_inline()
                && b.style.overflow_x == crate::Overflow::Visible
                && b.style.overflow_y == crate::Overflow::Visible
            {
                last_baseline(b).map(|y| (b.margin.top + y - b.border_box.y) as f32)
            } else {
                None
            },
            vertical_align,
        });
        return;
    }
    let owner = if b.box_type == crate::BoxType::Text {
        owner.or(b.node)
    } else {
        b.node.or(owner)
    };
    let alpha = alpha
        * if b.box_type == crate::BoxType::Text {
            1.0
        } else {
            b.style.opacity
        };
    let mut style = text_style(&b.style, owner, alpha);
    // A <br> is a forced break even under normal whitespace collapsing.
    if b.text_content.as_deref() == Some("\n") && b.box_type != crate::BoxType::Text {
        style.white_space_collapse = parley::WhiteSpaceCollapse::Preserve;
    }
    builder.push_style_span(style);
    // Horizontal margin, border and padding of an inline element occupy line
    // space at its start and end; reserve it with empty spacer boxes.
    let edges = if b.box_type == crate::BoxType::Inline {
        let (fs, rfs) = (b.style.font_size, b.style.root_font_size);
        let side = |m: &crate::Length, p: &crate::Length, border: f64| {
            (m.resolve(0.0, fs, rfs) + p.resolve(0.0, fs, rfs) + border) as f32
        };
        (
            side(
                &b.style.margin.left,
                &b.style.padding.left,
                b.style.borders.left.width,
            ),
            side(
                &b.style.margin.right,
                &b.style.padding.right,
                b.style.borders.right.width,
            ),
        )
    } else {
        (0.0, 0.0)
    };
    let spacer = |builder: &mut parley::TreeBuilder<'_, TextBrush>, width: f32| {
        if width.is_finite() && width.abs() > 0.01 {
            builder.push_inline_box(parley::InlineBox {
                id: INLINE_SPACER,
                index: 0,
                width: width.max(0.0),
                height: 0.0,
                kind: parley::InlineBoxKind::InFlow,
                baseline: None,
                vertical_align: parley::VerticalAlign::default(),
            });
        }
    };
    spacer(builder, edges.0);
    if let Some(text) = &b.text_content {
        builder.push_text(text);
    }
    for child in &b.children {
        append_inline(builder, child, owner, alpha);
    }
    spacer(builder, edges.1);
    builder.pop_style_span();
}
/// Inline-box id reserved for inline edge spacers; never a node key.
pub const INLINE_SPACER: u64 = u64::MAX;

pub fn shaping_nanos() -> u128 {
    SHAPING_NANOS.with(Cell::get)
}
pub fn shape_inline(children: &[LayoutBox], width: f64, align: TextAlign) -> Vec<LineBox> {
    shape_inline_with_style(children, width, align, None)
}
pub fn shape_inline_with_style(
    children: &[LayoutBox],
    width: f64,
    align: TextAlign,
    style: Option<&ComputedStyle>,
) -> Vec<LineBox> {
    let start = std::time::Instant::now();
    if FONT_SOURCE.with(|s| s.borrow().is_none()) {
        configure_fonts(None);
    }
    let result = TEXT.with(|cx| {
        let (fonts, layouts) = &mut *cx.borrow_mut();
        // The line strut uses the owning block's font/line-height, including
        // image-only lines. A library default gives different descent metrics.
        let mut root = TextStyle::<TextBrush>::default();
        if let Some(s) = style {
            root.font_family =
                parley::FontFamily::Source(std::borrow::Cow::Owned(s.font_family.clone()));
            root.font_size = s.font_size as f32;
            root.font_weight = parley::FontWeight::new(s.font_weight);
            root.font_style = if s.font_italic {
                parley::FontStyle::Italic
            } else {
                parley::FontStyle::Normal
            };
            root.line_height = parley::LineHeight::Absolute(s.line_height as f32);
        }
        let mut builder = layouts.tree_builder(fonts, 1.0, true, &root);
        for child in children {
            append_inline(&mut builder, child, None, 1.0);
        }
        let (mut shaped, text) = builder.build();
        fn has_atomic(b: &LayoutBox) -> bool {
            b.is_atomic_inline()
                || b.form_state.is_some()
                || b.image.is_some()
                || b.svg.is_some()
                || b.children.iter().any(has_atomic)
        }
        if text.is_empty() && !children.iter().any(has_atomic) {
            return Vec::new();
        }
        // Half a pixel of slack: content measured at its max-content width must
        // not wrap from float rounding when laid out at exactly that width.
        shaped.break_all_lines(Some((width.max(0.0) + 0.5) as f32));
        shaped.align(
            match align {
                TextAlign::Left => parley::Alignment::Left,
                TextAlign::Right => parley::Alignment::Right,
                TextAlign::Center => parley::Alignment::Center,
                TextAlign::Justify => parley::Alignment::Justify,
            },
            parley::AlignmentOptions::default(),
        );
        let mut lines: Vec<LineBox> = shaped
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
                            inline_boxes.push(crate::PositionedInlineBox {
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
                    baseline: m.baseline as f64,
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
            .collect();
        // A final forced break (`<br>` or a preserved newline) ends the last
        // line; it does not open an empty line box after it.
        if text.ends_with('\n')
            && lines.len() > 1
            && lines
                .last()
                .is_some_and(|l| l.text_runs.is_empty() && l.inline_boxes.is_empty())
        {
            lines.pop();
        }
        lines
    });
    SHAPING_NANOS.with(|n| n.set(n.get() + start.elapsed().as_nanos()));
    result
}

/// Construct only the supplied broker fonts; system discovery is used solely
/// when the embedding browser explicitly supplies no broker font source.
pub fn font_context(source: Option<&[Vec<u8>]>) -> FontContext {
    if let Some(bytes) = source {
        use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
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
    }
}
