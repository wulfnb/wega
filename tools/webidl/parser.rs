//! WebIDL subset parser. Every construct outside the supported policy produces
//! a positioned error; the pipeline never guesses semantics for syntax it does
//! not understand.

use crate::ast::{
    ArgumentDefault, AttributeMember, ConstructorMember, Interface, Member, OperationMember,
    Parameter, Type,
};

#[derive(Debug)]
pub struct ParseError {
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(file: &str, line: usize, message: impl Into<String>) -> Self {
        Self {
            file: file.to_owned(),
            line,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    Str(String),
    Int(i64),
    LBrack,
    RBrack,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Semi,
    Comma,
    Eq,
    Colon,
    Question,
    Less,
    Greater,
    Dots,
}

struct Tokenizer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}
fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn tokenize(file: &str, src: &str) -> Result<Vec<(usize, Token)>, ParseError> {
    let mut out = Vec::new();
    let mut t = Tokenizer {
        src: src.as_bytes(),
        pos: 0,
        line: 1,
    };
    loop {
        while t.pos < t.src.len() {
            let b = t.src[t.pos];
            if b == b'\n' {
                t.line += 1;
                t.pos += 1;
            } else if b.is_ascii_whitespace() {
                t.pos += 1;
            } else if b == b'/' && t.src.get(t.pos + 1) == Some(&b'/') {
                while t.pos < t.src.len() && t.src[t.pos] != b'\n' {
                    t.pos += 1;
                }
            } else if b == b'/' && t.src.get(t.pos + 1) == Some(&b'*') {
                t.pos += 2;
                let start_line = t.line;
                let mut closed = false;
                while t.pos < t.src.len() {
                    if t.src[t.pos] == b'\n' {
                        t.line += 1;
                    }
                    if t.src[t.pos] == b'*' && t.src.get(t.pos + 1) == Some(&b'/') {
                        t.pos += 2;
                        closed = true;
                        break;
                    }
                    t.pos += 1;
                }
                if !closed {
                    return Err(ParseError::new(file, start_line, "unterminated block comment"));
                }
            } else {
                break;
            }
        }
        if t.pos >= t.src.len() {
            break;
        }
        let line = t.line;
        let b = t.src[t.pos];
        let two = t.src.get(t.pos + 1).copied();
        let three = t.src.get(t.pos + 2).copied();
        let token = match b {
            b'[' => {
                t.pos += 1;
                Token::LBrack
            }
            b']' => {
                t.pos += 1;
                Token::RBrack
            }
            b'(' => {
                t.pos += 1;
                Token::LParen
            }
            b')' => {
                t.pos += 1;
                Token::RParen
            }
            b'{' => {
                t.pos += 1;
                Token::LBrace
            }
            b'}' => {
                t.pos += 1;
                Token::RBrace
            }
            b';' => {
                t.pos += 1;
                Token::Semi
            }
            b',' => {
                t.pos += 1;
                Token::Comma
            }
            b'=' => {
                t.pos += 1;
                Token::Eq
            }
            b':' => {
                t.pos += 1;
                Token::Colon
            }
            b'?' => {
                t.pos += 1;
                Token::Question
            }
            b'<' => {
                t.pos += 1;
                Token::Less
            }
            b'>' => {
                t.pos += 1;
                Token::Greater
            }
            b'.' if two == Some(b'.') && three == Some(b'.') => {
                t.pos += 3;
                Token::Dots
            }
            b'"' => {
                t.pos += 1;
                let mut value = String::new();
                let mut closed = false;
                while t.pos < t.src.len() {
                    let c = t.src[t.pos];
                    if c == b'\n' {
                        t.line += 1;
                        break;
                    }
                    if c == b'"' {
                        t.pos += 1;
                        closed = true;
                        break;
                    }
                    if c == b'\\' && t.pos + 1 < t.src.len() {
                        let e = t.src[t.pos + 1];
                        let mapped = match e {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            b'"' => '"',
                            b'\\' => '\\',
                            other => other as char,
                        };
                        value.push(mapped);
                        t.pos += 2;
                    } else if c < 0x80 {
                        value.push(c as char);
                        t.pos += 1;
                    } else {
                        let rest = std::str::from_utf8(&t.src[t.pos..])
                            .map_err(|_| ParseError::new(file, line, "invalid UTF-8"))?;
                        let ch = rest.chars().next().unwrap();
                        value.push(ch);
                        t.pos += ch.len_utf8();
                    }
                }
                if !closed {
                    return Err(ParseError::new(file, line, "unterminated string literal"));
                }
                Token::Str(value)
            }
            b'0'..=b'9' => {
                let start = t.pos;
                while t.pos < t.src.len() && t.src[t.pos].is_ascii_digit() {
                    t.pos += 1;
                }
                let text = std::str::from_utf8(&t.src[start..t.pos]).unwrap();
                let value: i64 = text
                    .parse()
                    .map_err(|_| ParseError::new(file, line, "numeric literal out of range"))?;
                Token::Int(value)
            }
            _ if is_ident_start(b) => {
                let start = t.pos;
                while t.pos < t.src.len() && is_ident_cont(t.src[t.pos]) {
                    t.pos += 1;
                }
                Token::Ident(std::str::from_utf8(&t.src[start..t.pos]).unwrap().to_owned())
            }
            other => {
                let ch = other as char;
                return Err(ParseError::new(file, line, format!("unsupported character '{ch}'")));
            }
        };
        out.push((line, token));
    }
    Ok(out)
}

/// Extended-attribute bookkeeping for one declaration.
#[derive(Debug, Default, Clone)]
struct Attrs {
    names: Vec<String>,
    exposed: Vec<String>,
}

impl Attrs {
    fn contains(&self, name: &str) -> bool {
        self.names.iter().any(|entry| entry == name)
    }
}

struct Parser<'a> {
    file: &'a str,
    tokens: Vec<(usize, Token)>,
    pos: usize,
    interfaces: Vec<Interface>,
}

