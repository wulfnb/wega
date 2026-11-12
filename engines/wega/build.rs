//! Compile trusted bootstrap factories into a V8 snapshot without profile/DOM data.
use std::path::PathBuf;
use wega_adapter_v8::{SnapshotScript, build_startup_snapshot};
#[path = "../../tools/webidl/ast.rs"]
#[allow(dead_code)]
mod ast;
#[path = "../../tools/webidl/generate.rs"]
#[allow(dead_code)]
mod generate;
#[path = "../../tools/webidl/manifest.rs"]
mod manifest;
#[path = "../../tools/webidl/parser.rs"]
#[allow(dead_code)]
mod parser;

fn generated_bindings() -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pins = manifest::load_pins(&root).expect("WebIDL pins");
    let mut source = String::new();
    let mut registry = Vec::new();
    for family in manifest::load_families(&root).expect("WebIDL families") {
        let inputs =
            manifest::verify_family_files(&root, &family, &pins).expect("pinned WebIDL inputs");
        let model =
            generate::build_family(&family, &pins, &inputs).expect("valid typed WebIDL family");
        let js = generate::emit_js(&model);
        let hooks = generate::emit_manifest(&model);
        assert_eq!(
            std::fs::read_to_string(root.join(&family.output)).unwrap(),
            js,
            "stale generated bindings: run wega-webidl generate"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(&family.manifest)).unwrap(),
            hooks,
            "stale generated hooks: run wega-webidl generate"
        );
        let hooks: serde_json::Value = serde_json::from_str(&hooks).unwrap();
        registry.extend(hooks["hooks"].as_array().unwrap().iter().cloned());
        source.push_str(&js);
        for file in family
            .files
            .iter()
            .chain([&family.output, &family.manifest])
        {
            println!("cargo:rerun-if-changed={}", root.join(file).display());
        }
    }
    let checked: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("src/dom/generated/hooks.json")).unwrap();
    assert_eq!(registry, checked, "stale native hook registry");
    for file in [
        "tools/webidl/families.json",
        "tools/webidl/idls.json",
        "tools/webidl/ast.rs",
        "tools/webidl/parser.rs",
        "tools/webidl/generate.rs",
        "tools/webidl/manifest.rs",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(file).display());
    }
    source
}

fn main() {
    let generated = generated_bindings();
    // Trusted bootstrap factories, compiled into the startup snapshot by the V8 adapter.
    let mut scripts = Vec::new();
    for (key, file) in [
        ("web", "web.js"),
        ("bridge", "v8-bridge.js"),
        ("canvas", "canvas.js"),
        ("semantics", "semantics.js"),
        ("bindings", "bindings.js"),
        ("generated", "generated.js"),
        ("observers", "observers.js"),
        ("install", "install-web.js"),
        ("cache", "cache.js"),
        ("xhr", "xhr.js"),
        ("forms", "forms.js"),
        ("worker", "worker.js"),
    ] {
        let path = PathBuf::from("src/dom").join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let source = if key == "generated" {
            generated.clone()
        } else {
            std::fs::read_to_string(path).unwrap()
        };
        scripts.push((key, file, source));
    }
    let scripts: Vec<_> = scripts
        .iter()
        .map(|(key, file, source)| SnapshotScript { key, file, source })
        .collect();
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("wega-startup.bin");
    std::fs::write(output, build_startup_snapshot(&scripts)).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
