//! Assemble/sign the content helper in Rust. No shell templating or inherited
//! browser sandbox/network entitlements are used for the helper.
use std::{io::Write, path::PathBuf, process::Command};
fn main() {
    if let Err(error) = run() {
        eprintln!("Content helper packaging failed: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    if !cfg!(target_os = "macos") {
        return Err("Content bundle packaging requires macOS".into());
    }
    let mut args = std::env::args_os().skip(1);
    let worker = PathBuf::from(args.next().ok_or("Expected worker executable")?).canonicalize()?;
    let app = PathBuf::from(args.next().ok_or("Expected browser app directory")?);
    if args.next().is_some() {
        return Err("Unexpected packaging arguments".into());
    }
    let helper = app.join("Contents/Helpers/WegaContent.app");
    let contents = helper.join("Contents");
    let macos = contents.join("MacOS");
    std::fs::create_dir_all(&macos)?;
    let destination = macos.join("wega-engine");
    let temporary = macos.join("wega-engine.new");
    std::fs::copy(worker, &temporary)?;
    std::fs::rename(temporary, &destination)?;
    std::fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleExecutable</key><string>wega-engine</string><key>CFBundleIdentifier</key><string>io.github.wulfnb.wega.content</string><key>CFBundleName</key><string>Wega Content</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>LSBackgroundOnly</key><true/><key>LSMinimumSystemVersion</key><string>12.0</string></dict></plist>
"#,
    )?;
    let mut entitlements = tempfile::NamedTempFile::new()?;
    entitlements.write_all(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>
"#.as_bytes(),
    )?;
    let identity = std::env::var_os("WEGA_CODESIGN_IDENTITY").unwrap_or_else(|| "-".into());
    let status = Command::new("/usr/bin/codesign")
        .args([
            "--force",
            "--options",
            "runtime",
            "--timestamp=none",
            "--entitlements",
        ])
        .arg(entitlements.path())
        .arg("--sign")
        .arg(identity)
        .arg(&helper)
        .status()?;
    if !status.success() {
        return Err("codesign rejected the content helper".into());
    }
    let verified = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(&helper)
        .status()?;
    if !verified.success() {
        return Err("Content helper signature verification failed".into());
    }
    println!("{}", destination.display());
    Ok(())
}
