//! Startup snapshot generation (called from the embedder's build script).
use crate::{native_ops, web_extensions};
use deno_core::{JsRuntimeForSnapshot, RuntimeOptions, v8};

/// One trusted bootstrap factory compiled into the snapshot as
/// `globalThis.__wega_bootstrap_<key>`.
pub struct SnapshotScript<'a> {
    pub key: &'a str,
    /// Used for the script origin: `ext:wega_dom/<file>`.
    pub file: &'a str,
    pub source: &'a str,
}

/// Builds the startup snapshot with the same extension order every restored runtime uses.
/// Panics on invalid trusted sources (build-time only).
pub fn build_startup_snapshot(scripts: &[SnapshotScript<'_>]) -> Vec<u8> {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = executor.enter();
    let mut extensions = web_extensions::extensions(url::Url::parse("about:blank").unwrap(), None);
    extensions.push(native_ops::wega_dom::init());
    let mut runtime = JsRuntimeForSnapshot::new(RuntimeOptions {
        extensions,
        extension_transpiler: Some(std::rc::Rc::new(crate::extension_adapter::adapt_extension)),
        ..Default::default()
    });
    for script in scripts {
        deno_core::scope!(scope, &mut runtime);
        let code = v8::String::new(scope, script.source).unwrap();
        let name = v8::String::new(scope, &format!("ext:wega_dom/{}", script.file)).unwrap();
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
        let key = v8::String::new(scope, &format!("__wega_bootstrap_{}", script.key)).unwrap();
        assert_eq!(global.set(scope, key.into(), function.into()), Some(true));
    }
    runtime.snapshot().to_vec()
}
