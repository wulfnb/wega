//! WebIDL AST for the construct subset the generation pipeline supports.
//! Anything outside this subset is a positioned parser error, not a silent
//! skip: the pipeline must fail generation explicitly for unsupported syntax.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    DomString { nullable: bool },
    Boolean,
    UnsignedLong,
    UnsignedShort,
    Float,
    Double,
    Undefined,
    Interface { name: String, nullable: bool },
    /// A union type flattened to its last-primitive fallback (e.g. `(DOMString or Options)` → DomString).
    UnionFallback { spelling: String },
    /// An enum type used as a value in a type position (e.g. `enum DocumentReadyState`).
    Enum { name: String, nullable: bool },
}

impl Type {
    /// Stable IDL spelling for hook manifests and conversion selection.
    pub fn spelling(&self) -> String {
        match self {
            Type::DomString { nullable } => {
                if *nullable { "DOMString?".into() } else { "DOMString".into() }
            }
            Type::Boolean => "boolean".into(),
            Type::UnsignedLong => "unsigned long".into(),
            Type::UnsignedShort => "unsigned short".into(),
            Type::Float => "float".into(),
            Type::Double => "double".into(),
            Type::Undefined => "undefined".into(),
            Type::Interface { name, nullable } => {
                if *nullable { format!("{name}?") } else { name.clone() }
            }
            Type::UnionFallback { spelling } => spelling.clone(),
            Type::Enum { name, nullable } => {
                if *nullable { format!("{name}?") } else { name.clone() }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentDefault {
    String(String),
    Number(i64),
    Boolean(bool),
}

impl ArgumentDefault {
    /// JavaScript literal for the emitted conversion.
    pub fn js_literal(&self) -> String {
        match self {
            ArgumentDefault::String(text) => format!("{:?}", text),
            ArgumentDefault::Number(value) => value.to_string(),
            ArgumentDefault::Boolean(value) => value.to_string(),
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

/// A resolved constant value from IDL (must be one of these types).
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
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationMember {
    pub name: String,
    pub ty: Type,
    pub params: Vec<Parameter>,
    pub throws: bool,
    pub new_object: bool,
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
}

impl Member {
    pub fn name(&self) -> &str {
        match self {
            Member::Attribute(member) => &member.name,
            Member::Operation(member) => &member.name,
            Member::Constructor(_) => "constructor",
            Member::Const(member) => &member.name,
            Member::Enum(member) => &member.name,
        }
    }
    pub fn line(&self) -> usize {
        match self {
            Member::Attribute(member) => member.line,
            Member::Operation(member) => member.line,
            Member::Constructor(member) => member.line,
            Member::Const(member) => member.line,
            Member::Enum(member) => member.line,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    pub base: Option<String>,
    pub exposed: Vec<String>,
    pub abstract_: bool,
    pub members: Vec<Member>,
    /// `includes` clauses. Members must be provided by the family's native
    /// base; the generator never implements mixin members itself.
    pub includes: Vec<String>,
    pub file: String,
    pub line: usize,
}

impl Interface {
    pub fn has_constructor(&self) -> bool {
        self.members
            .iter()
            .any(|member| matches!(member, Member::Constructor(_)))
    }
}
