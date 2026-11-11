//! Compile trusted bootstrap factories into a V8 snapshot without profile/DOM data.
use deno_core::{JsRuntimeForSnapshot, RuntimeOptions, v8};
use std::path::PathBuf;
#[path = "src/extension_adapter.rs"]
mod extension_adapter;
#[path = "src/native_ops.rs"]
mod native_ops;
#[path = "src/web_extensions.rs"]
mod web_extensions;

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
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = executor.enter();
    let mut extensions = web_extensions::extensions(url::Url::parse("about:blank").unwrap(), None);
    extensions.push(native_ops::wega_dom::init());
    let mut runtime = JsRuntimeForSnapshot::new(RuntimeOptions {
        extensions,
        extension_transpiler: Some(std::rc::Rc::new(extension_adapter::adapt_extension)),
        ..Default::default()
    });
    for (key, file) in [
        ("web", "web.js"),
        ("bridge", "v8-bridge.js"),
        ("semantics", "semantics.js"),
        ("bindings", "bindings.js"),
        ("generated", "generated.js"),
        ("observers", "observers.js"),
        ("install", "install-web.js"),
        ("cache", "cache.js"),
        ("xhr", "xhr.js"),
        ("forms", "forms.js"),
    ] {
        let path = PathBuf::from("src/dom").join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let source = if key == "generated" {
            generated.clone()
        } else {
            std::fs::read_to_string(path).unwrap()
        };
        deno_core::scope!(scope, &mut runtime);
        let code = v8::String::new(scope, &source).unwrap();
        let name = v8::String::new(scope, &format!("ext:wega_dom/{file}")).unwrap();
        let origin = v8::ScriptOrigin::new(
            scope,
            name.into(),
            0,
            0,
            false,
            0,
            None,
            false,
            false,
            false,
            None,
        );
        let mut source = v8::script_compiler::Source::new(code, Some(&origin));
        let function = v8::script_compiler::compile_function(
            scope,
            &mut source,
            &[],
            &[],
            v8::script_compiler::CompileOptions::EagerCompile,
            v8::script_compiler::NoCacheReason::NoReason,
        )
        .expect("compile trusted binding factory");
        let global = scope.get_current_context().global(scope);
        let key = v8::String::new(scope, &format!("__wega_bootstrap_{key}")).unwrap();
        assert_eq!(global.set(scope, key.into(), function.into()), Some(true));
    }
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("wega-startup.bin");
    std::fs::write(output, runtime.snapshot()).unwrap();
    for file in [
        "build.rs",
        "src/native_ops.rs",
        "src/web_extensions.rs",
        "src/extension_adapter.rs",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
}
