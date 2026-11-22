//! Authorized resource requests and document-thread application. No local fetcher.
use super::*;
use ::selectors::context::QuirksMode;
use style::{
    media_queries::MediaList,
    shared_lock::Locked,
    stylesheets::{
        AllowImportRules, CssRule, ImportRule, Origin, Stylesheet, StylesheetInDocument,
        StylesheetLoader as Loader, UrlExtraData,
        import_rule::{ImportLayer, ImportSheet, ImportSupportsCondition},
    },
    values::{CssUrl, SourceLocation},
};
use wega_image::{RasterImageData, SvgImageData};
use wega_network::document::net::{Bytes, NetHandler, Request};

#[derive(Clone)]
pub(super) enum Image {
    Raster(RasterImageData),
    Svg(SvgImageData),
}
pub(super) enum Message {
    Sheet {
        key: NodeKey,
        requested: Url,
        sheet: DocumentStyleSheet,
    },
    Import(StyleArc<Locked<ImportRule>>, StyleArc<Stylesheet>),
    Image(Url, Image),
    Font(Bytes, FontOverrides),
}
struct Handler<F>(F);
impl<F: FnOnce(String, Bytes) + Send + Sync + 'static> NetHandler for Handler<F> {
    fn bytes(self: Box<Self>, url: String, bytes: Bytes) {
        (self.0)(url, bytes);
    }
}
fn request(url: Url, signal: Option<&AbortSignal>, destination: &'static str) -> Request {
    let mut request = Request::get(url);
    request.signal = signal.cloned();
    request
        .headers
        .insert("sec-fetch-dest", destination.parse().unwrap());
    request
}
#[derive(Clone)]
pub(super) struct StylesheetLoader {
    tx: mpsc::Sender<Message>,
    doc_id: usize,
    net: Option<Arc<dyn NetProvider>>,
    signal: Option<AbortSignal>,
    depth: usize,
}
impl Loader for StylesheetLoader {
    fn request_stylesheet(
        &self,
        url: CssUrl,
        location: SourceLocation,
        lock: &SharedRwLock,
        media: StyleArc<Locked<MediaList>>,
        supports: Option<ImportSupportsCondition>,
        layer: ImportLayer,
    ) -> StyleArc<Locked<ImportRule>> {
        if self.depth >= 8 || !supports.as_ref().is_none_or(|s| s.enabled) {
            return StyleArc::new(lock.wrap(ImportRule {
                url,
                stylesheet: ImportSheet::new_refused(),
                supports,
                layer,
                source_location: location,
            }));
        }
        let Some(resolved) = url.url().map(|u| u.as_ref().clone()) else {
            return StyleArc::new(lock.wrap(ImportRule {
                url,
                stylesheet: ImportSheet::new_refused(),
                supports,
                layer,
                source_location: location,
            }));
        };
        let import = StyleArc::new(lock.wrap(ImportRule {
            url,
            stylesheet: ImportSheet::new_pending(),
            supports,
            layer,
            source_location: location,
        }));
        if let Some(net) = &self.net {
            let loader = Self {
                depth: self.depth + 1,
                ..self.clone()
            };
            let lock = lock.clone();
            let target = import.clone();
            let fallback = resolved.clone();
            net.fetch(
                self.doc_id,
                request(resolved, self.signal.as_ref(), "style"),
                Box::new(Handler(move |final_url: String, bytes: Bytes| {
                    if loader.signal.as_ref().is_some_and(AbortSignal::aborted) {
                        return;
                    }
                    let Ok(css) = std::str::from_utf8(&bytes) else {
                        return;
                    };
                    let base = Url::parse(&final_url).unwrap_or(fallback);
                    let sheet = StyleArc::new(Stylesheet::from_str(
                        css,
                        base.into(),
                        Origin::Author,
                        media,
                        lock.clone(),
                        Some(&loader),
                        Some(&CssReporter),
                        QuirksMode::NoQuirks,
                        AllowImportRules::Yes,
                    ));
                    let guard = lock.read();
                    fetch_fonts(sheet.contents(&guard).rules(&guard).iter(), &loader, &guard);
                    drop(guard);
                    let _ = loader.tx.send(Message::Import(target, sheet));
                })),
            );
        }
        import
    }
}
pub(super) struct CssReporter;
impl style::error_reporting::ParseErrorReporter for CssReporter {
    fn report_error(
        &self,
        url: &UrlExtraData,
        location: cssparser::SourceLocation,
        error: style::error_reporting::ContextualParseError,
    ) {
        let message = error.to_string();
        let event = serde_json::json!({"category":"css","severity":"warning","message":message.chars().take(16384).collect::<String>(),"url":url.as_str().chars().take(8192).collect::<String>(),"line":location.line as u64+1,"column":location.column,"truncated":message.chars().count()>16384});
        eprintln!("WEGA_DIAGNOSTIC\t{event}");
    }
}
impl WegaDocument {
    pub(super) fn loader(&self) -> StylesheetLoader {
        StylesheetLoader {
            tx: self.tx.clone(),
            doc_id: self.root().as_u64() as usize,
            net: self.net.clone(),
            signal: self.signal.clone(),
            depth: 0,
        }
    }
    pub(super) fn fetch_fonts<'a>(&self, rules: impl Iterator<Item = &'a CssRule>) {
        fetch_fonts(rules, &self.loader(), &self.guard.read());
    }
    pub(super) fn refresh_style(&mut self, key: NodeKey) {
        if !self
            .node(key)
            .and_then(NodeView::element_name)
            .is_some_and(|n| n.local == "style")
        {
            return;
        }
        let source = wega_dom::content::text_content(self, key).unwrap_or_default();
        if self
            .inline_sources
            .get(&key)
            .is_some_and(|(old, generation)| {
                *old == source && *generation == self.stylesheet_generation
            })
        {
            return;
        }
        let sheet = Stylesheet::from_str(
            &source,
            self.url().clone().into(),
            Origin::Author,
            StyleArc::new(self.guard.wrap(MediaList::empty())),
            self.guard.clone(),
            Some(&self.loader()),
            Some(&CssReporter),
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        {
            let guard = self.guard.read();
            fetch_fonts(
                sheet.contents(&guard).rules(&guard).iter(),
                &self.loader(),
                &guard,
            );
        }
        self.nodes_to_stylesheet
            .insert(key, DocumentStyleSheet(StyleArc::new(sheet)));
        self.stylesheet_generation = self.stylesheet_generation.wrapping_add(1);
        self.inline_sources
            .insert(key, (source, self.stylesheet_generation));
    }
    pub(super) fn discover_resources(&mut self, key: NodeKey) {
        let Some(node) = self.node(key) else { return };
        let Some(name) = node.element_name() else {
            return;
        };
        let style = node.attr("style").map(|s| s.to_owned());
        if name.local == "link"
            && node.attr("rel").is_some_and(|r| {
                r.split_ascii_whitespace()
                    .any(|r| r.eq_ignore_ascii_case("stylesheet"))
            })
        {
            let Some(requested) = node.attr("href").and_then(|v| self.url().join(v).ok()) else {
                return;
            };
            if self
                .stylesheet_requests
                .get(&key)
                .is_some_and(|u| u == requested.as_str())
            {
                return;
            }
            self.nodes_to_stylesheet.remove(&key);
            self.stylesheet_requests.insert(key, requested.to_string());
            if let Some(net) = &self.net {
                let loader = self.loader();
                let lock = self.guard.clone();
                let requested_copy = requested.clone();
                net.fetch(
                    loader.doc_id,
                    request(requested, self.signal.as_ref(), "style"),
                    Box::new(Handler(move |final_url: String, bytes: Bytes| {
                        if loader.signal.as_ref().is_some_and(AbortSignal::aborted) {
                            return;
                        }
                        let Ok(css) = std::str::from_utf8(&bytes) else {
                            return;
                        };
                        let base =
                            Url::parse(&final_url).unwrap_or_else(|_| requested_copy.clone());
                        let sheet = Stylesheet::from_str(
                            css,
                            base.into(),
                            Origin::Author,
                            StyleArc::new(lock.wrap(MediaList::empty())),
                            lock.clone(),
                            Some(&loader),
                            Some(&CssReporter),
                            QuirksMode::NoQuirks,
                            AllowImportRules::Yes,
                        );
                        let _ = loader.tx.send(Message::Sheet {
                            key,
                            requested: requested_copy,
                            sheet: DocumentStyleSheet(StyleArc::new(sheet)),
                        });
                    })),
                );
            }
        } else if name.local == "img" {
            if let Some(url) = node.attr("src").and_then(|v| self.url().join(v).ok()) {
                self.image_users.insert(key, url.clone());
                self.image(url);
            }
        }
        if let Some(style) = style {
            let _ = self.parse_style_attr_block(&style);
        }
    }
    fn image(&mut self, url: Url) -> Option<Image> {
        if let Some(image) = self.images.get(&url) {
            return image.clone();
        }
        self.images.insert(url.clone(), None);
        if let Some(net) = &self.net {
            let tx = self.tx.clone();
            let signal = self.signal.clone();
            let requested = url.clone();
            let svg_fonts = self.svg_fonts.clone();
            net.fetch(
                self.root().as_u64() as usize,
                request(url, self.signal.as_ref(), "image"),
                Box::new(Handler(move |_: String, bytes: Bytes| {
                    if signal.as_ref().is_some_and(AbortSignal::aborted) {
                        return;
                    }
                    use wega_image::{RasterDecodeError, decode_raster};
                    let image = match decode_raster(&bytes) {
                        Ok(image) => Image::Raster(RasterImageData::new(
                            image.width,
                            image.height,
                            image.data,
                        )),
                        Err(RasterDecodeError::Rejected(_)) => return,
                        Err(RasterDecodeError::Format(_)) => {
                            let Ok(image) = SvgImageData::from_data(
                                &bytes,
                                &usvg::Options {
                                    fontdb: svg_fonts,
                                    ..Default::default()
                                },
                            ) else {
                                return;
                            };
                            Image::Svg(image)
                        }
                    };
                    let _ = tx.send(Message::Image(requested, image));
                })),
            );
        }
        None
    }
    pub fn release_image_cache(&mut self) {
        self.image_users.retain(|key, url| {
            self.tree
                .node(*key)
                .and_then(|n| n.attr("src"))
                .and_then(|v| self.tree.base_url().join(v).ok())
                .as_ref()
                == Some(url)
        });
        self.background_users
            .retain(|key, _| self.tree.node(*key).is_some());
        let used: HashSet<_> = self
            .image_users
            .values()
            .chain(self.background_users.values())
            .collect();
        self.images
            .retain(|url, image| image.is_none() || used.contains(url));
    }
    pub fn handle_messages(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            if self.signal.as_ref().is_some_and(AbortSignal::aborted) {
                continue;
            }
            self.resource_generation = self.resource_generation.wrapping_add(1);
            match message {
                Message::Sheet {
                    key,
                    requested,
                    sheet,
                } => {
                    if self
                        .node(key)
                        .and_then(|n| n.attr("href"))
                        .and_then(|v| self.url().join(v).ok())
                        .as_ref()
                        != Some(&requested)
                    {
                        continue;
                    }
                    {
                        let guard = self.guard.read();
                        fetch_fonts(
                            sheet.contents(&guard).rules(&guard).iter(),
                            &self.loader(),
                            &guard,
                        );
                    }
                    self.nodes_to_stylesheet.insert(key, sheet);
                    self.stylesheet_generation = self.stylesheet_generation.wrapping_add(1);
                }
                Message::Import(import, sheet) => {
                    import.write_with(&mut self.guard.write()).stylesheet =
                        ImportSheet::Sheet(sheet);
                }
                Message::Image(url, image) => {
                    self.images.insert(url, Some(image));
                }
                Message::Font(bytes, info) => {
                    let mut fonts = self
                        .fonts
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let override_info = parley::fontique::FontInfoOverride {
                        family_name: info.family_name.as_deref(),
                        weight: info.weight.map(parley::fontique::FontWeight::new),
                        style: info.style,
                        ..Default::default()
                    };
                    fonts.collection.register_fonts(
                        parley::fontique::Blob::new(Arc::new(bytes)),
                        Some(override_info),
                    );
                }
            }
        }
    }
    pub(super) fn attach_images(&mut self, b: &mut LayoutBox) {
        fn set(b: &mut LayoutBox, image: Image, background: bool) {
            match image {
                Image::Raster(image) => {
                    let image = peniko::ImageData {
                        data: image.data,
                        format: peniko::ImageFormat::Rgba8,
                        alpha_type: peniko::ImageAlphaType::Alpha,
                        width: image.width,
                        height: image.height,
                    };
                    if background {
                        b.background_image = Some(image);
                    } else {
                        b.image = Some(image);
                    }
                }
                Image::Svg(image) => {
                    let image = wega_image::SvgGraphic::from_decoded(
                        image.tree,
                        image.intrinsic_dimensions.view_box,
                        image.intrinsic_dimensions.preserve_aspect_ratio,
                        image.intrinsic_dimensions.degenerate_view_box,
                    );
                    if background {
                        b.background_svg = Some(image);
                    } else {
                        b.svg = Some(image);
                    }
                }
            }
        }
        if let Some(key) = b.node {
            if self
                .node(key)
                .and_then(NodeView::element_name)
                .is_some_and(|n| n.local == "svg")
            {
                b.svg = wega_image::svg_source(self, key).and_then(|s| {
                    wega_image::decode_svg_with_options(
                        s.as_bytes(),
                        &usvg::Options {
                            fontdb: self.svg_fonts.clone(),
                            ..Default::default()
                        },
                    )
                });
            }
            if let Some(url) = self
                .node(key)
                .and_then(|n| n.attr("src"))
                .and_then(|v| self.url().join(v).ok())
                && let Some(image) = self.image(url)
            {
                set(b, image, false);
            }
        }
        if let Some(url) = b
            .style
            .background_image
            .as_ref()
            .and_then(|v| self.url().join(v).ok())
        {
            if let Some(key) = b.node {
                self.background_users.insert(key, url.clone());
            }
            if let Some(image) = self.image(url) {
                set(b, image, true);
            }
        }
        for child in &mut b.children {
            self.attach_images(child);
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct FontOverrides {
    family_name: Option<String>,
    weight: Option<f32>,
    style: Option<parley::fontique::FontStyle>,
}
fn fetch_fonts<'a>(
    rules: impl Iterator<Item = &'a CssRule>,
    loader: &StylesheetLoader,
    guard: &style::shared_lock::SharedRwLockReadGuard,
) {
    use style::font_face::{
        FontFaceSourceFormat, FontFaceSourceFormatKeyword as Format, FontStyleRange, Source,
    };
    let Some(net) = &loader.net else {
        return;
    };
    for rule in rules {
        let CssRule::FontFace(rule) = rule else {
            continue;
        };
        let descriptor = &rule.read_with(guard).descriptors;
        let Some(family) = &descriptor.font_family else {
            continue;
        };
        let Some(sources) = &descriptor.src else {
            continue;
        };
        let overrides = FontOverrides {
            family_name: Some(family.name.to_string()),
            weight: descriptor
                .font_weight
                .as_ref()
                .and_then(|r| r.0.compute().map(|w| w.value())),
            style: descriptor.font_style.as_ref().map(|style| match style {
                FontStyleRange::Italic => parley::fontique::FontStyle::Italic,
                FontStyleRange::Oblique(min, max) => {
                    let angle = min.degrees();
                    if angle.is_none_or(|a| a == 0.0) && max.degrees().is_none_or(|a| a == 0.0) {
                        parley::fontique::FontStyle::Normal
                    } else {
                        parley::fontique::FontStyle::Oblique(angle)
                    }
                }
            }),
        };
        let source = sources.0.iter().find_map(|source| {
            let Source::Url(source) = source else {
                return None;
            };
            let mut format = match &source.format_hint {
                Some(FontFaceSourceFormat::Keyword(format)) => *format,
                Some(FontFaceSourceFormat::String(format)) => match format.as_str() {
                    "woff2" => Format::Woff2,
                    "ttf" => Format::Truetype,
                    "otf" => Format::Opentype,
                    _ => Format::None,
                },
                _ => Format::None,
            };
            if format == Format::None
                && let Some((_, extension)) = source.url.as_str().rsplit_once('.')
            {
                format = match extension {
                    "woff2" => Format::Woff2,
                    "woff" => Format::Woff,
                    "ttf" => Format::Truetype,
                    "otf" => Format::Opentype,
                    "svg" => Format::Svg,
                    "eot" => Format::EmbeddedOpentype,
                    _ => Format::None,
                };
            }
            if matches!(format, Format::Svg | Format::EmbeddedOpentype) {
                return None;
            }
            Some((source.url.url()?.as_ref().clone(), format))
        });
        if let Some((url, format)) = source {
            let loader = loader.clone();
            net.fetch(
                loader.doc_id,
                request(url, loader.signal.as_ref(), "font"),
                Box::new(Handler(move |_: String, mut bytes: Bytes| {
                    if loader.signal.as_ref().is_some_and(AbortSignal::aborted) {
                        return;
                    }
                    let format = if format == Format::None {
                        match bytes.get(..4) {
                            Some(b"wOFF") => Format::Woff,
                            Some(b"wOF2") => Format::Woff2,
                            Some(b"OTTO") => Format::Opentype,
                            Some(b"\0\x01\0\0" | b"true") => Format::Truetype,
                            _ => Format::None,
                        }
                    } else {
                        format
                    };
                    match format {
                        Format::Woff => {
                            if let Ok(decoded) = wuff::decompress_woff1(&bytes) {
                                bytes = Bytes::from(decoded);
                            }
                        }
                        Format::Woff2 => {
                            if let Ok(decoded) = wuff::decompress_woff2(&bytes) {
                                bytes = Bytes::from(decoded);
                            }
                        }
                        Format::None => return,
                        _ => {}
                    }
                    let _ = loader.tx.send(Message::Font(bytes, overrides));
                })),
            );
        }
    }
}
