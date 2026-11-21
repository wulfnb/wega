//! Form entries and submission from Wega's sole tree/editor/file state.
use super::*;
use std::{fmt::Display, str::FromStr};
use wega_network::document::{
    navigation::NavigationOptions,
    net::{Body, Entry, EntryValue, FormData, Method},
};
/// https://url.spec.whatwg.org/#default-encode-set
const DEFAULT_ENCODE_SET: percent_encoding::AsciiSet = percent_encoding::CONTROLS
    // Query Set
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    // Path Set
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

impl WegaDocument {
    pub fn form_data(&self, form: NodeKey, submitter: Option<NodeKey>) -> FormData {
        let mut entries = FormData::new();
        let mut push = |name: &str, value: EntryValue| {
            entries.0.push(Entry {
                name: name.to_owned(),
                value,
            })
        };
        for key in wega_dom::preorder(self, self.root()) {
            let Some(node) = self.node(key) else { continue };
            let Some(tag) = node.element_name().map(|n| n.local) else {
                continue;
            };
            if !matches!(tag, "input" | "select" | "textarea" | "button")
                || wega_dom::content::form_owner(self, key) != Some(form)
                || node.attr("disabled").is_some()
            {
                continue;
            }
            let mut ancestor = node.parent();
            let mut in_datalist = false;
            while let Some(key) = ancestor {
                let Some(node) = self.node(key) else { break };
                if node.element_name().is_some_and(|n| n.local == "datalist") {
                    in_datalist = true;
                    break;
                }
                ancestor = node.parent();
            }
            if in_datalist {
                continue;
            }
            let Some(name) = node.attr("name").filter(|n| !n.is_empty()) else {
                continue;
            };
            let kind = node.attr("type").unwrap_or_default().to_ascii_lowercase();
            let value = || EntryValue::String(self.input_value(key).unwrap_or_default());
            match tag {
                "button" => {
                    if Some(key) == submitter {
                        push(name, value());
                    }
                }
                "select" => {
                    for option in wega_dom::preorder(self, key).skip(1) {
                        let Some(node) = self.node(option) else {
                            continue;
                        };
                        if node.element_name().is_some_and(|n| n.local == "option")
                            && self.option_selected(option)
                            && node.attr("disabled").is_none()
                        {
                            push(
                                name,
                                EntryValue::String(
                                    node.attr("value").map(str::to_owned).unwrap_or_else(|| {
                                        wega_dom::content::text_content(self, option)
                                            .unwrap_or_default()
                                            .trim()
                                            .to_owned()
                                    }),
                                ),
                            );
                        }
                    }
                }
                "textarea" => push(name, value()),
                _ => match kind.as_str() {
                    "submit" | "button" | "reset" | "image" => {
                        if Some(key) == submitter && kind == "submit" {
                            push(name, value());
                        }
                    }
                    "checkbox" | "radio" => {
                        if self.checked(key) {
                            push(
                                name,
                                EntryValue::String(node.attr("value").unwrap_or("on").into()),
                            );
                        }
                    }
                    "file" => {
                        if let Some(paths) = self.files.get(&key).filter(|p| !p.is_empty()) {
                            for path in paths {
                                push(name, EntryValue::File(path.clone()));
                            }
                        } else {
                            push(name, EntryValue::EmptyFile);
                        }
                    }
                    "hidden" if name.eq_ignore_ascii_case("_charset_") => {
                        push(name, EntryValue::String("UTF-8".into()))
                    }
                    _ => push(name, value()),
                },
            }
        }
        entries
    }
    pub fn submit_form(&self, node_id: NodeKey, submitter_id: NodeKey) {
        let Some(form) = self.node(node_id) else {
            return;
        };
        if !form.element_name().is_some_and(|n| n.local == "form") {
            return;
        }
        let entry = self.form_data(node_id, Some(submitter_id));
        let attr = |name: &str, override_name: &str| {
            self.node(submitter_id)
                .and_then(|n| n.attr(override_name))
                .or_else(|| form.attr(name))
        };
        let method = attr("method", "formmethod")
            .and_then(|v| v.parse::<FormMethod>().ok())
            .unwrap_or(FormMethod::Get);
        let action = attr("action", "formaction").unwrap_or_default();
        let mut parsed_action = self
            .url()
            .join(action)
            .unwrap_or_else(|_| self.url().clone());

        let scheme = parsed_action.scheme();

        let enctype = attr("enctype", "formenctype")
            .and_then(|v| v.parse::<RequestContentType>().ok())
            .unwrap_or(RequestContentType::FormUrlEncoded);

        let mut post_resource = Body::Empty;

        match (scheme, method) {
            ("http" | "https" | "data", FormMethod::Get) => {
                let pairs = convert_to_list_of_name_value_pairs(entry);
                let mut query = String::new();
                url::form_urlencoded::Serializer::new(&mut query).extend_pairs(pairs);
                parsed_action.set_query(Some(&query));
            }
            ("http" | "https", FormMethod::Post) => post_resource = Body::Form(entry),
            ("mailto", FormMethod::Get) => {
                let pairs = convert_to_list_of_name_value_pairs(entry);
                parsed_action.query_pairs_mut().extend_pairs(pairs);
            }
            ("mailto", FormMethod::Post) => {
                let pairs = convert_to_list_of_name_value_pairs(entry);
                let body = match enctype {
                    RequestContentType::TextPlain => {
                        let body = encode_text_plain(&pairs);
                        percent_encoding::utf8_percent_encode(&body, &DEFAULT_ENCODE_SET)
                            .to_string()
                    }
                    _ => {
                        let mut body = String::new();
                        url::form_urlencoded::Serializer::new(&mut body).extend_pairs(pairs);
                        body
                    }
                };
                let mut query = if let Some(query) = parsed_action.query() {
                    let mut query = query.to_string();
                    query.push('&');
                    query
                } else {
                    String::new()
                };
                query.push_str("body=");
                query.push_str(&body);
                parsed_action.set_query(Some(&query));
            }
            _ => {
                return;
            }
        }

        let method = method.try_into().unwrap_or_default();

        let navigation_options = NavigationOptions::new(
            parsed_action,
            Some(enctype.to_string()),
            self.root().as_u64() as usize,
        )
        .set_document_resource(post_resource)
        .set_method(method);

        if let Some(navigation) = &self.navigation {
            navigation.navigate_to(navigation_options);
        }
    }
}
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum FormMethod {
    Get,
    Post,
    Dialog,
}
impl FromStr for FormMethod {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "get" => FormMethod::Get,
            "post" => FormMethod::Post,
            "dialog" => FormMethod::Dialog,
            _ => return Err(()),
        })
    }
}
impl TryFrom<FormMethod> for Method {
    type Error = &'static str;
    fn try_from(method: FormMethod) -> Result<Self, Self::Error> {
        Ok(match method {
            FormMethod::Get => Method::GET,
            FormMethod::Post => Method::POST,
            FormMethod::Dialog => return Err("Dialog is not an HTTP method"),
        })
    }
}
/// Supported content types for HTTP requests
#[derive(Debug, Clone)]
pub enum RequestContentType {
    /// application/x-www-form-urlencoded
    FormUrlEncoded,
    /// multipart/form-data
    MultipartFormData,
    /// text/plain
    TextPlain,
}

