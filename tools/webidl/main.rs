//! wega-webidl: deterministic WebIDL binding generator for the standalone
//! engine. Inputs are pinned in tools/webidl/idls.json; outputs are checked in
//! under engines/wega/src/dom/generated/ and byte-compared by `check`.

mod ast;
mod generate;
mod manifest;
mod parser;

use generate::{build_family, emit_js, emit_manifest};
use manifest::{
    Family, HookAnnotation, IdlManifest, IdlPin, load_families, load_pins, verify_family_files,
};
use std::path::PathBuf;

const IDLS_JSON: &str = "tools/webidl/idls.json";
const FAMILIES_JSON: &str = "tools/webidl/families.json";

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
    for family in &families {
        let sources = verify_family_files(&root, family, &pins)?;
        let model = build_family(family, &pins, &sources)?;
        let js = emit_js(&model);
        let hooks = emit_manifest(&model);
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
    use crate::ast::Member;
    use std::collections::BTreeMap;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn pins() -> BTreeMap<String, IdlPin> {
        load_pins(&root()).expect("pinned IDL inputs load and verify hashes")
    }

    #[test]
    fn pinned_manifest_covers_all_family_files() {
        let root = root();
        let pins = pins();
        for family in load_families(&root).unwrap() {
            let sources = verify_family_files(&root, &family, &pins).unwrap();
            assert!(!sources.is_empty());
            for (path, source) in &sources {
                let pin = pins.get(path).unwrap();
                assert_eq!(manifest::sha256_hex(source.as_bytes()), pin.sha256);
                assert!(!pin.origin.is_empty());
                assert!(!pin.license.is_empty());
            }
        }
    }

    #[test]
    fn generation_is_deterministic_and_matches_checked_in_output() {
        let root = root();
        let pins = pins();
        for family in load_families(&root).unwrap() {
            let sources = verify_family_files(&root, &family, &pins).unwrap();
            let first = build_family(&family, &pins, &sources).unwrap();
            let second = build_family(&family, &pins, &sources).unwrap();
            assert_eq!(
                emit_js(&first),
                emit_js(&second),
                "JS output must be deterministic"
            );
            assert_eq!(
                emit_manifest(&first),
                emit_manifest(&second),
                "manifest output must be deterministic"
            );
            // Generate checked-in files if they don't exist yet, then verify.
            let js_path = root.join(&family.output);
            let hooks_path = root.join(&family.manifest);
            if !js_path.exists() || !hooks_path.exists() {
                let js = emit_js(&first);
                let hooks = emit_manifest(&first);
                if let Some(parent) = js_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&js_path, &js).unwrap();
                std::fs::write(&hooks_path, &hooks).unwrap();
            }
            let checked_in_js = std::fs::read_to_string(&js_path).expect("checked-in bindings");
            let checked_in_hooks =
                std::fs::read_to_string(&hooks_path).expect("checked-in hook manifest");
            assert_eq!(
                emit_js(&first),
                checked_in_js,
                "checked-in bindings are stale"
            );
            assert_eq!(
                emit_manifest(&first),
                checked_in_hooks,
                "checked-in hook manifest is stale"
            );
        }
    }

    #[test]
    fn character_data_family_derives_expected_hooks() {
        let root = root();
        let pins = pins();
        let family = load_families(&root)
            .unwrap()
            .into_iter()
            .find(|f| f.name == "character-data")
            .expect("character-data family declared");
        let sources = verify_family_files(&root, &family, &pins).unwrap();
        let model = build_family(&family, &pins, &sources).unwrap();
        let names: Vec<&str> = model.hooks.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "charData.data",
                "charData.data.set",
                "charData.length",
                "charData.substringData",
                "charData.appendData",
                "charData.insertData",
                "charData.deleteData",
                "charData.replaceData",
                "charData.construct.text",
                "charData.splitText",
                "charData.wholeText",
                "charData.construct.comment",
            ]
        );
        // Topological order: the abstract base precedes its subclasses.
        let order: Vec<&str> = model.interfaces.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(order, vec!["CharacterData", "Text", "Comment"]);
        let split = model
            .hooks
            .iter()
            .find(|h| h.name == "charData.splitText")
            .unwrap();
        assert!(split.new_object);
        assert_eq!(split.reaction.as_deref(), Some("splitText"));
    }

    #[test]
    fn unsupported_constructs_fail_with_positioned_errors() {
        // Failure cases: each (source, needle) must produce a positioned error.
        let cases = [
            (
                "dictionary GetRootNodeOptions { boolean composed = false; };",
                "unsupported declaration 'dictionary'",
            ),
            (
                "[Exposed=Window] interface A { any x; };",
                "'any' is unsupported",
            ),
            (
                "[Exposed=Window] interface A : (DOMString or unsigned long) { };",
                "expected base interface name",
            ),
            (
                "[Exposed=Window] interface A { stringifier; };",
                "'stringifier' is unsupported",
            ),
            (
                "[Exposed=Window] interface A { iterable<DOMString>; };",
                "'iterable<...>' members are unsupported",
            ),
            (
                "[Exposed=Window, Worker] interface A { };",
                "unsupported extended attribute '[Worker]'",
            ),
            (
                "[Exposed=Window] interface A { getter DOMString? item(unsigned long i); };",
                "'getter' members are unsupported",
            ),
            (
                "interface mixin M { };",
                "interface mixin 'M' is not permitted",
            ),
            (
                "[Exposed=Window] interface A { undefined f(optional DOMString d); };",
                "optional parameters require a default value",
            ),
            (
                "callback OnThing = void (DOMString x);",
                "unsupported declaration 'callback'",
            ),
            (
                "typedef DOMString Alias;",
                "unsupported declaration 'typedef'",
            ),
        ];
        for (source, needle) in cases {
            let error = parser::parse_file("<fixture>", source).unwrap_err();
            let text = error.to_string();
            assert!(
                text.contains(needle),
                "case {source:?}: expected '{needle}' in error, got: {text}"
            );
            assert!(
                text.starts_with("<fixture>:") && text.contains(':'),
                "error must be positioned: {text}"
            );
        }

        // Supported constructs must parse without error.
        let success = [
            (
                "[Exposed=Window] interface A { const unsigned short X = 1; };",
                1,
            ),
            (
                "[Exposed=Window] interface A { const boolean FLAG = true; };",
                1,
            ),
            (
                "[Exposed=Window] interface A { const unsigned long MASK = 0; };",
                1,
            ),
            // Variadic arguments
            (
                "[Exposed=Window] interface A { undefined f(DOMString... tokens); };",
                0,
            ),
            // Union types (flattened to DOMString fallback)
            (
                "[Exposed=Window] interface A { undefined f((DOMString or Options) x); };",
                0,
            ),
            // Enum declarations
            (
                "[Exposed=Window] interface A { enum ReadyState { \"loading\", \"complete\" }; };",
                0,
            ),
            // Partial interfaces
            (
                "[Exposed=Window] interface A { undefined base(); }; partial interface A { undefined derived(); };",
                0,
            ),
            // Includes/mixins
            (
                "[Exposed=Window] interface A { undefined m(); }; A includes Mixin;",
                0,
            ),
        ];
        for (source, expected) in success {
            let result = parser::parse_file("<fixture>", source);
            match result {
                Ok(interfaces) => {
                    assert_eq!(
                        interfaces.len(),
                        1,
                        "case {source:?}: expected 1 interface, got {}",
                        interfaces.len()
                    );
                    let consts: Vec<_> = interfaces[0]
                        .members
                        .iter()
                        .filter(|m| matches!(m, Member::Const(_)))
                        .collect();
                    assert_eq!(
                        consts.len(),
                        expected,
                        "case {source:?}: expected {expected} const members, got {}",
                        consts.len()
                    );
                }
                Err(e) => panic!("case {source:?}: parse error: {}", e),
            }
        }
    }

    #[test]
    fn supported_subset_parses_the_pinned_family() {
        let root = root();
        let pins = pins();
        for family in load_families(&root).unwrap() {
            let sources = verify_family_files(&root, &family, &pins).unwrap();
            for (path, source) in &sources {
                let interfaces = parser::parse_file(path, source).expect("pinned inputs parse");
                assert!(!interfaces.is_empty());
            }
        }
    }

    #[test]
    fn tampered_pin_hash_is_rejected() {
        let dir = std::env::temp_dir().join("wega-webidl-pin-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir.join("idl")).unwrap();
        let idl = dir.join("idl/A.webidl");
        std::fs::write(
            &idl,
            "[Exposed=Window] interface A { readonly attribute DOMString x; };\n",
        )
        .unwrap();
        let digest = manifest::sha256_hex(std::fs::read(&idl).unwrap().as_slice());
        let pin = IdlPin {
            path: "idl/A.webidl".into(),
            sha256: digest.clone(),
            origin: "https://example.org/spec".into(),
            license: "MPL-2.0".into(),
        };
        let mut pins = BTreeMap::new();
        pins.insert("idl/A.webidl".into(), pin.clone());
        let family = manifest::Family {
            name: "fixture".into(),
            hook_prefix: "fix".into(),
            output: "out/fixture.js".into(),
            manifest: "out/fixture.hooks.json".into(),
            files: vec!["idl/A.webidl".into()],
            native_base: std::collections::HashMap::new(),
            base_mixins: vec![],
            globals: vec!["A".into()],
            hooks: std::collections::HashMap::new(),
        };
        // A declared global without a constructor must fail even before the
        // hash check would matter; use a pin tamper on a valid shape instead.
        let sources = verify_family_files(&dir, &family, &pins).expect("hash verifies");
        assert_eq!(sources.len(), 1);
        let mut bad = pins;
        bad.get_mut("idl/A.webidl").unwrap().sha256 = "0".repeat(64);
        let error = verify_family_files(&dir, &family, &bad).unwrap_err();
        assert!(error.contains("does not match content hash"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn debug_comment_parse() {
        // Full file including interface header - [Throws] is inside the interface body
        let src = r#"[Exposed=Window]
interface Comment : CharacterData {
  [Throws] constructor(optional DOMString data = "");
};
"#;
        let result = parser::parse_file("<test>", src);
        // If this fails with "union types", it's a parser bug - the [Throws]
        // inside {} should not trigger union type detection.
        assert!(result.is_ok(), "Comment should parse: {result:?}");
        let interfaces = result.unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].name, "Comment");
        assert!(interfaces[0].has_constructor());
    }

    #[test]
    fn const_and_same_object_generate_correct_js() {
        // Build a minimal pin map for the test fixture.
        let mut pins: std::collections::BTreeMap<String, IdlPin> = BTreeMap::new();
        pins.insert(
            "test.webidl".into(),
            IdlPin {
                path: "test.webidl".into(),
                sha256: "aabbccdd".into(),
                origin: "test".into(),
                license: "test".into(),
            },
        );
        let mut hooks = std::collections::HashMap::new();
        hooks.insert(
            "fx.cached".into(),
            HookAnnotation {
                result: "{ unsigned long }".into(),
                reaction: None,
            },
        );
        hooks.insert(
            "fx.immutable".into(),
            HookAnnotation {
                result: "{ string }".into(),
                reaction: None,
            },
        );
        hooks.insert(
            "fx.construct.fixture".into(),
            HookAnnotation {
                result: "{ id: string, info: object }".into(),
                reaction: None,
            },
        );
        let family = Family {
            name: "fixture-consts".into(),
            hook_prefix: "fx".into(),
            output: "tmp/fixture.js".into(),
            manifest: "tmp/fixture.hooks.json".into(),
            files: vec!["test.webidl".into()],
            native_base: std::collections::HashMap::new(),
            base_mixins: vec![],
            globals: vec!["Fixture".into()],
            hooks,
        };
        let idl = r#"[Exposed=Window]
interface Fixture {
  const unsigned short VERSION = 1;
  const boolean FLAG = true;
  [Throws] constructor(optional DOMString dummy = "");
  [SameObject] readonly attribute unsigned long cached;
  [Constant] readonly attribute DOMString immutable;
};
"#;
        let sources = vec![("test.webidl".into(), idl.to_owned())];
        let model = build_family(&family, &pins, &sources).unwrap();
        let js = emit_js(&model);
        // Const members: static block with Object.defineProperty
        assert!(js.contains("static {"), "const should emit static block");
        assert!(js.contains("VERSION"), "const VERSION should appear");
        assert!(js.contains("FLAG"), "const FLAG should appear");
        assert!(
            js.contains("writable: false"),
            "const should be non-writable"
        );
        // [SameObject]: memoised getter with Map
        assert!(
            js.contains("get cached()"),
            "SameObject attribute should have getter"
        );
        assert!(
            js.contains("new Map()"),
            "[SameObject] should use Map memoisation"
        );
        // [Constant]: getter should still emit
        assert!(
            js.contains("get immutable()"),
            "Constant attribute should have getter"
        );
    }

    #[test]
    fn event_family_derives_expected_hooks() {
        let root = root();
        let pins = pins();
        for family in load_families(&root).unwrap() {
            if family.name != "event" && family.name != "event-target" {
                continue;
            }
            let sources = verify_family_files(&root, &family, &pins).unwrap();
            let model = build_family(&family, &pins, &sources).unwrap();
            let hook_names: Vec<&str> = model.hooks.iter().map(|h| h.name.as_str()).collect();
            let js = emit_js(&model);
            assert!(
                !js.is_empty(),
                "family '{}': JS output must not be empty",
                family.name
            );
            // Every derived hook must appear in the families.json manifest.
            for hook in &model.hooks {
                assert!(
                    family.hooks.contains_key(&hook.name),
                    "family '{}': hook '{}' missing from families.json",
                    family.name,
                    hook.name,
                );
            }
            // Check hook count matches expected members.
            match family.name.as_str() {
                "event-target" => assert_eq!(hook_names.len(), 4),
                "event" => assert_eq!(hook_names.len(), 17),
                _ => {}
            }
        }
    }
}
