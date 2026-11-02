//! wega-webidl: deterministic WebIDL binding generator for the standalone
//! engine. Inputs are pinned in tools/webidl/idls.json; outputs are checked in
//! under engines/wega/src/dom/generated/ and byte-compared by `check`.

mod ast;
mod generate;
mod manifest;
mod parser;

use generate::{build_family, emit_js, emit_manifest};
use manifest::{IdlManifest, IdlPin, load_families, load_pins, verify_family_files};
use std::path::PathBuf;

const IDLS_JSON: &str = "tools/webidl/idls.json";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = run(&args) {
        eprintln!("wega-webidl: {error}");
        std::process::exit(1);
    }
}

fn repo_root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    if cwd.join("Cargo.toml").is_file() && cwd.join("tools/webidl").is_dir() {
        Ok(cwd)
    } else {
        Err("run from the repository root".into())
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("verify") => cmd_verify(),
        Some("generate") => cmd_generate(true),
        Some("check") => cmd_generate(false),
        Some("pin") => cmd_pin(&args[1..]),
        _ => Err("usage: wega-webidl {verify | generate | check | pin <idl-file>...}".into()),
    }
}

/// Verify pins, parse every family input and validate family configuration.
fn cmd_verify() -> Result<(), String> {
    let root = repo_root()?;
    let pins = load_pins(&root)?;
    let families = load_families(&root)?;
    if families.is_empty() {
        return Err("no families declared in tools/webidl/families.json".into());
    }
    for family in &families {
        let sources = verify_family_files(&root, family, &pins)?;
        build_family(family, &pins, &sources)?;
        println!(
            "family '{}' OK ({} interfaces, {} hooks)",
            family.name,
            build_family(family, &pins, &sources)?.interfaces.len(),
            build_family(family, &pins, &sources)?.hooks.len(),
        );
    }
    Ok(())
}

