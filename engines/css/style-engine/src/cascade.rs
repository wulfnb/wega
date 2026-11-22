//! Cascade: matching, origin and specificity order, inheritance and `var()` resolution.
use crate::*;
use wega_css_parser::*;
use wega_css_values::*;
use wega_dom::{DocumentView, NodeKey, NodeKind, NodeView};

#[derive(Debug, Clone)]
pub(crate) struct MatchedDecl {
    property: String,
    value: String,
    typed: Parsed,
    origin: u8, // 0 = UA, 1 = Author, 2 = Inline
    important: bool,
    specificity: (usize, usize, usize),
    source_order: usize,
    /// Cascade layer rank for author rules; unlayered rules rank highest.
    layer: u32,
}

pub fn resolve_computed_style<D: DocumentView>(
    doc: &D,
    node_key: NodeKey,
    parent_style: Option<&ComputedStyle>,
    author_sheets: &[&StyleSheet],
    media_ctx: Option<&MediaContext>,
) -> ComputedStyle {
    let default_ctx = MediaContext::default();
    let m_ctx = media_ctx.unwrap_or(&default_ctx);
    set_viewport(m_ctx.viewport_width, m_ctx.viewport_height);
    let mut style = parent_style.map_or_else(ComputedStyle::default, |p| ComputedStyle {
        // Inherited properties:
        custom_properties: p.custom_properties.clone(),
        line_height_factor: p.line_height_factor,
        color: p.color,
        visible: p.visible,
        pointer_events: p.pointer_events,
        cursor: p.cursor.clone(),
        font_size: p.font_size,
        root_font_size: p.root_font_size,
        line_height: p.line_height,
        text_align: p.text_align,
        font_family: p.font_family.clone(),
        font_weight: p.font_weight,
        font_italic: p.font_italic,
        white_space: p.white_space,
        ..Default::default()
    });

    let Some(node) = doc.node(node_key) else {
        return style;
    };
    if node.kind() != NodeKind::Element {
        return style;
    };

    let is_document_element = node
        .parent()
        .and_then(|p| doc.node(p))
        .is_some_and(|n| n.kind() == NodeKind::Document);
    // Registered custom properties (`@property`) start at their initial value:
    // on every element when they do not inherit, otherwise from the root.
    for sheet in author_sheets {
        for (name, initial, inherits) in &sheet.registered_properties {
            if !inherits || !style.custom_properties.contains_key(name) {
                style
                    .custom_properties
                    .insert(name.clone(), initial.clone());
            }
        }
    }
    let mut matched: Vec<MatchedDecl> = Vec::new();

    // 1. UA Stylesheet
    let mut order = 0;
    // Candidate rules for this element from each sheet's selector map, in source order.
    let element = node.element_name().map(|n| n.local).unwrap_or_default();
    let (id, class) = (node.attr("id"), node.attr("class").unwrap_or_default());
    let candidates = |sheet: &StyleSheet| -> Vec<usize> {
        if sheet.index.covers(sheet.rules.len()) {
            sheet
                .index
                .candidates(id, class.split_whitespace(), element)
        } else {
            (0..sheet.rules.len()).collect()
        }
    };
    USER_AGENT.with(|ua| {
        for rule in candidates(ua).into_iter().map(|i| &ua.rules[i]) {
            if !rule.media.iter().all(|group| matches_media(group, m_ctx)) {
                continue;
            }
            for sel in &rule.selectors {
                if sel.matches(doc, node_key) {
                    let spec = sel.specificity();
                    for d in &rule.declarations {
                        matched.push(MatchedDecl {
                            layer: u32::MAX,
                            property: d.property.clone(),
                            value: d.value.clone(),
                            typed: d.typed.clone(),
                            origin: 0,
                            important: d.important,
                            specificity: spec,
                            source_order: order,
                        });
                        order += 1;
                    }
                }
            }
        }
    });

    // HTML presentational attributes map to author-level hints with zero
    // specificity that precede every authored rule (HTML §15 Rendering).
    for (property, value) in presentational_hints(doc, node_key) {
        matched.push(MatchedDecl {
            layer: u32::MAX,
            typed: PropertyDeclaration::parse(property, &value),
            property: property.into(),
            value,
            origin: 1,
            important: false,
            specificity: (0, 0, 0),
            source_order: 0,
        });
    }
    // 2. Author Stylesheets
    for (sheet_index, sheet) in author_sheets.iter().enumerate() {
        for rule in candidates(sheet).into_iter().map(|i| &sheet.rules[i]) {
            let layer = rule.layer.map_or(u32::MAX, |l| {
                (sheet_index as u32)
                    .saturating_mul(4096)
                    .saturating_add(l.min(4095))
            });
            if !rule.media.iter().all(|group| matches_media(group, m_ctx)) {
                continue;
            }
            for sel in &rule.selectors {
                if sel.matches(doc, node_key) {
                    let spec = sel.specificity();
                    for d in &rule.declarations {
                        matched.push(MatchedDecl {
                            layer,
                            property: d.property.clone(),
                            value: d.value.clone(),
                            typed: d.typed.clone(),
                            origin: 1,
                            important: d.important,
                            specificity: spec,
                            source_order: order,
                        });
                        order += 1;
                    }
                }
            }
        }
    }

    // 3. Inline style attribute
    if let Some(inline_css) = node.attr("style") {
        for d in parse_declarations(inline_css) {
            matched.push(MatchedDecl {
                layer: u32::MAX,
                property: d.property,
                value: d.value,
                typed: d.typed.clone(),
                origin: 2,
                important: d.important,
                specificity: (1, 0, 0), // Inline origin weight
                source_order: order,
            });
            order += 1;
        }
    }

    // Sort according to CSS Cascade:
    // 1. Important (true > false)
    // 2. Origin (Inline > Author > UA for normal; UA > Author > Inline for !important)
    // 3. Specificity
    // 4. Source order
    matched.sort_by(|a, b| {
        if a.important != b.important {
            return a.important.cmp(&b.important);
        }
        let (origin_a, origin_b) = if a.important {
            (
                if a.origin == 0 { 3 } else { a.origin },
                if b.origin == 0 { 3 } else { b.origin },
            )
        } else {
            (a.origin, b.origin)
        };
        if origin_a != origin_b {
            return origin_a.cmp(&origin_b);
        }
        // Cascade layers: later layers and then unlayered rules win for
        // normal declarations; the order reverses for !important ones.
        if a.layer != b.layer {
            return if a.important {
                b.layer.cmp(&a.layer)
            } else {
                a.layer.cmp(&b.layer)
            };
        }
        if a.specificity != b.specificity {
            return a.specificity.cmp(&b.specificity);
        }
        a.source_order.cmp(&b.source_order)
    });

    // Resolve custom properties before ordinary properties, independent of order.
    for decl in &matched {
        if decl.property.starts_with("--") {
            match decl.value.trim() {
                "initial" => {
                    style.custom_properties.remove(&decl.property);
                }
                "inherit" | "unset" => {
                    if let Some(value) =
                        parent_style.and_then(|p| p.custom_properties.get(&decl.property))
                    {
                        style
                            .custom_properties
                            .insert(decl.property.clone(), value.clone());
                    } else {
                        style.custom_properties.remove(&decl.property);
                    }
                }
                _ => {
                    style
                        .custom_properties
                        .insert(decl.property.clone(), decl.value.clone());
                }
            }
        }
    }
    style.custom_properties = resolve_variables(&style.custom_properties);
    // Pending variable shorthands participate in the cascade as their
    // longhands. Expand only after substitution, retaining the original rank.
    // Invalid-at-computed-value shorthands reset their longhands (CSS Variables
    // 3.1); an earlier declaration must not silently become the winner.
    let mut matched: Vec<MatchedDecl> = matched
        .into_iter()
        .flat_map(|d| {
            let names = shorthand_properties(&d.property);
            if names.is_empty() {
                return vec![d];
            }
            let value =
                substitute(&d.value, &style.custom_properties).unwrap_or_else(|| "unset".into());
            let mut expanded =
                expand_shorthand(Declaration::new(d.property.clone(), value, d.important));
            if expanded.is_empty() {
                expanded = names
                    .iter()
                    .map(|p| Declaration::new(*p, "unset", d.important))
                    .collect();
            }
            expanded
                .into_iter()
                .map(|e| MatchedDecl {
                    layer: u32::MAX,
                    property: e.property,
                    value: e.value,
                    typed: e.typed,
                    ..d.clone()
                })
                .collect()
        })
        .collect();
    // Font-relative computed values and currentColor use the winning font and
    // color, regardless of declaration order or selector specificity.
    matched.sort_by_key(|d| match d.property.as_str() {
        "font-size" => 0,
        "color" => 1,
        _ => 2,
    });
    for decl in matched {
        if decl.property.starts_with("--") {
            continue;
        }
        match decl.typed {
            Parsed::Typed(typed) => {
                apply_typed(&mut style, typed);
                continue;
            }
            Parsed::Invalid => continue,
            Parsed::Deferred => {}
        }
        let value = substitute(&decl.value, &style.custom_properties);
        let inherited = matches!(
            decl.property.as_str(),
            "color"
                | "visibility"
                | "pointer-events"
                | "cursor"
                | "font-size"
                | "font-family"
                | "font-weight"
                | "font-style"
                | "line-height"
                | "text-align"
                | "white-space"
        );
        let value = value
            .as_deref()
            .unwrap_or(if inherited { "inherit" } else { "initial" });
        if value == "inherit" || (value == "unset" && inherited) {
            copy_property(
                &mut style,
                parent_style.unwrap_or(&ComputedStyle::default()),
                &decl.property,
            );
        } else if value == "initial" || value == "unset" {
            copy_property(&mut style, &ComputedStyle::default(), &decl.property);
        } else if decl.property == "font-size" {
            if let Some(l) =
                Length::parse(value).filter(|l| !matches!(l, Length::Vw(_) | Length::Vh(_)))
            {
                let parent_size = parent_style.map(|p| p.font_size).unwrap_or(16.0);
                let size = l.resolve(
                    parent_size,
                    parent_size,
                    if is_document_element {
                        16.0
                    } else {
                        style.root_font_size
                    },
                );
                if size >= 0.0 && size.is_finite() {
                    style.font_size = size;
                    if is_document_element {
                        style.root_font_size = size;
                    }
                }
            }
        } else if decl.property == "color" && value.eq_ignore_ascii_case("currentcolor") {
            style.color = parent_style.map(|p| p.color).unwrap_or(Color::BLACK);
        } else {
            let resolved = if value.eq_ignore_ascii_case("currentcolor") {
                format!(
                    "rgba({},{},{},{})",
                    style.color.r,
                    style.color.g,
                    style.color.b,
                    style.color.a as f32 / 255.0
                )
            } else {
                value.to_owned()
            };
            apply_declaration(&mut style, &decl.property, &resolved);
        }
    }
    for (side, hidden) in [
        &mut style.borders.top,
        &mut style.borders.right,
        &mut style.borders.bottom,
        &mut style.borders.left,
    ]
    .into_iter()
    .zip(style.border_hidden)
    {
        if hidden {
            side.width = 0.0;
        }
    }
    if let Some(factor) = style.line_height_factor {
        style.line_height = style.font_size * factor;
    }
    // `background-clip: text` shows the background through the glyphs. Glyphs
    // are painted with a solid color, so use the gradient's middle color and
    // do not paint the background box itself.
    if style.background_clip_text {
        if style.color.a == 0
            && let Some(g) = &style.background_gradient
        {
            style.color = g.midpoint();
        }
        style.background_gradient = None;
        style.background_color = Color::TRANSPARENT;
    }
    if let Some(mask) = style.mask_image.clone() {
        style.background_image = Some(mask);
        style.background_color = Color::TRANSPARENT;
        style.background_size = style.mask_size.clone().unwrap_or(BackgroundSize::Auto);
        style.background_position = style
            .mask_position
            .unwrap_or((Length::Percent(0.0), Length::Percent(0.0)));
        style.background_repeat = style.mask_repeat.unwrap_or((true, true));
    }
    if let Some(el_name) = node.element_name() {
        let tag = el_name.local;
        if matches!(tag, "img" | "iframe" | "canvas" | "video" | "embed" | "svg") {
            if style.width == Length::Auto
                && let Some(w_attr) = node.attr("width")
                && let Some(l) =
                    Length::parse(w_attr).or_else(|| w_attr.parse::<f64>().ok().map(Length::Px))
            {
                style.width = l;
            }
            if style.height == Length::Auto
                && let Some(h_attr) = node.attr("height")
                && let Some(l) =
                    Length::parse(h_attr).or_else(|| h_attr.parse::<f64>().ok().map(Length::Px))
            {
                style.height = l;
            }
        }
    }
    if node.element_name().is_some_and(|n| n.local == "canvas") {
        if style.width == Length::Auto {
            style.width = Length::Px(300.0);
        }
        if style.height == Length::Auto {
            style.height = Length::Px(150.0);
        }
    }
    style
}

