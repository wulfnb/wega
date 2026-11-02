//! Deterministic binding generation for one pinned IDL family.
//!
//! The generator emits two artifacts per family: the JS binding file and the
//! semantic-hook manifest. Both are byte-stable for a given input set; the
//! checked-in artifacts are regenerated in CI and compared.

use crate::ast::{AttributeMember, ConstValue, ConstructorMember, Interface, Member, Parameter, Type};
use crate::manifest::{Family, HookAnnotation, IdlPin};
use crate::parser::parse_file;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct HookSpec {
    pub name: String,
    pub interface: String,
    pub member: String,
    pub kind: &'static str,
    pub pure: bool,
    pub throws: bool,
    pub new_object: bool,
    pub args: Vec<String>,
    pub result: String,
    pub reaction: Option<String>,
    pub line: usize,
}

impl HookSpec {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "interface": self.interface,
            "member": self.member,
            "kind": self.kind,
            "pure": self.pure,
            "throws": self.throws,
            "new_object": self.new_object,
            "args": self.args,
            "result": self.result,
            "reaction": self.reaction,
            "line": self.line,
        })
    }
}

pub struct FamilyModel {
    pub family: Family,
    pub interfaces: Vec<Interface>,
    pub hooks: Vec<HookSpec>,
    pub pins: Vec<(String, String)>,
    /// Conversion helpers the emitter must define, in fixed output order.
    pub conversions: Vec<&'static str>,
}

impl FamilyModel {
    fn hook(&self, interface: &str, member: &str, setter: bool) -> Option<&HookSpec> {
        let name = if setter {
            format!("{}.{}.set", self.family.hook_prefix, member)
        } else {
            hook_name(&self.family.hook_prefix, member)
        };
        self.find_hook(&name, interface)
    }
    fn constructor_hook(&self, interface: &str) -> Option<&HookSpec> {
        self.find_hook(&constructor_hook_name(&self.family.hook_prefix, interface), interface)
    }
    fn find_hook(&self, name: &str, interface: &str) -> Option<&HookSpec> {
        self.hooks
            .iter()
            .find(|hook| hook.name == name && hook.interface == interface)
    }
    fn base_map(&self) -> BTreeMap<&str, &Interface> {
        self.interfaces
            .iter()
            .map(|i| (i.name.as_str(), i))
            .collect()
    }
    fn is_generated_base(&self, base: &str) -> bool {
        self.base_map().contains_key(base)
    }
}

fn fail(message: impl Into<String>) -> String {
    message.into()
}

fn annotation<'a>(family: &'a Family, name: &str) -> Result<&'a HookAnnotation, String> {
    family
        .hooks
        .get(name)
        .ok_or_else(|| {
            format!(
                "hook '{name}' is missing its semantic annotation in tools/webidl/families.json"
            )
        })
}

fn hook_name(prefix: &str, member: &str) -> String {
    format!("{prefix}.{member}")
}

fn constructor_hook_name(prefix: &str, interface: &str) -> String {
    format!("{prefix}.construct.{}", interface.to_lowercase())
}