/// Regenerate family outputs. `write=true` updates the checked-in artifacts;
/// otherwise the outputs are byte-compared against them (CI gate).
fn cmd_generate(write: bool) -> Result<(), String> {
    let root = repo_root()?;
    let pins = load_pins(&root)?;
    let families = load_families(&root)?;
    let mut written = Vec::new();
    let mut registry = Vec::new();
    for family in &families {
        let sources = verify_family_files(&root, family, &pins)?;
        let model = build_family(family, &pins, &sources)?;
        let js = emit_js(&model);
        let hooks = emit_manifest(&model);
        registry.extend(
            serde_json::from_str::<serde_json::Value>(&hooks).unwrap()["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
        let js_path = root.join(&family.output);
        let hooks_path = root.join(&family.manifest);
        if write {
            for path in [&js_path, &hooks_path] {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
            }
            std::fs::write(&js_path, &js).map_err(|e| e.to_string())?;
            std::fs::write(&hooks_path, &hooks).map_err(|e| e.to_string())?;
            written.push(format!("{}/{}", family.name, family.output));
            written.push(family.manifest.clone());
        } else {
            for (path, expected, label) in [
                (&js_path, js.as_str(), "bindings"),
                (&hooks_path, hooks.as_str(), "hook manifest"),
            ] {
                let current = std::fs::read_to_string(path).map_err(|e| {
                    format!("{}: cannot read checked-in {}: {e}", path.display(), label)
                })?;
                if current != expected {
                    return Err(format!(
                        "family '{}': checked-in {} at {} is stale; run `wega-webidl generate`",
                        family.name,
                        label,
                        path.display()
                    ));
                }
            }
        }
    }
    let registry = serde_json::to_string_pretty(&registry).unwrap() + "\n";
    let registry_path = root.join("engines/wega/src/dom/generated/hooks.json");
    if write {
        std::fs::write(&registry_path, registry).map_err(|e| e.to_string())?;
    } else if std::fs::read_to_string(&registry_path).map_err(|e| e.to_string())? != registry {
        return Err("native hook registry is stale; run wega-webidl generate".into());
    }
    if write {
        for path in &written {
            println!("wrote {path}");
        }
    } else {
        println!("all family outputs are current");
    }
    Ok(())
}

/// Record sha256 hashes for the given IDL files in idls.json.
fn cmd_pin(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("pin: pass at least one IDL file path".into());
    }
    let root = repo_root()?;
    let manifest = match std::fs::read_to_string(root.join(IDLS_JSON)) {
        Ok(text) => {
            serde_json::from_str::<IdlManifest>(&text).map_err(|e| format!("{IDLS_JSON}: {e}"))?
        }
        Err(_) => IdlManifest { pins: Vec::new() },
    };
    let mut pins = manifest.pins;
    for path in paths {
        let file = root.join(path);
        let source = std::fs::read_to_string(&file)
            .map_err(|e| format!("{path}: cannot read IDL file: {e}"))?;
        let digest = manifest::sha256_hex(source.as_bytes());
        match pins.iter_mut().find(|pin| pin.path == *path) {
            Some(pin) => pin.sha256 = digest,
            None => pins.push(IdlPin {
                path: path.clone(),
                sha256: digest,
                origin: String::new(),
                license: String::new(),
            }),
        }
    }
    let text = serde_json::to_string_pretty(&IdlManifest { pins }).unwrap() + "\n";
    std::fs::write(root.join(IDLS_JSON), text).map_err(|e| e.to_string())?;
    println!("updated {IDLS_JSON}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Family, HookAnnotation};
    use std::collections::{BTreeMap, HashMap};
    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }
    fn fixture(sources: &[(&str, &str)], names: &[&str]) -> Result<generate::FamilyModel, String> {
        let mut family = Family {
            name: "fixture".into(),
            hook_prefix: "fx".into(),
            output: "out.js".into(),
            manifest: "out.json".into(),
            files: vec![],
            native_base: HashMap::new(),
            base_mixins: vec![],
            globals: vec!["Fixture".into()],
            hooks: HashMap::new(),
        };
        for name in names {
            family.hooks.insert(
                format!("fx.{name}"),
                HookAnnotation {
                    result: "typed".into(),
                    reaction: None,
                },
            );
        }
        let mut pins = BTreeMap::new();
        let mut input = vec![];
        for (path, source) in sources {
            pins.insert(
                path.to_string(),
                IdlPin {
                    path: path.to_string(),
                    sha256: manifest::sha256_hex(source.as_bytes()),
                    origin: "test".into(),
                    license: "MPL-2.0".into(),
                },
            );
            input.push((path.to_string(), source.to_string()));
        }
        build_family(&family, &pins, &input)
    }
    #[test]
    fn all_pinned_families_and_native_registry_are_deterministic_and_current() {
        let root = root();
        let pins = load_pins(&root).unwrap();
        let mut registry = vec![];
        let mut names = std::collections::HashSet::new();
        for family in load_families(&root).unwrap() {
            let sources = verify_family_files(&root, &family, &pins).unwrap();
            let first = build_family(&family, &pins, &sources).unwrap();
            let second = build_family(&family, &pins, &sources).unwrap();
            assert_eq!(emit_js(&first), emit_js(&second));
            assert_eq!(emit_manifest(&first), emit_manifest(&second));
            assert_eq!(
                emit_js(&first),
                std::fs::read_to_string(root.join(&family.output)).unwrap(),
                "{} JS drift",
                family.name
            );
            let manifest = emit_manifest(&first);
            assert_eq!(
                manifest,
                std::fs::read_to_string(root.join(&family.manifest)).unwrap(),
                "{} hook drift",
                family.name
            );
            let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
            for hook in manifest["hooks"].as_array().unwrap() {
                assert!(names.insert(hook["name"].as_str().unwrap().to_owned()));
                registry.push(hook.clone());
            }
            assert!(!first.interfaces.is_empty());
        }
        assert_eq!(
            serde_json::to_string_pretty(&registry).unwrap() + "\n",
            std::fs::read_to_string(root.join("engines/wega/src/dom/generated/hooks.json"))
                .unwrap()
        );
    }
    #[test]
    fn partials_and_mixins_merge_across_files_without_erasing_types() {
        let model=fixture(&[
            ("partial.idl","partial interface Fixture { Fixture? parent(); };"),
            ("base.idl","[Exposed=Window] interface Fixture { constructor(); }; interface mixin M { unsigned long count(); }; Fixture includes M;"),
        ],&["construct.fixture","parent","count"]).unwrap();
        assert_eq!(model.interfaces.len(), 1);
        assert_eq!(
            model
                .hooks
                .iter()
                .find(|h| h.member == "parent")
                .unwrap()
                .result,
            "Fixture?"
        );
        assert!(emit_js(&model).contains("\"kind\":\"interface\""));
    }
    #[test]
    fn invalid_input_has_positioned_errors() {
        for source in [
            "[Exposed=Worker] interface A {};",
            "[Exposed=Window] interface A { [Unknown] undefined x(); };",
            "[Exposed=Window] interface A { undefined x(DOMString... a, DOMString b); };",
            "[Exposed=Window] interface A { undefined x(optional DOMString a, DOMString b); };",
            "dictionary A { required boolean a = false; };",
            "dictionary A { DOMString x; DOMString x; };",
            "[Exposed=Window] interface A { iterable<DOMString; };",
        ] {
            let error = parser::parse_definitions("invalid.idl", source).unwrap_err();
            assert!(error.line > 0);
            assert!(error.to_string().starts_with("invalid.idl:"));
        }
    }
    #[test]
    fn cycles_and_hook_drift_are_errors() {
        assert!(fixture(&[("cycle.idl","typedef B A; typedef A B; [Exposed=Window] interface Fixture { constructor(); }; ")],&["construct.fixture"]).err().unwrap().contains("recursive"));
        assert!(
            fixture(
                &[(
                    "fixture.idl",
                    "[Exposed=Window] interface Fixture { constructor(); undefined missing(); };"
                )],
                &["construct.fixture"]
            )
            .err()
            .unwrap()
            .contains("missing")
        );
        assert!(
            fixture(
                &[(
                    "fixture.idl",
                    "[Exposed=Window] interface Fixture { constructor(); };"
                )],
                &["construct.fixture", "extra"]
            )
            .err()
            .unwrap()
            .contains("unknown hook")
        );
    }
    #[test]
    fn tampered_pin_cannot_be_generated() {
        let root = root();
        let mut pins = load_pins(&root).unwrap();
        let family = load_families(&root).unwrap().remove(0);
        pins.get_mut(&family.files[0]).unwrap().sha256 = "0".repeat(64);
        assert!(
            verify_family_files(&root, &family, &pins)
                .err()
                .unwrap()
                .contains("does not match")
        );
    }
    #[test]
    fn emitted_bindings_execute_typed_conversions_brands_and_same_object_contracts() {
        let idl = r#"
enum Mode { "open", "closed" };
typedef Mode Alias;
dictionary Base { boolean a = false; };
dictionary Options : Base { required DOMString name; Alias mode = "open"; sequence<DOMString> tokens = []; };
callback Listener = undefined (Fixture target);
[Exposed=Window] interface Fixture {
 constructor(Options options);
 const unsigned short VERSION = 7;
 [SameObject] readonly attribute object first;
 [SameObject] readonly attribute object second;
 [SameObject] object cache(unsigned long key);
 object inspect(Fixture node, Options options, optional boolean active, DOMString... tokens);
 undefined sample((DOMString or Options) value);
 undefined listen(Listener callback);
 static Alias mode(Alias value);
};"#;
        let model = fixture(
            &[("fixture.idl", idl)],
            &[
                "construct.fixture",
                "first",
                "second",
                "cache",
                "inspect",
                "sample",
                "listen",
                "mode",
            ],
        )
        .unwrap();
        let setup = r#"
const assert = require('node:assert/strict');
const branded = new WeakSet(), hasBrand = Function.call.bind(WeakSet.prototype.has);
let reads=0, hooks=[];
class Fixture {
 constructor(options) { this.options=options; branded.add(this); }
 get first() { return { n: ++reads }; }
 get second() { return { n: ++reads }; }
 cache(key) { return { key }; }
 inspect(node, options, active, ...tokens) { return { node, options, active, tokens }; }
 sample(value) { this.sampled=value; }
 listen(callback) { callback(this); }
 static mode(value) { return value; }
}
globalThis.__wega_semantics={ classes:{Fixture}, brand:(_name,value)=>hasBrand(branded,value), ids:()=>[], construct:(_name,args,newTarget)=>Reflect.construct(Fixture,args,newTarget) };
globalThis.__wega_guard_binding=(name)=>hooks.push(name);
"#;
        let assertions = r#"
const F=globalThis.Fixture, order=[];
const instance=new F({ get a(){order.push('a');return 1},get mode(){order.push('mode');return 'closed'},get name(){order.push('name');return 'hi'},get tokens(){order.push('tokens');return ['a',2]}});
assert.deepEqual(order,['a','mode','name','tokens']);
assert.equal(instance.options.a,true); assert.equal(instance.options.mode,'closed');
assert.deepEqual(instance.options.tokens,['a','2']);
assert.throws(()=>new F({}),TypeError);
assert.throws(()=>F.mode('bad'),TypeError); assert.equal(F.mode('open'),'open');
assert.equal(F.VERSION,7); assert.equal(F.prototype.VERSION,7);
assert.equal(Object.getOwnPropertyDescriptor(F,'VERSION').writable,false);
assert.equal(instance.first,instance.first);assert.equal(instance.second,instance.second);
assert.notEqual(instance.first,instance.second);assert.equal(reads,2);
assert.equal(instance.cache(Infinity),instance.cache(0));
assert.equal(instance.cache(1),instance.cache(1));assert.notEqual(instance.cache(1),instance.cache(2));
assert.throws(()=>instance.inspect(Object.create(F.prototype),{name:'x'}),TypeError);
assert.throws(()=>F.prototype.inspect.call({},instance,{name:'x'}),TypeError);
const result=instance.inspect(instance,{name:'x'},undefined,'a',2);
assert.equal(result.node,instance);assert.equal(result.active,undefined);assert.deepEqual(result.tokens,['a','2']);
assert.throws(()=>instance.inspect(),TypeError);
const failure=new Error('getter'); assert.throws(()=>instance.sample({get name(){throw failure}}),error=>error===failure);
instance.sample({name:'dictionary'});assert.equal(instance.sampled.name,'dictionary');
instance.sample(123);assert.equal(instance.sampled,'123');
assert.throws(()=>instance.sample(Symbol()),TypeError);
let callback=false;instance.listen(value=>{callback=value===instance});assert(callback);
assert.throws(()=>instance.listen({handleEvent(){}}),TypeError);
let closed=false;
assert.throws(()=>new F({name:'x',tokens:{*[Symbol.iterator](){try{yield Symbol()}finally{closed=true}}}}),TypeError);assert(closed);
const old=WeakMap.prototype.get;WeakMap.prototype.get=function(){throw Error('poisoned')};
assert.equal(instance.first,instance.first);WeakMap.prototype.get=old;
assert(hooks.includes('fx.construct.fixture'));assert(hooks.includes('fx.inspect'));
"#;
        let runtime =
            std::fs::read_to_string(root().join("engines/wega/src/dom/bindings.js")).unwrap();
        let program = format!("{setup}\n{runtime}\n{}\n{assertions}", emit_js(&model));
        let output = std::process::Command::new("node")
            .args(["-e", &program])
            .output()
            .expect("Node is required for emitted binding tests");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
