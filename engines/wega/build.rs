//! Compile trusted bootstrap factories into a V8 snapshot without profile/DOM data.
use deno_core::{JsRuntimeForSnapshot, RuntimeOptions, v8};
use std::path::PathBuf;
#[path = "src/extension_adapter.rs"]
mod extension_adapter;
#[path = "src/native_ops.rs"]
mod native_ops;
#[path = "src/web_extensions.rs"]
mod web_extensions;

fn main() {
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
        ("bindings", "bindings.js"),
        ("observers", "observers.js"),
        ("install", "install-web.js"),
        ("xhr", "xhr.js"),
        ("forms", "forms.js"),
    ] {
        let path = PathBuf::from("src/dom").join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let source = std::fs::read_to_string(path).unwrap();
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
