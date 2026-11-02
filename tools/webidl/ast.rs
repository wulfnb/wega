//! Typed WebIDL definitions consumed by the deterministic binding compiler.
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    DomString { nullable: bool },
    Boolean,
    UnsignedLong,
    UnsignedShort,
    Float,
    Double,
    Undefined,
    Any,
    Object,
    Interface { name: String, nullable: bool },
    Sequence(Box<Type>),
    Promise(Box<Type>),
    Union(Vec<Type>),
}
impl Type {
    pub fn spelling(&self) -> String {
        match self {
            Self::DomString { nullable } => {
                format!("DOMString{}", if *nullable { "?" } else { "" })
            }
            Self::Boolean => "boolean".into(),
            Self::UnsignedLong => "unsigned long".into(),
            Self::UnsignedShort => "unsigned short".into(),
            Self::Float => "float".into(),
            Self::Double => "double".into(),
            Self::Undefined => "undefined".into(),
            Self::Any => "any".into(),
            Self::Object => "object".into(),
            Self::Interface { name, nullable } => {
                format!("{name}{}", if *nullable { "?" } else { "" })
            }
            Self::Sequence(t) => format!("sequence<{}>", t.spelling()),
            Self::Promise(t) => format!("Promise<{}>", t.spelling()),
            Self::Union(types) => format!(
                "({})",
                types
                    .iter()
                    .map(Type::spelling)
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentDefault {
    String(String),
    Number(i64),
    Boolean(bool),
    Null,
    Dictionary,
    Sequence,
}
impl ArgumentDefault {
    pub fn js_literal(&self) -> String {
        match self {
            Self::String(v) => serde_json::to_string(v).unwrap(),
            Self::Number(v) => v.to_string(),
            Self::Boolean(v) => v.to_string(),
            Self::Null => "null".into(),
            Self::Dictionary => "{}".into(),
            Self::Sequence => "[]".into(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
    pub optional: bool,
    pub default: Option<ArgumentDefault>,
    pub variadic: bool,
    pub line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstMember {
    pub name: String,
    pub ty: Type,
    pub value: ConstValue,
    pub line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstValue {
    Number(i64),
    Boolean(bool),
    String(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeMember {
    pub name: String,
    pub ty: Type,
    pub readonly: bool,
    pub pure: bool,
    pub constant: bool,
    pub throws: bool,
    pub setter_throws: bool,
    pub legacy_null_to_empty: bool,
    pub same_object: bool,
    pub forwards: Option<String>,
    pub line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationMember {
    pub name: String,
    pub ty: Type,
    pub params: Vec<Parameter>,
    pub throws: bool,
    pub new_object: bool,
    pub same_object: bool,
    pub static_: bool,
    pub getter: bool,
    pub line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructorMember {
    pub params: Vec<Parameter>,
    pub throws: bool,
    pub line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumMember {
    pub name: String,
    pub values: Vec<String>,
    pub line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Member {
    Attribute(AttributeMember),
    Operation(OperationMember),
    Constructor(ConstructorMember),
    Const(ConstMember),
    Enum(EnumMember),
    Iterable(Vec<Type>, usize),
}
impl Member {
    pub fn name(&self) -> &str {
        match self {
            Self::Attribute(m) => &m.name,
            Self::Operation(m) => &m.name,
            Self::Constructor(_) => "constructor",
            Self::Const(m) => &m.name,
            Self::Enum(m) => &m.name,
            Self::Iterable(..) => "@@iterator",
        }
    }
    pub fn line(&self) -> usize {
        match self {
            Self::Attribute(m) => m.line,
            Self::Operation(m) => m.line,
            Self::Constructor(m) => m.line,
            Self::Const(m) => m.line,
            Self::Enum(m) => m.line,
            Self::Iterable(_, line) => *line,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    pub base: Option<String>,
    pub exposed: Vec<String>,
    pub abstract_: bool,
    pub mixin: bool,
    pub partial: bool,
    pub members: Vec<Member>,
    pub includes: Vec<String>,
    pub file: String,
    pub line: usize,
}
#[derive(Debug, Clone)]
pub struct Dictionary {
    pub base: Option<String>,
    pub members: Vec<Parameter>,
}
#[derive(Debug, Default)]
pub struct Definitions {
    pub interfaces: Vec<Interface>,
    pub dictionaries: BTreeMap<String, Dictionary>,
    pub enums: BTreeMap<String, Vec<String>>,
    pub typedefs: BTreeMap<String, Type>,
    pub callbacks: BTreeMap<String, (Type, Vec<Parameter>, bool)>,
}