impl FromStr for RequestContentType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "application/x-www-form-urlencoded" => RequestContentType::FormUrlEncoded,
            "multipart/form-data" => RequestContentType::MultipartFormData,
            "text/plain" => RequestContentType::TextPlain,
            _ => return Err(()),
        })
    }
}

impl Display for RequestContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestContentType::FormUrlEncoded => write!(f, "application/x-www-form-urlencoded"),
            RequestContentType::MultipartFormData => write!(f, "multipart/form-data"),
            RequestContentType::TextPlain => write!(f, "text/plain"),
        }
    }
}

/// Converts the entry list to a vector of name-value pairs with normalized line endings
/// https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#converting-an-entry-list-to-a-list-of-name-value-pairs
fn convert_to_list_of_name_value_pairs(form_data: FormData) -> Vec<(String, String)> {
    form_data
        .iter()
        .map(|Entry { name, value }| {
            let name = normalize_line_endings(name.as_ref());
            let value = normalize_line_endings(value.as_ref());
            (name, value)
        })
        .collect()
}

/// Normalizes line endings in a string according to HTML spec
/// Converts single CR or LF to CRLF pairs according to HTML form submission requirements
fn normalize_line_endings(input: &str) -> String {
    // Replace every occurrence of U+000D (CR) not followed by U+000A (LF),
    // and every occurrence of U+000A (LF) not preceded by U+000D (CR),
    // in value, by a string consisting of U+000D (CR) and U+000A (LF).

    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(current) = chars.next() {
        match (current, chars.peek()) {
            ('\r', Some('\n')) => {
                result.push_str("\r\n");
                chars.next();
            }
            ('\r' | '\n', _) => {
                result.push_str("\r\n");
            }
            _ => result.push(current),
        }
    }

    result
}

/// Encodes form data as text/plain according to HTML spec given an slice of name-value pairs
/// https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#text/plain-encoding-algorithm
fn encode_text_plain<T: AsRef<str>, U: AsRef<str>>(input: &[(T, U)]) -> String {
    let mut out = String::new();
    for (name, value) in input {
        out.push_str(name.as_ref());
        out.push('=');
        out.push_str(value.as_ref());
        out.push_str("\r\n");
    }
    out
}
