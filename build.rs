fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icons/BrowserIcon.ico");
    // fontsan/ots uses C++ standard algorithms whose MSVC vectorized helpers reside in msvcprt.lib.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!("cargo:rustc-link-lib=msvcprt");
        }
        #[cfg(windows)]
        winresource::WindowsResource::new()
            .set_icon("assets/icons/BrowserIcon.ico")
            .compile()
            .expect("embed the Windows icon resource");
    }
}
