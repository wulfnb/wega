//! Deterministic typed binding descriptors and an exhaustive native hook registry.
use crate::ast::*;
use crate::manifest::{Family, IdlPin};
use crate::parser::parse_definitions;
use serde_json::{Value, json};
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
    fn json(&self) -> Value {
        json!({"name":self.name,"interface":self.interface,"member":self.member,
        "kind":self.kind,"pure":self.pure,"throws":self.throws,"new_object":self.new_object,
        "args":self.args,"result":self.result,"reaction":self.reaction,"line":self.line})
    }
}
pub struct FamilyModel {
    pub family: Family,
    pub interfaces: Vec<Interface>,
    pub hooks: Vec<HookSpec>,
    pub pins: Vec<(String, String)>,
    pub descriptors: Vec<Value>,
}
fn merge<T>(target: &mut BTreeMap<String, T>, source: BTreeMap<String, T>) -> Result<(), String> {
    for (name, value) in source {
        if target.insert(name.clone(), value).is_some() {
            return Err(format!("duplicate type declaration '{name}'"));
        }
    }
    Ok(())
}
fn ty_json(ty: &Type, defs: &Definitions, depth: usize) -> Result<Value, String> {
    if depth > 32 {
        return Err("recursive typedef/dictionary or type nesting exceeds 32".into());
    }
    Ok(match ty {
        Type::DomString { nullable } => json!({"kind":"string","nullable":nullable}),
        Type::Boolean => json!({"kind":"boolean"}),
        Type::UnsignedLong => json!({"kind":"unsigned","bits":32}),
        Type::UnsignedShort => json!({"kind":"unsigned","bits":16}),
        Type::Float => json!({"kind":"float"}),
        Type::Double => json!({"kind":"double"}),
        Type::Undefined => json!({"kind":"undefined"}),
        Type::Any => json!({"kind":"any"}),
        Type::Object => json!({"kind":"object"}),
        Type::Sequence(t) => json!({"kind":"sequence","element":ty_json(t,defs,depth+1)?}),
        Type::Promise(t) => json!({"kind":"promise","element":ty_json(t,defs,depth+1)?}),
        Type::Union(types) => {
            json!({"kind":"union","types":types.iter().map(|t|ty_json(t,defs,depth+1)).collect::<Result<Vec<_>,_>>()?})
        }
        Type::Interface { name, nullable } => {
            if let Some(alias) = defs.typedefs.get(name) {
                let mut ty = ty_json(alias, defs, depth + 1)?;
                if *nullable {
                    ty["nullable"] = json!(true);
                }
                ty
            } else if let Some(values) = defs.enums.get(name) {
                json!({"kind":"enum","name":name,"values":values,"nullable":nullable})
            } else if defs.dictionaries.contains_key(name) {
                let members = dictionary_members(name, defs, depth + 1)?;
                json!({"kind":"dictionary","name":name,"members":members,"nullable":nullable})
            } else if let Some(callback) = defs.callbacks.get(name) {
                json!({"kind":if callback.2 { "callback" } else { "callback-interface" },"name":name,"nullable":nullable})
            } else {
                json!({"kind":"interface","name":name,"nullable":nullable})
            }
        }
    })
}
fn dictionary_members(name: &str, defs: &Definitions, depth: usize) -> Result<Vec<Value>, String> {
    if depth > 32 {
        return Err(format!("dictionary inheritance/type cycle at {name}"));
    }
    let dictionary = defs
        .dictionaries
        .get(name)
        .ok_or_else(|| format!("unknown base dictionary {name}"))?;
    let mut members = if let Some(base) = &dictionary.base {
        dictionary_members(base, defs, depth + 1)?
    } else {
        vec![]
    };
    for member in &dictionary.members {
        if members.iter().any(|m| m["name"] == member.name) {
            return Err(format!("dictionary {name} redeclares {}", member.name));
        }
        members.push(param_json(member, defs, depth + 1)?);
    }
    Ok(members)
}
fn param_json(param: &Parameter, defs: &Definitions, depth: usize) -> Result<Value, String> {
    let mut value = json!({"name":param.name,"type":ty_json(&param.ty,defs,depth+1)?,"optional":param.optional,"variadic":param.variadic});
    if let Some(default) = &param.default {
        value["default"] =
            serde_json::from_str(&default.js_literal()).map_err(|e| e.to_string())?;
    }
    Ok(value)
}
fn params_json(params: &[Parameter], defs: &Definitions) -> Result<Value, String> {
    Ok(json!(
        params
            .iter()
            .map(|p| param_json(p, defs, 0))
            .collect::<Result<Vec<_>, _>>()?
    ))
}

