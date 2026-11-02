//! WebIDL subset parser. Every construct outside the supported policy produces
//! a positioned error; the pipeline never guesses semantics for syntax it does
//! not understand.

use crate::ast::{
    ArgumentDefault, AttributeMember, ConstMember, ConstValue, ConstructorMember, Definitions,
    Dictionary, EnumMember, Interface, Member, OperationMember, Parameter, Type,
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
                    return Err(ParseError::new(
                        file,
                        start_line,
                        "unterminated block comment",
                    ));
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
                // Check for hex literal: 0x or 0X followed by hex digits
                if t.src.get(t.pos + 1) == Some(&b'x') || t.src.get(t.pos + 1) == Some(&b'X') {
                    t.pos += 2;
                    let hex_start = t.pos;
                    while t.pos < t.src.len() && t.src[t.pos].is_ascii_hexdigit() {
                        t.pos += 1;
                    }
                    if t.pos == hex_start {
                        return Err(ParseError::new(file, line, "expected hex digit after '0x'"));
                    }
                    let text = std::str::from_utf8(&t.src[hex_start..t.pos]).unwrap();
                    let value = i64::from_str_radix(text, 16)
                        .map_err(|_| ParseError::new(file, line, "hex literal out of range"))?;
                    Token::Int(value)
                } else {
                    // Decimal integer
                    while t.pos < t.src.len() && t.src[t.pos].is_ascii_digit() {
                        t.pos += 1;
                    }
                    let text = std::str::from_utf8(&t.src[start..t.pos]).unwrap();
                    let value: i64 = text
                        .parse()
                        .map_err(|_| ParseError::new(file, line, "numeric literal out of range"))?;
                    Token::Int(value)
                }
            }
            _ if is_ident_start(b) => {
                let start = t.pos;
                while t.pos < t.src.len() && is_ident_cont(t.src[t.pos]) {
                    t.pos += 1;
                }
                Token::Ident(
                    std::str::from_utf8(&t.src[start..t.pos])
                        .unwrap()
                        .to_owned(),
                )
            }
            other => {
                let ch = other as char;
                return Err(ParseError::new(
                    file,
                    line,
                    format!("unsupported character '{ch}'"),
                ));
            }
        };
        out.push((line, token));
    }
    Ok(out)
}

