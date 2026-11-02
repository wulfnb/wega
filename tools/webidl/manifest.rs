//! Pinned IDL inputs and family configuration.
//!
//! `idls.json` pins every IDL file the pipeline may consume: path, sha256,
//! spec origin and license. `families.json` declares each generated family:
//! its files, native base class, base-provided mixins, output paths and the
//! per-hook semantic annotations (result shape and reaction).

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdlManifest {
    pub pins: Vec<IdlPin>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdlPin {
    pub path: String,
    pub sha256: String,
    pub origin: String,
    pub license: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamiliesFile {
    pub families: Vec<Family>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Family {
    pub name: String,
    pub hook_prefix: String,
    pub output: String,
    pub manifest: String,
    pub files: Vec<String>,
    /// Interface name -> JS base expression (e.g. Node -> dom.base.Node).
    pub native_base: HashMap<String, String>,
    /// Mixins whose members the native base already provides.
    pub base_mixins: Vec<String>,
    /// Interface names exposed as window globals (exposure-checked).
    pub globals: Vec<String>,
    /// Hook name -> semantic annotation. Every generated hook must be
    /// annotated; a missing annotation is a generation failure.
    pub hooks: HashMap<String, HookAnnotation>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HookAnnotation {
    /// Documented structured result of the hook (manifest text).
    pub result: String,
    /// Named JS-layer reaction run after the hook, or null.
    pub reaction: Option<String>,
}

pub struct LoadedFamily {
    pub family: Family,
    /// Parsed interfaces in family file order.
    pub interfaces: crate::ast::Interface,
    /// Resolved source text per family file.
    pub sources: Vec<(String, String)>,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("{path}: {e}"))
}

pub fn load_pins(root: &Path) -> Result<BTreeMap<String, IdlPin>, String> {
    let manifest = read_json::<IdlManifest>(root.join("tools/webidl/idls.json").to_str().unwrap())?;
    let mut pins = BTreeMap::new();
    for pin in manifest.pins {
        let path = pin.path.clone();
        if pins.insert(pin.path.clone(), pin).is_some() {
            return Err(format!("duplicate pin for {path}"));
        }
    }
    Ok(pins)
}

pub fn load_families(root: &Path) -> Result<Vec<Family>, String> {
    Ok(read_json::<FamiliesFile>(root.join("tools/webidl/families.json").to_str().unwrap())?.families)
}

/// Verify every pin referenced by `family` and read its IDL sources.
/// Returns (resolved paths, sources) in family file order.
pub fn verify_family_files(
    root: &Path,
    family: &Family,
    pins: &BTreeMap<String, IdlPin>,
) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for path in &family.files {
        let pin = pins
            .get(path)
            .ok_or_else(|| format!("{}: IDL file is not pinned in tools/webidl/idls.json", path))?;
        let file = root.join(path);
        let source = std::fs::read_to_string(&file)
            .map_err(|e| format!("{}: cannot read pinned IDL file: {e}", path))?;
        let digest = sha256_hex(source.as_bytes());
        if digest != pin.sha256 {
            return Err(format!(
                "{}: pinned sha256 {} does not match content hash {}",
                path, pin.sha256, digest
            ));
        }
        if pin.origin.is_empty() || pin.license.is_empty() {
            return Err(format!(
                "{}: pin is incomplete; every pinned IDL file needs origin and license",
                path
            ));
        }
        out.push((path.clone(), source));
    }
    Ok(out)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