pub(crate) fn copy_property(target: &mut ComputedStyle, source: &ComputedStyle, prop: &str) {
    macro_rules! copy { ($($name:literal => $field:ident),* $(,)?) => { match prop { $( $name => target.$field=source.$field.clone(), )* _=>{} } }; }
    copy! { "display"=>display,"position"=>position,"box-sizing"=>box_sizing,
    "color"=>color,"background-color"=>background_color,"background"=>background_color,
    "width"=>width,"height"=>height,"min-width"=>min_width,"max-width"=>max_width,"min-height"=>min_height,"max-height"=>max_height,
    "margin"=>margin,"padding"=>padding,"border"=>borders,"font-size"=>font_size,"font-family"=>font_family,
    "font-weight"=>font_weight,"font-style"=>font_italic,"white-space"=>white_space,"text-align"=>text_align,
    "opacity"=>opacity,"visibility"=>visible,"pointer-events"=>pointer_events,"cursor"=>cursor,"top"=>top,"right"=>right,"bottom"=>bottom,"left"=>left,"z-index"=>z_index,
    "transform"=>transform,"box-shadow"=>box_shadow,"border-radius"=>border_radii,
    "overflow-x"=>overflow_x,"overflow-y"=>overflow_y,"flex-direction"=>flex_direction,
    "flex-grow"=>flex_grow,"flex-shrink"=>flex_shrink,"flex-basis"=>flex_basis,"align-items"=>align_items,"justify-content"=>justify_content,"object-fit"=>object_fit,"object-position"=>object_position,"vertical-align"=>vertical_align }
    if prop == "line-height" {
        target.line_height = source.line_height;
        target.line_height_factor = source.line_height_factor;
    }
    if let Some(i) = [
        "border-top-left-radius",
        "border-top-right-radius",
        "border-bottom-right-radius",
        "border-bottom-left-radius",
    ]
    .iter()
    .position(|p| *p == prop)
    {
        target.border_radii[i] = source.border_radii[i];
    }
}