#[derive(Default)]
struct Attrs {
    names: Vec<String>,
    exposed: Vec<String>,
    forwards: Option<String>,
}
impl Attrs {
    fn has(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
    }
}
struct Parser<'a> {
    file: &'a str,
    tokens: Vec<(usize, Token)>,
    pos: usize,
    definitions: Definitions,
}
impl Parser<'_> {
    fn err(&self, message: impl Into<String>) -> ParseError {
        ParseError::new(self.file, self.line(), message)
    }
    fn line(&self) -> usize {
        self.tokens
            .get(self.pos)
            .or_else(|| self.tokens.last())
            .map_or(1, |t| t.0)
    }
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.1)
    }
    fn next(&mut self) -> Result<Token, ParseError> {
        let t = self
            .peek()
            .cloned()
            .ok_or_else(|| self.err("unexpected end of file"))?;
        self.pos += 1;
        Ok(t)
    }
    fn eat(&mut self, t: &Token) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn word(&mut self, s: &str) -> bool {
        self.eat(&Token::Ident(s.into()))
    }
    fn expect(&mut self, t: Token) -> Result<(), ParseError> {
        if self.eat(&t) {
            Ok(())
        } else {
            Err(self.err(format!("expected {t:?}, found {:?}", self.peek())))
        }
    }
    fn ident(&mut self) -> Result<String, ParseError> {
        match self.next()? {
            Token::Ident(s) => Ok(s),
            t => Err(self.err(format!("expected identifier, found {t:?}"))),
        }
    }
    fn attrs(&mut self) -> Result<Attrs, ParseError> {
        let mut attrs = Attrs::default();
        if !self.eat(&Token::LBrack) {
            return Ok(attrs);
        }
        loop {
            let name = self.ident()?;
            if name == "Exposed" {
                self.expect(Token::Eq)?;
                let list = self.eat(&Token::LParen);
                loop {
                    let realm = self.ident()?;
                    if realm != "Window" {
                        return Err(self.err(format!("unsupported exposure {realm}")));
                    }
                    attrs.exposed.push(realm);
                    if !list || !self.eat(&Token::Comma) {
                        break;
                    }
                }
                if list {
                    self.expect(Token::RParen)?;
                }
            } else if name == "PutForwards" {
                self.expect(Token::Eq)?;
                attrs.forwards = Some(self.ident()?);
            } else if matches!(
                name.as_str(),
                "Pure"
                    | "Throws"
                    | "SetterThrows"
                    | "NewObject"
                    | "CEReactions"
                    | "LegacyNullToEmptyString"
                    | "Abstract"
                    | "Constant"
                    | "SameObject"
            ) {
                attrs.names.push(name);
            } else {
                return Err(self.err(format!("unsupported extended attribute [{name}]")));
            }
            if self.eat(&Token::RBrack) {
                break;
            }
            self.expect(Token::Comma)?;
        }
        Ok(attrs)
    }
    fn ty(&mut self) -> Result<Type, ParseError> {
        let mut ty = if self.eat(&Token::LParen) {
            let mut types = vec![self.ty()?];
            while self.word("or") {
                types.push(self.ty()?);
            }
            self.expect(Token::RParen)?;
            if types.len() < 2 {
                return Err(self.err("union needs at least two alternatives"));
            }
            Type::Union(types)
        } else {
            match self.ident()?.as_str() {
                "DOMString" | "USVString" | "ByteString" => Type::DomString { nullable: false },
                "boolean" => Type::Boolean,
                "unsigned" => match self.ident()?.as_str() {
                    "long" => Type::UnsignedLong,
                    "short" => Type::UnsignedShort,
                    name => return Err(self.err(format!("unsupported unsigned type {name}"))),
                },
                "float" => Type::Float,
                "double" | "DOMHighResTimeStamp" => Type::Double,
                "undefined" | "void" => Type::Undefined,
                "any" => Type::Any,
                "object" => Type::Object,
                name @ ("sequence" | "FrozenArray" | "Promise") => {
                    self.expect(Token::Less)?;
                    let inner = Box::new(self.ty()?);
                    self.expect(Token::Greater)?;
                    if name == "Promise" {
                        Type::Promise(inner)
                    } else {
                        Type::Sequence(inner)
                    }
                }
                name => Type::Interface {
                    name: name.into(),
                    nullable: false,
                },
            }
        };
        if self.eat(&Token::Question) {
            match &mut ty {
                Type::DomString { nullable } | Type::Interface { nullable, .. } => *nullable = true,
                _ => return Err(self.err("nullable form is unsupported for this type")),
            }
        }
        Ok(ty)
    }
    fn default(&mut self) -> Result<ArgumentDefault, ParseError> {
        match self.next()? {
            Token::Str(s) => Ok(ArgumentDefault::String(s)),
            Token::Int(v) => Ok(ArgumentDefault::Number(v)),
            Token::Ident(s) if s == "true" || s == "false" => {
                Ok(ArgumentDefault::Boolean(s == "true"))
            }
            Token::Ident(s) if s == "null" => Ok(ArgumentDefault::Null),
            Token::LBrace => {
                self.expect(Token::RBrace)?;
                Ok(ArgumentDefault::Dictionary)
            }
            Token::LBrack => {
                self.expect(Token::RBrack)?;
                Ok(ArgumentDefault::Sequence)
            }
            t => Err(self.err(format!("unsupported default {t:?}"))),
        }
    }
    fn parameters(&mut self) -> Result<Vec<Parameter>, ParseError> {
        let mut out = Vec::new();
        let mut saw_optional = false;
        if self.eat(&Token::RParen) {
            return Ok(out);
        }
        loop {
            let line = self.line();
            let optional = self.word("optional");
            let ty = self.ty()?;
            let variadic = self.eat(&Token::Dots);
            let name = self.ident()?;
            let default = if self.eat(&Token::Eq) {
                Some(self.default()?)
            } else {
                None
            };
            if (variadic && (optional || default.is_some())) || (!optional && default.is_some()) {
                return Err(self.err("invalid optional/variadic/default combination"));
            }
            if saw_optional && !optional && !variadic {
                return Err(self.err("required argument after optional argument"));
            }
            saw_optional |= optional;
            out.push(Parameter {
                name,
                ty,
                optional,
                default,
                variadic,
                line,
            });
            if self.eat(&Token::RParen) {
                break;
            }
            if variadic {
                return Err(self.err("variadic argument must be last"));
            }
            self.expect(Token::Comma)?;
        }
        Ok(out)
    }
    fn enum_values(&mut self) -> Result<Vec<String>, ParseError> {
        self.expect(Token::LBrace)?;
        let mut values = Vec::new();
        while !self.eat(&Token::RBrace) {
            match self.next()? {
                Token::Str(s) => values.push(s),
                t => return Err(self.err(format!("expected enum string, found {t:?}"))),
            }
            if !self.eat(&Token::Comma) {
                self.expect(Token::RBrace)?;
                break;
            }
        }
        self.expect(Token::Semi)?;
        Ok(values)
    }
    fn member(&mut self) -> Result<Member, ParseError> {
        let line = self.line();
        let mut attrs = self.attrs()?;
        if self.word("const") {
            let ty = self.ty()?;
            let name = self.ident()?;
            self.expect(Token::Eq)?;
            let value = match self.default()? {
                ArgumentDefault::Number(v)
                    if matches!(ty, Type::UnsignedLong | Type::UnsignedShort) && v >= 0 =>
                {
                    ConstValue::Number(v)
                }
                ArgumentDefault::Boolean(v) if matches!(ty, Type::Boolean) => {
                    ConstValue::Boolean(v)
                }
                ArgumentDefault::String(v) if matches!(ty, Type::DomString { .. }) => {
                    ConstValue::String(v)
                }
                _ => return Err(self.err("constant value does not match its type")),
            };
            self.expect(Token::Semi)?;
            return Ok(Member::Const(ConstMember {
                name,
                ty,
                value,
                line,
            }));
        }
        if self.word("enum") {
            let name = self.ident()?;
            return Ok(Member::Enum(EnumMember {
                name,
                values: self.enum_values()?,
                line,
            }));
        }
        if self.word("iterable") {
            self.expect(Token::Less)?;
            let mut types = vec![self.ty()?];
            if self.eat(&Token::Comma) {
                types.push(self.ty()?);
            }
            self.expect(Token::Greater)?;
            self.expect(Token::Semi)?;
            return Ok(Member::Iterable(types, line));
        }
        if self.word("constructor") {
            self.expect(Token::LParen)?;
            let params = self.parameters()?;
            self.expect(Token::Semi)?;
            return Ok(Member::Constructor(ConstructorMember {
                params,
                throws: attrs.has("Throws"),
                line,
            }));
        }
        let stringifier = self.word("stringifier");
        if stringifier && self.eat(&Token::Semi) {
            return Ok(Member::Operation(OperationMember {
                name: "toString".into(),
                ty: Type::DomString { nullable: false },
                params: vec![],
                throws: false,
                new_object: false,
                same_object: false,
                static_: false,
                getter: false,
                line,
            }));
        }
        let static_ = self.word("static");
        let getter = self.word("getter");
        if self.word("setter") || self.word("deleter") {
            return Err(self.err("special setters/deleters require a collection adapter"));
        }
        let readonly = self.word("readonly");
        if readonly || self.word("attribute") {
            if readonly && !self.word("attribute") {
                return Err(self.err("expected attribute after readonly"));
            }
            let extra = self.attrs()?;
            attrs.names.extend(extra.names);
            let ty = self.ty()?;
            let name = self.ident()?;
            self.expect(Token::Semi)?;
            return Ok(Member::Attribute(AttributeMember {
                name,
                ty,
                readonly,
                pure: attrs.has("Pure"),
                constant: attrs.has("Constant"),
                throws: attrs.has("Throws"),
                setter_throws: attrs.has("SetterThrows"),
                legacy_null_to_empty: attrs.has("LegacyNullToEmptyString"),
                same_object: attrs.has("SameObject"),
                forwards: attrs.forwards,
                line,
            }));
        }
        let ty = self.ty()?;
        let name = if getter && self.peek() == Some(&Token::LParen) {
            "__getter".into()
        } else {
            self.ident()?
        };
        self.expect(Token::LParen)?;
        let params = self.parameters()?;
        self.expect(Token::Semi)?;
        Ok(Member::Operation(OperationMember {
            name,
            ty,
            params,
            throws: attrs.has("Throws"),
            new_object: attrs.has("NewObject"),
            same_object: attrs.has("SameObject"),
            static_,
            getter,
            line,
        }))
    }
    fn parse(mut self) -> Result<Definitions, ParseError> {
        while self.peek().is_some() {
            let line = self.line();
            let attrs = self.attrs()?;
            let partial = self.word("partial");
            if self.word("interface") {
                let mixin = self.word("mixin");
                let name = self.ident()?;
                let base = if self.eat(&Token::Colon) {
                    Some(self.ident()?)
                } else {
                    None
                };
                if !mixin && !partial && attrs.exposed.is_empty() {
                    return Err(self.err("interface lacks [Exposed=Window]"));
                }
                self.expect(Token::LBrace)?;
                let mut members = vec![];
                while !self.eat(&Token::RBrace) {
                    members.push(self.member()?);
                }
                self.expect(Token::Semi)?;
                self.definitions.interfaces.push(Interface {
                    name,
                    base,
                    exposed: attrs.exposed.clone(),
                    abstract_: attrs.has("Abstract"),
                    mixin,
                    partial,
                    members,
                    includes: vec![],
                    file: self.file.into(),
                    line,
                });
            } else if self.word("dictionary") {
                let name = self.ident()?;
                let base = if self.eat(&Token::Colon) {
                    Some(self.ident()?)
                } else {
                    None
                };
                self.expect(Token::LBrace)?;
                let mut members = vec![];
                while !self.eat(&Token::RBrace) {
                    let line = self.line();
                    let required = self.word("required");
                    let ty = self.ty()?;
                    let name = self.ident()?;
                    let default = if self.eat(&Token::Eq) {
                        Some(self.default()?)
                    } else {
                        None
                    };
                    if required && default.is_some() {
                        return Err(self.err("required dictionary member cannot have a default"));
                    }
                    self.expect(Token::Semi)?;
                    if members.iter().any(|m: &Parameter| m.name == name) {
                        return Err(self.err("duplicate dictionary member"));
                    }
                    members.push(Parameter {
                        name,
                        ty,
                        optional: !required,
                        default,
                        variadic: false,
                        line,
                    });
                }
                self.expect(Token::Semi)?;
                members.sort_by(|a, b| a.name.cmp(&b.name));
                if self
                    .definitions
                    .dictionaries
                    .insert(name, Dictionary { base, members })
                    .is_some()
                {
                    return Err(self.err("duplicate dictionary"));
                }
            } else if self.word("typedef") {
                let ty = self.ty()?;
                let name = self.ident()?;
                self.expect(Token::Semi)?;
                if self.definitions.typedefs.insert(name, ty).is_some() {
                    return Err(self.err("duplicate typedef"));
                }
            } else if self.word("enum") {
                let name = self.ident()?;
                let values = self.enum_values()?;
                if self.definitions.enums.insert(name, values).is_some() {
                    return Err(self.err("duplicate enum"));
                }
            } else if self.word("callback") {
                let interface = self.word("interface");
                let name = self.ident()?;
                let (ty, params) = if interface {
                    self.expect(Token::LBrace)?;
                    let Member::Operation(operation) = self.member()? else {
                        return Err(self.err("callback interface requires one operation"));
                    };
                    self.expect(Token::RBrace)?;
                    (operation.ty, operation.params)
                } else {
                    self.expect(Token::Eq)?;
                    let ty = self.ty()?;
                    self.expect(Token::LParen)?;
                    (ty, self.parameters()?)
                };
                self.expect(Token::Semi)?;
                if self
                    .definitions
                    .callbacks
                    .insert(name, (ty, params, !interface))
                    .is_some()
                {
                    return Err(self.err("duplicate callback"));
                }
            } else {
                let owner = self.ident()?;
                if !self.word("includes") {
                    return Err(self.err("unsupported declaration (expected interface, dictionary, enum, callback or typedef)"));
                }
                let mixin = self.ident()?;
                self.expect(Token::Semi)?;
                // Keep includes with the interface even when a partial occurs between them.
                let Some(interface) = self
                    .definitions
                    .interfaces
                    .iter_mut()
                    .find(|i| i.name == owner)
                else {
                    return Err(self.err("includes has no preceding interface"));
                };
                interface.includes.push(mixin);
            }
        }
        Ok(self.definitions)
    }
}
pub fn parse_definitions(path: &str, source: &str) -> Result<Definitions, ParseError> {
    Parser {
        file: path,
        tokens: tokenize(path, source)?,
        pos: 0,
        definitions: Definitions::default(),
    }
    .parse()
}