/// Parse, validate and order one family.
pub fn build_family(
    family: &Family,
    pins: &BTreeMap<String, IdlPin>,
    sources: &[(String, String)],
) -> Result<FamilyModel, String> {
    let mut interfaces: Vec<Interface> = Vec::new();
    for (path, source) in sources {
        for interface in parse_file(path, source).map_err(|e| e.to_string())? {
            if interfaces.iter().any(|existing| existing.name == interface.name) {
                return Err(fail(format!(
                    "{path}: interface '{}' is declared more than once in family '{}'",
                    interface.name, family.name
                )));
            }
            interfaces.push(interface);
        }
    }
    if interfaces.is_empty() {
        return Err(fail(format!("family '{}' declares no interfaces", family.name)));
    }
    let mut names: BTreeSet<&str> = BTreeSet::new();
    for interface in &interfaces {
        names.insert(interface.name.as_str());
    }
    for interface in &interfaces {
        if interface.exposed != vec!["Window".to_owned()] {
            return Err(fail(format!(
                "{}:{}: interface '{}' has unsupported exposure; only [Exposed=Window] is admitted",
                interface.file, interface.line, interface.name
            )));
        }
        if let Some(base) = &interface.base {
            let known = names.contains(base.as_str()) || family.native_base.contains_key(base);
            if !known {
                return Err(fail(format!(
                    "{}:{}: interface '{}' extends unknown interface '{base}' (add it to the family or native_base)",
                    interface.file, interface.line, interface.name
                )));
            }
        }
        for mixin in &interface.includes {
            if !family.base_mixins.iter().any(|m| m == mixin) {
                return Err(fail(format!(
                    "{}:{}: mixin '{mixin}' included by '{}' is not in the family's base-provided mixin registry",
                    interface.file, interface.line, interface.name
                )));
            }
        }
        let mut member_names: BTreeSet<&str> = BTreeSet::new();
        for member in &interface.members {
            if !member_names.insert(member.name()) {
                return Err(fail(format!(
                    "{}:{}: interface '{}' declares member '{}' twice",
                    interface.file, interface.line, interface.name, member.name()
                )));
            }
        }
        if interface.abstract_ {
            if family.globals.iter().any(|g| g == &interface.name) {
                return Err(fail(format!(
                    "family '{}': abstract interface '{}' must not be listed in globals",
                    family.name, interface.name
                )));
            }
        } else {
            if !family.globals.iter().any(|g| g == &interface.name) {
                return Err(fail(format!(
                    "family '{}': exposed interface '{}' must be listed in globals",
                    family.name, interface.name
                )));
            }
            if !interface.has_constructor() {
                return Err(fail(format!(
                    "family '{}': exposed interface '{}' needs a constructor",
                    family.name, interface.name
                )));
            }
        }
    }
    for global in &family.globals {
        if !names.contains(global.as_str()) {
            return Err(fail(format!(
                "family '{}': globals entry '{global}' is not a family interface",
                family.name
            )));
        }
    }

    // Deterministic topological order: base interfaces before dependents.
    let mut ordered: Vec<Interface> = Vec::new();
    let mut remaining = interfaces.clone();
    while !remaining.is_empty() {
        let mut progress = false;
        remaining.retain(|interface| {
            let ready = match &interface.base {
                None => true,
                Some(base) => family.native_base.contains_key(base)
                    || ordered.iter().any(|done| done.name == *base),
            };
            if ready {
                ordered.push(interface.clone());
                progress = true;
                false
            } else {
                true
            }
        });
        if !progress {
            return Err(fail(format!("family '{}': interface inheritance cycle", family.name)));
        }
    }

    // Derive hooks: one per IDL member, stable order, with annotations.
    let mut hooks: Vec<HookSpec> = Vec::new();
    let mut type_usage: BTreeSet<String> = BTreeSet::new();
    for interface in &ordered {
        for member in &interface.members {
            match member {
                Member::Attribute(attr) => {
                    type_usage.insert(type_key(&attr.ty));
                    if attr.legacy_null_to_empty {
                        type_usage.insert("LegacyNullDomString".to_owned());
                    }
                    if attr.readonly {
                        let name = hook_name(&family.hook_prefix, &attr.name);
                        let ann = annotation(family, &name)?;
                        if ann.reaction.is_some() {
                            return Err(fail(format!(
                                "hook '{name}': readonly attribute getters cannot trigger reactions"
                            )));
                        }
                        hooks.push(HookSpec {
                            name,
                            interface: interface.name.clone(),
                            member: attr.name.clone(),
                            kind: "attribute-getter",
                            pure: attr.pure,
                            throws: false,
                            new_object: false,
                            args: vec![attr.ty.spelling()],
                            result: ann.result.clone(),
                            reaction: None,
                            line: attr.line,
                        });
                    } else {
                        let getter = hook_name(&family.hook_prefix, &attr.name);
                        let ann_get = annotation(family, &getter)?;
                        if ann_get.reaction.is_some() {
                            return Err(fail(format!(
                                "hook '{getter}': attribute getters cannot trigger reactions"
                            )));
                        }
                        hooks.push(HookSpec {
                            name: getter.clone(),
                            interface: interface.name.clone(),
                            member: attr.name.clone(),
                            kind: "attribute-getter",
                            pure: attr.pure,
                            throws: false,
                            new_object: false,
                            args: vec![attr.ty.spelling()],
                            result: ann_get.result.clone(),
                            reaction: None,
                            line: attr.line,
                        });
                        let setter = format!("{getter}.set");
                        let ann_set = annotation(family, &setter)?;
                        hooks.push(HookSpec {
                            name: setter,
                            interface: interface.name.clone(),
                            member: attr.name.clone(),
                            kind: "attribute-setter",
                            pure: false,
                            throws: attr.throws,
                            new_object: false,
                            args: vec![attr.ty.spelling()],
                            result: ann_set.result.clone(),
                            reaction: ann_set.reaction.clone(),
                            line: attr.line,
                        });
                    }
                }
                Member::Operation(op) => {
                    for param in &op.params {
                        check_argument_type(&interface.name, &op.name, param)?;
                        type_usage.insert(type_key(&param.ty));
                    }
                    if matches!(op.ty, Type::Interface { .. })
                        && !op.new_object
                    {
                        return Err(fail(format!(
                            "{}: operation '{}' returns an interface without [NewObject]; unsupported in step 07.a",
                            interface.name, op.name
                        )));
                    }
                    let name = hook_name(&family.hook_prefix, &op.name);
                    let ann = annotation(family, &name)?;
                    let reaction = ann.reaction.clone();
                    let returns_value = !matches!(op.ty, Type::Undefined);
                    if reaction.is_some() && returns_value && !op.new_object {
                        return Err(fail(format!(
                            "hook '{name}': reactions require an undefined return (or [NewObject]) in step 07.a"
                        )));
                    }
                    hooks.push(HookSpec {
                        name,
                        interface: interface.name.clone(),
                        member: op.name.clone(),
                        kind: "operation",
                        pure: false,
                        throws: op.throws,
                        new_object: op.new_object,
                        args: op
                            .params
                            .iter()
                            .map(|p| p.ty.spelling())
                            .collect(),
                        result: ann.result.clone(),
                        reaction,
                        line: op.line,
                    });
                }
                Member::Constructor(ctor) => {
                    // The internal wrap path reuses the first IDL parameter to
                    // carry the node id, so constructors need at least one
                    // string-typed parameter in the supported subset.
                    let first = ctor.params.first().ok_or_else(|| {
                        fail(format!(
                            "interface '{}': zero-parameter constructors are unsupported in step 07.a (the internal wrap path reuses the first parameter)",
                            interface.name
                        ))
                    })?;
                    if !matches!(first.ty, Type::DomString { .. }) {
                        return Err(fail(format!(
                            "interface '{}': constructor parameter '{}' must be DOMString-typed in step 07.a",
                            interface.name, first.name
                        )));
                    }
                    for param in &ctor.params {
                        check_argument_type(&interface.name, "constructor", param)?;
                        type_usage.insert(type_key(&param.ty));
                    }
                    let name = constructor_hook_name(&family.hook_prefix, &interface.name);
                    let ann = annotation(family, &name)?;
                    if ann.reaction.is_some() {
                        return Err(fail(format!(
                            "hook '{name}': constructors cannot trigger reactions"
                        )));
                    }
                    hooks.push(HookSpec {
                        name,
                        interface: interface.name.clone(),
                        member: "constructor".into(),
                        kind: "constructor",
                        pure: false,
                        throws: ctor.throws,
                        new_object: false,
                        args: ctor
                            .params
                            .iter()
                            .map(|p| p.ty.spelling())
                            .collect(),
                        result: ann.result.clone(),
                        reaction: None,
                        line: ctor.line,
                    });
                }
                Member::Const(_) => {
                    // Const members are emitted as static class properties.
                    // No hook needed — JS reads the constant directly.
                }
                Member::Enum(_) => {
                    // Enum declarations are emitted as static properties on the class.
                    // No hook needed — enum values are plain string literals.
                }
            }
        }
    }

    // Unused annotation entries are a drift signal: fail explicitly.
    let expected: BTreeSet<&str> = hooks.iter().map(|h| h.name.as_str()).collect();
    for declared in family.hooks.keys() {
        if !expected.contains(declared.as_str()) {
            return Err(fail(format!(
                "family '{}': families.json annotates unknown hook '{declared}'",
                family.name
            )));
        }
    }

    let mut conversions: Vec<&'static str> = Vec::new();
    if type_usage.contains("DOMString") {
        conversions.push("toDomString");
    }
    if type_usage.contains("LegacyNullDomString") {
        conversions.push("toLegacyNullDomString");
    }
    if type_usage.contains("DOMString?") {
        conversions.push("toNullableDomString");
    }
    if type_usage.contains("unsigned long") {
        conversions.push("toUnsignedLong");
    }
    if type_usage.contains("unsigned short") {
        conversions.push("toUnsignedShort");
    }
    if type_usage.contains("float") {
        conversions.push("toFloat");
    }
    if type_usage.contains("double") {
        conversions.push("toDouble");
    }
    if type_usage.contains("boolean") {
        conversions.push("toBoolean");
    }

    let pins = sources
        .iter()
        .map(|(path, _)| {
            let pin = pins
                .get(path)
                .ok_or_else(|| fail(format!("missing pin for {path}")))?;
            Ok((path.clone(), pin.sha256.clone()))
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(FamilyModel {
        family: family.clone(),
        interfaces: ordered,
        hooks,
        pins,
        conversions,
    })
}

fn check_argument_type(interface: &str, member: &str, param: &Parameter) -> Result<(), String> {
    match &param.ty {
        Type::Interface { name, .. } => Err(fail(format!(
            "interface '{interface}', member '{member}': interface-typed argument '{name}' is unsupported in step 07.a"
        ))),
        _ => Ok(()),
    }
}

fn type_key(ty: &Type) -> String {
    ty.spelling()
}

fn conversion_for(ty: &Type, legacy_null: bool) -> &'static str {
    if legacy_null {
        return "toLegacyNullDomString";
    }
    match ty {
        Type::DomString { nullable: false } => "toDomString",
        Type::DomString { nullable: true } => "toNullableDomString",
        Type::Boolean => "toBoolean",
        Type::UnsignedLong => "toUnsignedLong",
        Type::UnsignedShort => "toUnsignedShort",
        Type::Float => "toFloat",
        Type::Double => "toDouble",
        Type::Undefined => "toUndefined",
        Type::Interface { .. } => "toInterfaceId",
        Type::UnionFallback { .. } => "toDomString",
        Type::Enum { .. } => "toDomString",
    }
}

fn arg_expression(param: &Parameter) -> String {
    let conversion = conversion_for(&param.ty, false);
    if param.variadic {
        // Variadic: convert each element of the array.
        return format!(
            "Array.isArray({0}) ? {0}.map(v => {1}(v)) : ( {0} !== undefined ? [{1}({0})] : [])",
            param.name, conversion
        );
    }
    match &param.default {
        Some(default) => format!(
            "{} === undefined ? {} : {}({})",
            param.name,
            default.js_literal(),
            conversion,
            param.name
        ),
        None => format!("{conversion}({})", param.name),
    }
}

const CONVERSION_SOURCES: [(&str, &str); 8] = [
    (
        "toDomString",
        "  function toDomString(value) {\n    return String(value);\n  }\n",
    ),
    (
        "toLegacyNullDomString",
        "  function toLegacyNullDomString(value) {\n    return value === null ? \"\" : String(value);\n  }\n",
    ),
    (
        "toNullableDomString",
        "  function toNullableDomString(value) {\n    return value === null ? null : String(value);\n  }\n",
    ),
    (
        "toUnsignedLong",
        "  function toUnsignedLong(value) {\n    let number = Number(value);\n    if (Number.isNaN(number)) number = 0;\n    number = Math.trunc(number);\n    return ((number % 4294967296) + 4294967296) % 4294967296;\n  }\n",
    ),
    (
        "toUnsignedShort",
        "  function toUnsignedShort(value) {\n    let number = Number(value);\n    if (Number.isNaN(number)) number = 0;\n    number = Math.trunc(number);\n    return ((number % 65536) + 65536) % 65536;\n  }\n",
    ),
    (
        "toFloat",
        "  function toFloat(value) {\n    const number = Number(value);\n    return Number.isNaN(number) || !Number.isFinite(number) ? 0 : number;\n  }\n",
    ),
    (
        "toDouble",
        "  function toDouble(value) {\n    const number = Number(value);\n    return Number.isNaN(number) ? 0 : number;\n  }\n",
    ),
    (
        "toBoolean",
        "  function toBoolean(value) {\n    return Boolean(value);\n  }\n",
    ),
];

/// Emit the checked-in JS binding file for the family.
pub fn emit_js(model: &FamilyModel) -> String {
    let family = &model.family;
    let mut out = String::new();
    out.push_str("// ---------------------------------------------------------------------------\n");
    out.push_str(&format!(
        "// wega-webidl generated bindings - family \"{}\"\n",
        family.name
    ));
    out.push_str("// Do not edit by hand. Regenerate with:\n");
    out.push_str(
        "//   cargo run --locked --no-default-features --features webidl-tools --bin wega-webidl -- generate\n",
    );
    out.push_str("// Pinned inputs (tools/webidl/idls.json):\n");
    for (path, digest) in &model.pins {
        out.push_str(&format!("//   {path} sha256={digest}\n"));
    }
    out.push_str(&format!("// Hook manifest: {}\n", family.manifest));
    out.push_str("// ---------------------------------------------------------------------------\n");
    out.push_str("((dom) => {\n");
    out.push_str("  const INTERNAL = dom.internal;\n");
    out.push_str(
        "  function op(name, ...args) {\n    try {\n      return dom.hook(name, ...args);\n    } catch (error) {\n      const message = String(error && error.message !== undefined ? error.message : error);\n      const cut = message.indexOf(\": \");\n      if (cut > 0) {\n        const name = message.slice(0, cut);\n        if (/^[A-Z][A-Za-z]*Error$/.test(name)) dom.fail(name, message.slice(cut + 2));\n      }\n      throw error;\n    }\n  }\n",
    );
    for (name, source) in CONVERSION_SOURCES {
        if model.conversions.iter().any(|c| *c == name) {
            out.push_str(source);
        }
    }

    let bases = model.base_map();
    for interface in &model.interfaces {
        let base_expr = match &interface.base {
            None => "dom.base.Node".to_owned(),
            Some(base) => {
                if bases.contains_key(base.as_str()) {
                    base.clone()
                } else {
                    family
                        .native_base
                        .get(base)
                        .cloned()
                        .unwrap_or_else(|| "dom.base.Node".to_owned())
                }
            }
        };
        out.push_str(&format!("  class {} extends {} {{\n", interface.name, base_expr));
        emit_constructor(&mut out, model, interface);
        emit_members(&mut out, model, interface);
        out.push_str("  }\n");
    }
    for interface in &model.interfaces {
        out.push_str(&format!(
            "  dom.classes.{} = {};\n",
            interface.name, interface.name
        ));
    }
    for global in &family.globals {
        out.push_str(&format!("  dom.globals.{global} = {global};\n"));
    }
    out.push_str("})(globalThis.__wega_dom);\n");
    out
}

/// Generated ancestors (base first) plus the interface itself, for the brand
/// list each constructor installs.
fn brand_list(model: &FamilyModel, interface: &Interface) -> String {
    let bases = model.base_map();
    let mut chain = vec![interface.name.clone()];
    let mut current = interface;
    while let Some(base) = &current.base {
        match bases.get(base.as_str()) {
            Some(base_interface) => {
                chain.push(base_interface.name.clone());
                current = base_interface;
            }
            None => break,
        }
    }
    chain.reverse();
    chain.join(", ")
}

fn emit_constructor(out: &mut String, model: &FamilyModel, interface: &Interface) {
    let generated_base = interface
        .base
        .as_ref()
        .is_some_and(|base| model.is_generated_base(base));
    let super_args = if generated_base {
        "id, INTERNAL, nodeInfo"
    } else {
        "id, nodeInfo"
    };
    let brands = brand_list(model, interface);
    let ctor = interface
        .members
        .iter()
        .find_map(|member| match member {
            Member::Constructor(ctor) => Some(ctor),
            _ => None,
        });
    match ctor {
        Some(ConstructorMember { params, .. }) => {
            let idl_params: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            out.push_str(&format!(
                "    constructor({}, internal, info) {{\n",
                idl_params.join(", ")
            ));
            out.push_str("      let id, nodeInfo;\n");
            out.push_str("      if (internal === INTERNAL) {\n");
            out.push_str(&format!(
                "        id = {};\n",
                idl_params.first().cloned().unwrap_or_default()
            ));
            out.push_str("        nodeInfo = info;\n");
            out.push_str("      } else {\n");
            let hook = model
                .constructor_hook(&interface.name)
                .expect("constructor hook derived in build_family");
            out.push_str(&format!(
                "        const created = op(\"{}\", {});\n",
                hook.name,
                params.iter().map(arg_expression).collect::<Vec<_>>().join(", ")
            ));
            out.push_str("        id = created.id;\n");
            out.push_str("        nodeInfo = created.info;\n");
            out.push_str("      }\n");
            out.push_str(&format!("      super({super_args});\n"));
            out.push_str(&format!("      dom.brands([{brands}], this);\n"));
            out.push_str("    }\n");
        }
        None => {
            out.push_str("    constructor(id, internal, info) {\n");
            out.push_str(
                "      if (internal !== INTERNAL) throw new TypeError(\"Illegal constructor\");\n",
            );
            out.push_str(&format!("      super({super_args});\n"));
            out.push_str(&format!("      dom.brands([{brands}], this);\n"));
            out.push_str("    }\n");
        }
    }
}

fn emit_members(out: &mut String, model: &FamilyModel, interface: &Interface) {
    for member in &interface.members {
        match member {
            Member::Constructor(_) => {}
            Member::Const(cst) => {
                // Emit const as static class property: writable=false.
                let literal = match &cst.value {
                    ConstValue::Number(n) => n.to_string(),
                    ConstValue::Boolean(b) => b.to_string(),
                    ConstValue::String(s) => format!("{:?}", s),
                };
                out.push_str(&format!(
                    "    static {{\n      Object.defineProperty(this, \"{name}\", {{ value: {literal}, writable: false, enumerable: true, configurable: true }});\n    }}\n",
                    name = cst.name,
                    literal = literal
                ));
            }
            Member::Enum(enumm) => {
                // Emit enum as static Object.freeze map on the class:
                //   class X { static { Object.defineProperty(this, "EnumName", { value: Object.freeze({ value1: "value1", ... }) }) } }
                let entries: Vec<String> = enumm
                    .values
                    .iter()
                    .map(|v| format!("{key}: {val}", key = v, val = format!("{:?}", v)))
                    .collect();
                let frozen = format!("Object.freeze({{{entries}}})", entries = entries.join(", "));
                out.push_str(&format!(
                    "    static {{\n      Object.defineProperty(this, \"{name}\", {{ value: {frozen}, writable: false, enumerable: false, configurable: true }});\n    }}\n",
                    name = enumm.name,
                ));
            }
            Member::Attribute(AttributeMember {
                name,
                ty,
                readonly,
                constant: _,
                same_object,
                setter_throws: _,
                legacy_null_to_empty,
                ..
            }) => {
                let getter = model
                    .hook(&interface.name, name, false)
                    .expect("attribute getter hook derived in build_family");
                out.push_str(&format!("    get {name}() {{\n"));
                out.push_str(&format!("      dom.checkBrand({}, this);\n", interface.name));
                if *same_object {
                    // [SameObject] memoisation on a private Symbol field.
                    out.push_str(&format!(
                        "      if (this[dom._so]) return this[dom._so].get(\"{name}\");\n"
                    ));
                    out.push_str(&format!("      let _m = this[dom._so];\n"));
                    out.push_str(&format!("      if (!_m) {{ _m = new Map(); this[dom._so] = _m; }}\n"));
                    out.push_str(&format!("      let _v = _m.get(\"{name}\");\n"));
                    out.push_str(&format!("      if (!_v) {{ _v = op(\"{}\", this._id); _m.set(\"{name}\", _v); }}\n", getter.name));
                    out.push_str(&format!("      return _v;\n"));
                } else {
                    out.push_str(&format!("      return op(\"{}\", this._id);\n", getter.name));
                }
                out.push_str("    }\n");
                if !readonly {
                    let setter = model
                        .hook(&interface.name, name, true)
                        .expect("attribute setter hook derived in build_family");
                    let conversion = conversion_for(ty, *legacy_null_to_empty);
                    out.push_str(&format!("    set {name}(value) {{\n"));
                    out.push_str(&format!("      dom.checkBrand({}, this);\n", interface.name));
                    out.push_str(&format!(
                        "      const result = op(\"{}\", this._id, {conversion}(value));\n",
                        setter.name
                    ));
                    if *same_object {
                        // Invalidate [SameObject] cache on set.
                        out.push_str(&format!("      if (this[dom._so]) this[dom._so].delete(\"{name}\");\n"));
                    }
                    if let Some(reaction) = &setter.reaction {
                        out.push_str(&format!(
                            "      dom.reactions.{reaction}(this, result);\n"
                        ));
                    }
                    out.push_str("    }\n");
                }
            }
            Member::Operation(op) => {
                let hook = model
                    .hook(&interface.name, &op.name, false)
                    .expect("operation hook derived in build_family");
                out.push_str(&format!(
                    "    {}({}) {{\n",
                    op.name,
                    op.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")
                ));
                out.push_str(&format!("      dom.checkBrand({}, this);\n", interface.name));
                let args: Vec<String> = std::iter::once("this._id".to_owned())
                    .chain(op.params.iter().map(arg_expression))
                    .collect();
                match (op.new_object, matches!(op.ty, Type::Undefined)) {
                    (true, _) => {
                        out.push_str(&format!(
                            "      const result = op(\"{}\", {});\n",
                            hook.name,
                            args.join(", ")
                        ));
                        out.push_str(
                            "      const next = dom.wrap(result.id, this.ownerDocument || this);\n",
                        );
                        if let Some(reaction) = &hook.reaction {
                            out.push_str(&format!(
                                "      dom.reactions.{reaction}(this, next, result);\n"
                            ));
                        }
                        out.push_str("      return next;\n");
                    }
                    (false, true) => {
                        if let Some(reaction) = &hook.reaction {
                            out.push_str(&format!(
                                "      const result = op(\"{}\", {});\n",
                                hook.name,
                                args.join(", ")
                            ));
                            out.push_str(&format!(
                                "      dom.reactions.{reaction}(this, result);\n"
                            ));
                        } else {
                            out.push_str(&format!("      op(\"{}\", {});\n", hook.name, args.join(", ")));
                        }
                    }
                    (false, false) => {
                        out.push_str(&format!(
                            "      return op(\"{}\", {});\n",
                            hook.name,
                            args.join(", ")
                        ));
                    }
                }
                out.push_str("    }\n");
            }
        }
    }
}

/// Emit the semantic-hook manifest (checked in next to the JS file).
pub fn emit_manifest(model: &FamilyModel) -> String {
    let family = &model.family;
    let doc = serde_json::json!({
        "family": family.name,
        "generator": "wega-webidl (step 07.a)",
        "convention": "node hooks take the node id string as their first argument; hook errors are 'ExceptionName: message' strings",
        "idls": model.pins.iter().map(|(file, sha256)| serde_json::json!({"file": file, "sha256": sha256})).collect::<Vec<_>>(),
        "native_base": family.native_base,
        "base_mixins": family.base_mixins,
        "globals": family.globals,
        "hooks": model.hooks.iter().map(HookSpec::to_json).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&doc).unwrap() + "\n"
}