impl<'a> Parser<'a> {
    fn err(&self, line: usize, message: impl Into<String>) -> ParseError {
        ParseError::new(self.file, line, message)
    }
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(_, t)| t)
    }
    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset).map(|(_, t)| t)
    }
    fn cur_line(&self) -> usize {
        self.tokens
            .get(self.pos)
            .map(|(line, _)| *line)
            .or_else(|| self.tokens.last().map(|(line, _)| *line))
            .unwrap_or(1)
    }
    fn next(&mut self) -> Option<(usize, Token)> {
        let entry = self.tokens.get(self.pos)?;
        self.pos += 1;
        Some((entry.0, entry.1.clone()))
    }
    fn expect(&mut self, token: &Token, what: &str) -> Result<usize, ParseError> {
        let (line, got) = self.next().ok_or_else(|| {
            self.err(self.cur_line(), format!("unexpected end of file, expected {what}"))
        })?;
        if &got != token {
            return Err(self.err(line, format!("expected {what}, found {got:?}")));
        }
        Ok(line)
    }
    fn expect_ident(&mut self, name: &str) -> Result<usize, ParseError> {
        let (line, got) = self.next().ok_or_else(|| {
            self.err(self.cur_line(), format!("unexpected end of file, expected '{name}'"))
        })?;
        match &got {
            Token::Ident(text) if text == name => Ok(line),
            other => Err(self.err(line, format!("expected '{name}', found {other:?}"))),
        }
    }
    fn ident(&mut self, what: &str) -> Result<(String, usize), ParseError> {
        let (line, got) = self.next().ok_or_else(|| {
            self.err(self.cur_line(), format!("unexpected end of file, expected {what}"))
        })?;
        match got {
            Token::Ident(name) => Ok((name, line)),
            other => Err(self.err(line, format!("expected {what}, found {other:?}"))),
        }
    }

    fn parse(mut self) -> Result<Vec<Interface>, ParseError> {
        loop {
            match self.peek().cloned() {
                None => break,
                Some(Token::LBrack) => {
                    self.next();
                    let attrs = self.attr_list()?;
                    self.declare_interface(attrs)?;
                }
                Some(Token::Ident(ref word)) if word == "interface" => {
                    self.declare_interface(Attrs::default())?;
                }
                Some(Token::Ident(ref name)) => {
                    if self.peek_at(1) == Some(&Token::Ident("includes".to_owned())) {
                        let (owner, _) = self.ident("interface name")?;
                        self.expect_ident("includes")?;
                        let (mixin, line) = self.ident("mixin name")?;
                        self.expect(&Token::Semi, "';' after includes clause")?;
                        if let Some(last) = self.interfaces.last_mut() {
                            if last.name != owner {
                                return Err(self.err(
                                    line,
                                    format!("includes clause for '{owner}' does not follow its interface"),
                                ));
                            }
                            last.includes.push(mixin);
                            continue;
                        }
                        return Err(self.err(line, "includes clause before any interface"));
                    }
                    match name.as_str() {
                        "dictionary" | "enum" | "callback" | "typedef" => {
                            return Err(self.err(
                                self.cur_line(),
                                format!(
                                    "unsupported declaration '{name}' is not permitted in generated IDL inputs"
                                ),
                            ));
                        }
                        _ => {
                            return Err(self.err(
                                self.cur_line(),
                                format!("unsupported declaration or stray identifier '{name}'"),
                            ));
                        }
                    }
                }
                Some(other) => {
                    return Err(self.err(self.cur_line(), format!("unsupported token {other:?} at top level")));
                }
            }
        }
        Ok(std::mem::take(&mut self.interfaces))
    }

    fn declare_interface(&mut self, attrs: Attrs) -> Result<(), ParseError> {
        let line = self.expect_ident("interface")?;
        if self.peek() == Some(&Token::Ident("mixin".to_owned())) {
            self.next();
            let (name, _) = self.ident("mixin name")?;
            return Err(self.err(
                line,
                format!(
                    "interface mixin '{name}' is not permitted in generated IDL inputs; \
                     declare it in the family's base-provided mixin registry instead"
                ),
            ));
        }
        let (name, _) = self.ident("interface name")?;
        let base = if self.peek() == Some(&Token::Colon) {
            self.next();
            let (base, _) = self.ident("base interface name")?;
            Some(base)
        } else {
            None
        };
        if attrs.exposed.is_empty() {
            return Err(self.err(
                line,
                format!("interface '{name}' lacks the required [Exposed=...] attribute"),
            ));
        }
        if attrs.contains("NewObject") || attrs.contains("CEReactions") {
            return Err(self.err(line, "NewObject/CEReactions are member-level attributes"));
        }
        self.expect(&Token::LBrace, "'{' after interface header")?;
        let mut members = Vec::new();
        loop {
            match self.peek().cloned() {
                None => return Err(self.err(self.cur_line(), "unexpected end of file inside interface")),
                Some(Token::RBrace) => break,
                _ => self.declare_member(&mut members)?,
            }
        }
        self.expect(&Token::RBrace, "'}' closing interface body")?;
        self.expect(&Token::Semi, "';' after interface body")?;
        let abstract_ = attrs.contains("Abstract");
        self.interfaces.push(Interface {
            name,
            base,
            exposed: attrs.exposed,
            abstract_,
            members,
            includes: Vec::new(),
            file: self.file.to_owned(),
            line,
        });
        Ok(())
    }

    fn declare_member(&mut self, members: &mut Vec<Member>) -> Result<(), ParseError> {
        let line = self.cur_line();
        match self.peek().cloned() {
            Some(Token::LBrack) => {
                self.next();
                let attrs = self.attr_list()?;
                self.member_from_attrs(attrs, members, line)
            }
            Some(Token::Ident(ref word)) => match word.as_str() {
                "const" => Err(self.err(line, "'const' members are unsupported in generated IDL inputs")),
                "stringifier" => Err(self.err(line, "'stringifier' is unsupported in generated IDL inputs")),
                "iterable" => Err(self.err(line, "'iterable<...>' members are unsupported in generated IDL inputs")),
                "getter" | "setter" | "deleter" => Err(self.err(
                    line,
                    format!("'{word}' members are unsupported in generated IDL inputs"),
                )),
                "attribute" | "readonly" => self.member_from_attrs(Attrs::default(), members, line),
                other => {
                    // Attribute or operation with no leading attribute list.
                    let _ = other;
                    self.member_from_attrs(Attrs::default(), members, line)
                }
            },
            other => Err(self.err(line, format!("unsupported token {other:?} in interface body"))),
        }
    }

    fn member_from_attrs(
        &mut self,
        attrs: Attrs,
        members: &mut Vec<Member>,
        line: usize,
    ) -> Result<(), ParseError> {
        if !attrs.exposed.is_empty() {
            return Err(self.err(line, "'[Exposed]' is only valid on interface declarations"));
        }
        let peek = self.peek().cloned();
        match peek {
            Some(Token::Ident(ref word)) if word == "constructor" => {
                self.next();
                self.expect(&Token::LParen, "'(' after constructor name")?;
                let params = if self.peek() == Some(&Token::RParen) {
                    Vec::new()
                } else {
                    self.parameters()?
                };
                self.expect(&Token::RParen, "')' closing constructor parameters")?;
                self.expect(&Token::Semi, "';' after constructor")?;
                members.push(Member::Constructor(ConstructorMember {
                    params,
                    throws: attrs.contains("Throws"),
                    line,
                }));
                Ok(())
            }
            Some(Token::Ident(ref word)) if word == "readonly" || word == "attribute" => {
                let readonly = *word == "readonly";
                self.next();
                if readonly {
                    self.expect_ident("attribute")?;
                }
                // Servo inputs may attach a second attribute list between the
                // `attribute` keyword and the type; merge it into the first.
                let mut merged = attrs;
                if self.peek() == Some(&Token::LBrack) {
                    self.next();
                    let second = self.attr_list()?;
                    if !second.exposed.is_empty() {
                        return Err(self.err(line, "'[Exposed]' is only valid on interface declarations"));
                    }
                    merged.names.extend(second.names);
                }
                let (ty, _) = self.r#type()?;
                let (name, _) = self.ident("attribute name")?;
                self.expect(&Token::Semi, "';' after attribute")?;
                if matches!(ty, Type::Interface { .. }) {
                    return Err(self.err(
                        line,
                        format!(
                            "attribute '{name}' returns an interface; step 07.a supports primitive attributes only"
                        ),
                    ));
                }
                members.push(Member::Attribute(AttributeMember {
                    name,
                    ty,
                    readonly,
                    pure: merged.contains("Pure"),
                    throws: merged.contains("Throws"),
                    legacy_null_to_empty: merged.contains("LegacyNullToEmptyString"),
                    line,
                }));
                Ok(())
            }
            Some(Token::Ident(ref word)) if word == "stringifier" => Err(self.err(
                line,
                "'stringifier' is unsupported in generated IDL inputs",
            )),
            _ => {
                // Operation: explicit return type, name, parameters.
                let (ty, _) = self.r#type()?;
                let (name, _) = self.ident("operation name")?;
                self.expect(&Token::LParen, "'(' after operation name")?;
                let params = if self.peek() == Some(&Token::RParen) {
                    Vec::new()
                } else {
                    self.parameters()?
                };
                self.expect(&Token::RParen, "')' closing operation parameters")?;
                self.expect(&Token::Semi, "';' after operation")?;
                members.push(Member::Operation(OperationMember {
                    name,
                    ty,
                    params,
                    throws: attrs.contains("Throws"),
                    new_object: attrs.contains("NewObject"),
                    line,
                }));
                Ok(())
            }
        }
    }

    fn parameters(&mut self) -> Result<Vec<Parameter>, ParseError> {
        let mut params = Vec::new();
        loop {
            if self.peek() == Some(&Token::LBrack) {
                return Err(self.err(
                    self.cur_line(),
                    "parameter extended attributes are unsupported in generated IDL inputs",
                ));
            }
            // Variadic dot-dots before the type keyword.
            if self.peek() == Some(&Token::Dots) {
                return Err(self.err(self.cur_line(), "variadic arguments are unsupported in generated IDL inputs"));
            }
            let line = self.cur_line();
            let optional = self.peek() == Some(&Token::Ident("optional".to_owned()));
            if optional {
                self.next();
            }
            let (ty, _) = self.r#type()?;
            // Detect variadic pattern: `Type... name` — reject before trying
            // to parse the dots as a parameter name.
            if self.peek() == Some(&Token::Dots) {
                return Err(self.err(self.cur_line(), "variadic arguments are unsupported in generated IDL inputs"));
            }
            let (name, _) = self.ident("parameter name")?;
            if self.peek() == Some(&Token::Dots) {
                return Err(self.err(line, "variadic arguments are unsupported in generated IDL inputs"));
            }
            let default = if self.peek() == Some(&Token::Eq) {
                self.next();
                let default = self.argument_default(&ty, line)?;
                if !optional {
                    return Err(self.err(
                        line,
                        "default values are supported only for optional parameters in the supported subset",
                    ));
                }
                Some(default)
            } else {
                None
            };
            if optional && default.is_none() {
                return Err(self.err(line, "optional parameters require a default value"));
            }
            params.push(Parameter {
                name,
                ty,
                optional,
                default,
                line,
            });
            if self.peek() == Some(&Token::Comma) {
                self.next();
            } else {
                break;
            }
        }
        Ok(params)
    }

    fn argument_default(&mut self, ty: &Type, line: usize) -> Result<ArgumentDefault, ParseError> {
        match self.next() {
            Some((_, Token::Str(text))) => match ty {
                Type::DomString { nullable: _ } => Ok(ArgumentDefault::String(text)),
                other => Err(self.err(line, format!("string default for non-DOMString type {other:?}"))),
            },
            Some((_, Token::Int(value))) => match ty {
                Type::UnsignedLong | Type::UnsignedShort | Type::Float | Type::Double => {
                    Ok(ArgumentDefault::Number(value))
                }
                other => Err(self.err(line, format!("numeric default for non-numeric type {other:?}"))),
            },
            Some((_, Token::Ident(ref word))) => match word.as_str() {
                "true" | "false" => match ty {
                    Type::Boolean => Ok(ArgumentDefault::Boolean(word == "true")),
                    other => Err(self.err(line, format!("boolean default for non-boolean type {other:?}"))),
                },
                _ => Err(self.err(
                    line,
                    format!("unsupported default literal '{word}' (dictionary/object defaults are not supported)"),
                )),
            },
            Some((_, Token::LBrace)) => Err(self.err(
                line,
                "dictionary default values are unsupported in generated IDL inputs",
            )),
            Some((at, other)) => Err(self.err(at, format!("unsupported default literal {other:?}"))),
            None => Err(self.err(line, "unexpected end of file in parameter default")),
        }
    }

    fn r#type(&mut self) -> Result<(Type, usize), ParseError> {
        let (line, first) = self.next().ok_or_else(|| {
            self.err(self.cur_line(), "unexpected end of file in type position")
        })?;
        let base = match first {
            Token::Ident(ref word) => match word.as_str() {
                "DOMString" => Type::DomString { nullable: false },
                "boolean" => Type::Boolean,
                "unsigned" => {
                    let (line2, second) = self
                        .next()
                        .ok_or_else(|| self.err(line, "unexpected end of file after 'unsigned'"))?;
                    match second {
                        Token::Ident(ref w) if w == "long" => Type::UnsignedLong,
                        Token::Ident(ref w) if w == "short" => Type::UnsignedShort,
                        other => {
                            return Err(self.err(
                                line2,
                                format!("unsupported numeric type 'unsigned ...' (found {other:?})"),
                            ))
                        }
                    }
                }
                "float" => Type::Float,
                "double" => Type::Double,
                "undefined" => Type::Undefined,
                "any" => return Err(self.err(line, "'any' is unsupported in generated IDL inputs")),
                "ArrayBuffer" | "BufferSource" | "ArrayBufferView" => {
                    return Err(self.err(line, format!("'{word}' is unsupported in generated IDL inputs")))
                }
                "Promise" => {
                    return Err(self.err(line, "'Promise' return types are unsupported in generated IDL inputs"))
                }
                "optional" => {
                    return Err(self.err(line, "malformed parameter: missing type before 'optional' handling"))
                }
                other => Type::Interface {
                    name: other.to_owned(),
                    nullable: false,
                },
            },
            Token::LParen => {
                return Err(self.err(line, format!("union types are unsupported in generated IDL inputs")));
            }
            other => {
                return Err(self.err(line, format!("unexpected token in type position: {other:?}")));
            }
        };
        let nullable = if self.peek() == Some(&Token::Question) {
            self.next();
            true
        } else {
            false
        };
        let ty = match (base, nullable) {
            (Type::DomString { .. }, nullable) => Type::DomString { nullable },
            (Type::Interface { name, .. }, nullable) => Type::Interface { name, nullable },
            (other, false) => other,
            (other, true) => {
                return Err(self.err(line, format!("nullable primitive type {other:?} is unsupported")));
            }
        };
        Ok((ty, line))
    }

    /// Extended attribute list. Only the policy-approved attributes are
    /// accepted; anything else is an explicit generation failure.
    fn attr_list(&mut self) -> Result<Attrs, ParseError> {
        let mut attrs = Attrs::default();
        loop {
            let (line, name_token) = self.next().ok_or_else(|| {
                self.err(self.cur_line(), "unexpected end of file in extended attribute list")
            })?;
            match name_token {
                Token::RBrack => break,
                Token::Ident(ref name) => {
                    let value = if self.peek() == Some(&Token::Eq) {
                        self.next();
                        self.attr_value(name, line)?
                    } else {
                        None
                    };
                    let allowed = matches!(
                        name.as_str(),
                        "Exposed"
                            | "Pure"
                            | "Throws"
                            | "NewObject"
                            | "CEReactions"
                            | "LegacyNullToEmptyString"
                            | "Abstract"
                    );
                    if !allowed {
                        return Err(self.err(
                            line,
                            format!("unsupported extended attribute '[{name}]' in generated IDL inputs"),
                        ));
                    }
                    if name == "Exposed" {
                        let value = value.unwrap_or_else(|| "required".to_owned());
                        if value != "Window" {
                            return Err(self.err(
                                line,
                                format!("exposure '{value}' is unsupported; only [Exposed=Window] is admitted"),
                            ));
                        }
                        attrs.exposed.push("Window".to_owned());
                    } else if value.is_some() {
                        return Err(self.err(
                            line,
                            format!("extended attribute '[{name}]' does not take a value"),
                        ));
                    } else {
                        attrs.names.push(name.clone());
                    }
                }
                other => {
                    return Err(self.err(
                        line,
                        format!("unsupported token in extended attribute list: {other:?}"),
                    ));
                }
            }
            if self.peek() == Some(&Token::Comma) {
                self.next();
            }
        }
        Ok(attrs)
    }

    fn attr_value(&mut self, name: &str, line: usize) -> Result<Option<String>, ParseError> {
        if name != "Exposed" {
            return Err(self.err(line, format!("extended attribute '[{name}]' does not take a value")));
        }
        let first = match self.peek().cloned() {
            Some(Token::Ident(ref word)) => {
                self.next();
                word.clone()
            }
            Some(Token::LParen) => {
                self.next();
                let mut first = None;
                let mut entries = Vec::new();
                loop {
                    let (line2, entry) = self
                        .next()
                        .ok_or_else(|| self.err(line, "unterminated exposure list"))?;
                    match entry {
                        Token::Ident(ref word) => {
                            entries.push(word.clone());
                            if first.is_none() {
                                first = Some(word.clone());
                            }
                        }
                        Token::RParen => break,
                        other => {
                            return Err(self.err(line2, format!("unsupported exposure entry {other:?}")))
                        }
                    }
                    if self.peek() == Some(&Token::Comma) {
                        self.next();
                    }
                }
                // Validate every exposure entry (only Window is admitted).
                for entry in &entries {
                    if entry != "Window" {
                        return Err(self.err(
                            line,
                            format!("exposure '{entry}' is unsupported; only [Exposed=Window] is admitted"),
                        ));
                    }
                }
                let value = first.ok_or_else(|| self.err(line, "empty exposure list"))?;
                value
            }
            _ => return Err(self.err(line, "expected exposure value after '='")),
        };
        Ok(Some(first))
    }
}

/// Parse one IDL file into its interface declarations.
pub fn parse_file(path: &str, source: &str) -> Result<Vec<Interface>, ParseError> {
    Parser {
        file: path,
        tokens: tokenize(path, source)?,
        pos: 0,
        interfaces: Vec::new(),
    }
    .parse()
}