fn validate_type_names(value: &Value, known: &BTreeSet<String>) -> Result<(), String> {
    match value {
        Value::Object(fields) => {
            if fields.get("kind").and_then(Value::as_str) == Some("interface") {
                let name = fields["name"].as_str().unwrap();
                if !known.contains(name) {
                    return Err(format!("unresolved interface type '{name}'"));
                }
            }
            for value in fields.values() {
                validate_type_names(value, known)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_type_names(value, known)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn build_family(
    family: &Family,
    pins: &BTreeMap<String, IdlPin>,
    sources: &[(String, String)],
) -> Result<FamilyModel, String> {
    let mut defs = Definitions::default();
    let mut declarations = vec![];
    for (path, source) in sources {
        let parsed = parse_definitions(path, source).map_err(|e| e.to_string())?;
        declarations.extend(parsed.interfaces);
        merge(&mut defs.dictionaries, parsed.dictionaries)?;
        merge(&mut defs.enums, parsed.enums)?;
        merge(&mut defs.typedefs, parsed.typedefs)?;
        merge(&mut defs.callbacks, parsed.callbacks)?;
    }
    let mut interfaces: Vec<Interface> = vec![];
    for declaration in declarations.iter().filter(|d| !d.partial) {
        if interfaces.iter().any(|i| i.name == declaration.name) {
            return Err(format!(
                "{}:{}: duplicate interface {}",
                declaration.file, declaration.line, declaration.name
            ));
        }
        interfaces.push(declaration.clone());
    }
    for partial in declarations.iter().filter(|d| d.partial) {
        let original = interfaces
            .iter_mut()
            .find(|i| i.name == partial.name)
            .ok_or_else(|| {
                format!(
                    "{}:{}: partial interface {} has no primary declaration",
                    partial.file, partial.line, partial.name
                )
            })?;
        original.members.extend(partial.members.clone());
        original.includes.extend(partial.includes.clone());
    }
    let mixins: BTreeMap<_, _> = interfaces
        .iter()
        .filter(|i| i.mixin)
        .map(|i| (i.name.clone(), i.members.clone()))
        .collect();
    interfaces.retain(|i| !i.mixin);
    for interface in &mut interfaces {
        for mixin in &interface.includes {
            if let Some(members) = mixins.get(mixin) {
                interface.members.extend(members.clone());
            } else if !family.base_mixins.contains(mixin) {
                return Err(format!(
                    "{}:{}: unknown mixin {mixin}",
                    interface.file, interface.line
                ));
            }
        }
        let mut names = BTreeSet::new();
        for member in &interface.members {
            if !names.insert(member.name()) {
                return Err(format!(
                    "{}:{}: duplicate member {}.{}",
                    interface.file,
                    member.line(),
                    interface.name,
                    member.name()
                ));
            }
        }
        if interface.exposed != ["Window"] {
            return Err(format!(
                "{}:{}: unsupported interface exposure",
                interface.file, interface.line
            ));
        }
    }
    if interfaces.is_empty() {
        return Err("family has no interfaces".into());
    }
    let mut ordered = vec![];
    while !interfaces.is_empty() {
        let previous = interfaces.len();
        interfaces.retain(|i| {
            let ready = i.base.as_ref().is_none_or(|base| {
                family.native_base.contains_key(base)
                    || ordered.iter().any(|b: &Interface| &b.name == base)
            });
            if ready {
                ordered.push(i.clone());
                false
            } else {
                true
            }
        });
        if interfaces.len() == previous {
            return Err("unknown or cyclic interface base".into());
        }
    }
    // Validate aliases even if not referenced by a member; no silent dead declarations.
    for name in defs.typedefs.keys().chain(defs.dictionaries.keys()) {
        ty_json(
            &Type::Interface {
                name: name.clone(),
                nullable: false,
            },
            &defs,
            0,
        )?;
    }
    let mut hooks = vec![];
    let mut descriptors = vec![];
    for interface in &ordered {
        let mut members = vec![];
        for member in &interface.members {
            let mut specs: Vec<(&str, String, Vec<String>, &Type, bool, bool, bool)> = vec![];
            let descriptor = match member {
                Member::Attribute(a) => {
                    specs.push((
                        "attribute-getter",
                        format!("{}.{}", family.hook_prefix, a.name),
                        vec![],
                        &a.ty,
                        a.pure,
                        a.throws,
                        false,
                    ));
                    if !a.readonly || a.forwards.is_some() {
                        specs.push((
                            "attribute-setter",
                            format!("{}.{}.set", family.hook_prefix, a.name),
                            vec![a.ty.spelling()],
                            &Type::Undefined,
                            false,
                            a.setter_throws,
                            false,
                        ));
                    }
                    json!({"kind":"attribute","name":a.name,"type":ty_json(&a.ty,&defs,0)?,"readonly":a.readonly,
                        "constant":a.constant,"sameObject":a.same_object,"legacyNull":a.legacy_null_to_empty,"forwards":a.forwards})
                }
                Member::Operation(op) => {
                    specs.push((
                        "operation",
                        format!("{}.{}", family.hook_prefix, op.name),
                        op.params.iter().map(|p| p.ty.spelling()).collect(),
                        &op.ty,
                        false,
                        op.throws,
                        op.new_object,
                    ));
                    json!({"kind":"operation","name":op.name,"type":ty_json(&op.ty,&defs,0)?,"parameters":params_json(&op.params,&defs)?,"sameObject":op.same_object,"static":op.static_,"getter":op.getter})
                }
                Member::Constructor(c) => {
                    specs.push((
                        "constructor",
                        format!(
                            "{}.construct.{}",
                            family.hook_prefix,
                            interface.name.to_lowercase()
                        ),
                        c.params.iter().map(|p| p.ty.spelling()).collect(),
                        &Type::Undefined,
                        false,
                        c.throws,
                        false,
                    ));
                    json!({"kind":"constructor","name":"constructor","parameters":params_json(&c.params,&defs)?})
                }
                Member::Const(c) => {
                    let value = match &c.value {
                        ConstValue::Number(n) => json!(n),
                        ConstValue::Boolean(b) => json!(b),
                        ConstValue::String(s) => json!(s),
                    };
                    json!({"kind":"constant","name":c.name,"value":value})
                }
                Member::Enum(e) => json!({"kind":"enum","name":e.name,"values":e.values}),
                Member::Iterable(types, _) => {
                    json!({"kind":"iterable","name":"@@iterator","types":types.iter().map(|t|ty_json(t,&defs,0)).collect::<Result<Vec<_>,_>>()?})
                }
            };
            for (kind, name, args, result, pure, throws, new_object) in specs {
                let annotation = family
                    .hooks
                    .get(&name)
                    .ok_or_else(|| format!("hook '{name}' is missing its semantic annotation"))?;
                hooks.push(HookSpec {
                    name,
                    interface: interface.name.clone(),
                    member: member.name().into(),
                    kind,
                    pure,
                    throws,
                    new_object,
                    args,
                    result: result.spelling(),
                    reaction: annotation.reaction.clone(),
                    line: member.line(),
                });
            }
            members.push(descriptor);
        }
        descriptors.push(json!({"name":interface.name,"base":interface.base,"prefix":family.hook_prefix,"exposed":family.globals.contains(&interface.name),"members":members}));
    }
    let known: BTreeSet<String> = family
        .native_base
        .keys()
        .cloned()
        .chain(ordered.iter().map(|i| i.name.clone()))
        .collect();
    for descriptor in &descriptors {
        validate_type_names(descriptor, &known)?;
    }
    let derived: BTreeSet<_> = hooks.iter().map(|h| &h.name).collect();
    for name in family.hooks.keys() {
        if !derived.contains(name) {
            return Err(format!(
                "family {} annotates unknown hook '{name}'",
                family.name
            ));
        }
    }
    for global in &family.globals {
        if !ordered.iter().any(|i| &i.name == global) {
            return Err(format!("unknown exposed interface {global}"));
        }
    }
    let pins = sources
        .iter()
        .map(|(path, _)| {
            pins.get(path)
                .map(|pin| (path.clone(), pin.sha256.clone()))
                .ok_or_else(|| format!("missing pin {path}"))
        })
        .collect::<Result<_, _>>()?;
    Ok(FamilyModel {
        family: family.clone(),
        interfaces: ordered,
        hooks,
        pins,
        descriptors,
    })
}
/// Serialize identically with or without serde_json/preserve_order (Deno enables it).
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            let sorted: BTreeMap<String, Value> = fields.into_iter().collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| (key, canonical(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        value => value,
    }
}

pub fn emit_js(model: &FamilyModel) -> String {
    let mut output = String::from("// Generated by wega-webidl. Do not edit.\n");
    for (path, hash) in &model.pins {
        output.push_str(&format!("// {path} sha256={hash}\n"));
    }
    output.push_str("((dom) => {\n");
    for descriptor in &model.descriptors {
        output.push_str("  dom.install(");
        output.push_str(&serde_json::to_string(&canonical(descriptor.clone())).unwrap());
        output.push_str(");\n");
    }
    output.push_str("})(globalThis.__wega_dom);\n");
    output
}
pub fn emit_manifest(model: &FamilyModel) -> String {
    serde_json::to_string_pretty(&canonical(json!({"family":model.family.name,"generator":"wega-webidl typed descriptors v2","idls":model.pins,"native_base":model.family.native_base,"base_mixins":model.family.base_mixins,"globals":model.family.globals,"hooks":model.hooks.iter().map(HookSpec::json).collect::<Vec<_>>(),"interfaces":model.descriptors}))).unwrap()+"\n"
}
