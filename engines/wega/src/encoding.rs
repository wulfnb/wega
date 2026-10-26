//! Character encoding and MIME decisions for fetched documents and scripts
//! (WHATWG Encoding and the HTML encoding-sniffing algorithm, simplified).
use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};

/// The `charset` parameter of a Content-Type header, if it names a known encoding.
pub(crate) fn charset(content_type: Option<&str>) -> Option<&'static Encoding> {
    content_type?.split(';').skip(1).find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| Encoding::for_label(value.trim().trim_matches('"').as_bytes()))
            .flatten()
    })
}

/// The essence ("type/subtype") of a Content-Type header, lowercased.
pub(crate) fn essence(content_type: Option<&str>) -> Option<String> {
    let essence = content_type?.split(';').next()?.trim().to_ascii_lowercase();
    (!essence.is_empty()).then_some(essence)
}

/// Prescan the first 1024 bytes for `<meta charset>` or `http-equiv=content-type`.
fn prescan(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    let mut rest = head.as_str();
    while let Some(start) = rest.find("<meta") {
        rest = &rest[start + 5..];
        let tag = &rest[..rest.find('>').unwrap_or(rest.len())];
        let Some(value) = tag
            .find("charset")
            .and_then(|at| tag[at + 7..].trim_start().strip_prefix('='))
        else {
            continue;
        };
        let label = value
            .trim_start()
            .trim_start_matches(['"', '\''])
            .split(|c: char| c == '"' || c == '\'' || c == ';' || c == '/' || c.is_whitespace())
            .next()
            .unwrap_or_default()
            .to_owned();
        if let Some(encoding) = Encoding::for_label(label.as_bytes()) {
            // A meta declaration of UTF-16 means the bytes were transcoded to an ASCII superset.
            return Some(
                if encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE {
                    UTF_8
                } else if encoding == encoding_rs::X_USER_DEFINED {
                    WINDOWS_1252
                } else {
                    encoding
                },
            );
        }
    }
    None
}

/// Decode an HTML document: BOM, then the transport charset, then a meta prescan,
/// then UTF-8 when valid, else windows-1252.
pub(crate) fn decode_html(bytes: &[u8], content_type: Option<&str>) -> (String, &'static Encoding) {
    let encoding = Encoding::for_bom(bytes)
        .map(|(encoding, _)| encoding)
        .or_else(|| charset(content_type))
        .or_else(|| prescan(bytes))
        .unwrap_or_else(|| {
            if std::str::from_utf8(bytes).is_ok() {
                UTF_8
            } else {
                WINDOWS_1252
            }
        });
    let (text, actual, _) = encoding.decode(bytes);
    (text.into_owned(), actual)
}

/// Decode a script or text resource: BOM, then the transport charset, then the
/// document's encoding.
pub(crate) fn decode_text(
    bytes: &[u8],
    content_type: Option<&str>,
    fallback: &'static Encoding,
) -> String {
    let encoding = Encoding::for_bom(bytes)
        .map(|(encoding, _)| encoding)
        .or_else(|| charset(content_type))
        .unwrap_or(fallback);
    encoding.decode(bytes).0.into_owned()
}

/// How a navigation response should be shown.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DocumentKind {
    Html,
    Xml,
    Text,
    Image,
    Download,
}

/// Decide from headers, falling back to sniffing when there is no Content-Type.
pub(crate) fn document_kind(
    content_type: Option<&str>,
    disposition: Option<&str>,
    bytes: &[u8],
) -> DocumentKind {
    if disposition.is_some_and(|d| {
        d.split(';')
            .next()
            .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("attachment"))
    }) {
        return DocumentKind::Download;
    }
    match essence(content_type).as_deref() {
        Some("text/html") => DocumentKind::Html,
        Some("application/xhtml+xml" | "image/svg+xml" | "text/xml" | "application/xml") => {
            DocumentKind::Xml
        }
        Some(
            "text/plain"
            | "text/css"
            | "text/javascript"
            | "application/javascript"
            | "application/json"
            | "text/csv"
            | "text/markdown",
        ) => DocumentKind::Text,
        Some(kind) if kind.starts_with("image/") => DocumentKind::Image,
        Some(kind) if kind.starts_with("text/") => DocumentKind::Text,
        Some(_) => DocumentKind::Download,
        None => sniff(bytes),
    }
}
fn sniff(bytes: &[u8]) -> DocumentKind {
    let head = &bytes[..bytes.len().min(512)];
    let start = head
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map_or(&head[..0], |i| &head[i..]);
    let lower = String::from_utf8_lossy(start).to_ascii_lowercase();
    if [
        "<!doctype html",
        "<html",
        "<head",
        "<body",
        "<script",
        "<iframe",
        "<h1",
        "<div",
        "<font",
        "<table",
        "<a",
        "<style",
        "<title",
        "<b",
        "<br",
        "<p",
        "<!--",
    ]
    .iter()
    .any(|tag| lower.starts_with(tag))
    {
        return DocumentKind::Html;
    }
    for (magic, kind) in [
        (&b"\x89PNG"[..], DocumentKind::Image),
        (b"GIF8", DocumentKind::Image),
        (b"\xFF\xD8\xFF", DocumentKind::Image),
        (b"%PDF-", DocumentKind::Download),
        (b"PK\x03\x04", DocumentKind::Download),
    ] {
        if bytes.starts_with(magic) {
            return kind;
        }
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return DocumentKind::Image;
    }
    if head.iter().any(|&b| b == 0 || (b < 0x09)) {
        DocumentKind::Download
    } else if lower.starts_with('<') {
        DocumentKind::Html
    } else {
        DocumentKind::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_meta_bom_and_fallback_select_the_encoding() {
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("日本語");
        let (text, encoding) = decode_html(&sjis, Some("text/html; charset=Shift_JIS"));
        assert_eq!((text.as_str(), encoding.name()), ("日本語", "Shift_JIS"));
        let latin = b"<meta charset=\"windows-1252\"><p>caf\xe9";
        assert!(decode_html(latin, None).0.ends_with("café"));
        let bom = b"\xEF\xBB\xBF<p>x";
        assert_eq!(decode_html(bom, Some("text/html; charset=latin1")).1, UTF_8);
        assert_eq!(decode_html(b"<p>caf\xe9", None).1, WINDOWS_1252);
        assert_eq!(decode_html("<p>café".as_bytes(), None).1, UTF_8);
    }
    #[test]
    fn documents_are_classified_by_header_or_sniffed() {
        assert_eq!(
            document_kind(Some("text/html; charset=utf-8"), None, b""),
            DocumentKind::Html
        );
        assert_eq!(
            document_kind(Some("text/plain"), None, b"<b>"),
            DocumentKind::Text
        );
        assert_eq!(
            document_kind(Some("image/png"), None, b""),
            DocumentKind::Image
        );
        assert_eq!(
            document_kind(Some("application/pdf"), None, b""),
            DocumentKind::Download
        );
        assert_eq!(
            document_kind(Some("text/html"), Some("attachment; filename=a.html"), b""),
            DocumentKind::Download
        );
        assert_eq!(
            document_kind(None, None, b"  <!DOCTYPE html><p>"),
            DocumentKind::Html
        );
        assert_eq!(
            document_kind(None, None, b"%PDF-1.7"),
            DocumentKind::Download
        );
        assert_eq!(
            document_kind(None, None, b"plain words"),
            DocumentKind::Text
        );
    }
}
