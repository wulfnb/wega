//! Real-site regression gate. Each `fixtures/site_corpus/<site>.html` is a
//! frozen, script-free snapshot of a live site, with a Chrome rendering of the
//! same snapshot (`<site>-chrome.png` / `.json`). Both browsers render it
//! offline. Wega is scored on:
//! - geometry: the share of `body *` element boxes within tolerance of Chrome's
//! - sizes: the same, comparing only width and height
//! - pixels: the share of pixels close to Chrome's
//!
//! The test fails when either score falls below `scores.json`. Scores depend on the
//! installed system fonts, so they are enforced only where the font environment
//! matches the one they were recorded in (`environment` in `scores.json`); elsewhere,
//! such as CI runners, the scores are reported. `WEGA_SITE_SCORES_STRICT=1` enforces
//! them everywhere.
//!
//! Refresh snapshots (live network, Chrome required):
//!     node tests/support/site_snapshot.mjs <chrome> <tmp-profile> <url> tests/fixtures/site_corpus <site>
//! Raise the recorded scores after an improvement:
//!     WEGA_UPDATE_SITE_SCORES=1 cargo test -p wega-engine --test native_site_corpus
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url};

const SITES: &[&str] = &["example", "google", "hackernews", "wikipedia", "edudia"];
/// Must match `PROBE` in `support/site_snapshot.mjs`.
const PROBE: &str = "Array.from(document.body.querySelectorAll('*')).slice(0,800).map(e=>{const r=e.getBoundingClientRect();return [Math.round(r.x),Math.round(r.y),Math.round(r.width),Math.round(r.height)]})";
const SLACK: f64 = 0.01;

fn dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/site_corpus")
}

fn render(site: &str, width: u32, height: u32) -> (Vec<u8>, Vec<[i64; 4]>) {
    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(3),
            ..Limits::default()
        },
        NetworkPolicy {
            offline: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let url = Url::from_file_path(dir().join(format!("{site}.html"))).unwrap();
    let mut view = engine.create_view(url).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    view.poll();
    while view.loading && Instant::now() < deadline {
        view.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    view.paint(width, height, 1.0, 1.0, false).unwrap();
    let frame = view.paint(width, height, 1.0, 1.0, false).unwrap();
    let geometry = serde_json::from_str(&view.evaluate_json(PROBE).unwrap()).unwrap_or_default();
    (frame.rgba, geometry)
}

fn decode_png(path: &Path, width: u32, height: u32) -> Vec<u8> {
    let data = skia_safe::Data::new_copy(&std::fs::read(path).unwrap());
    let image = skia_safe::images::deferred_from_encoded_data(data, None).unwrap();
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    assert!(image.read_pixels(
        &info,
        &mut pixels,
        width as usize * 4,
        (0, 0),
        skia_safe::image::CachingHint::Disallow
    ));
    pixels
}

/// Boxes Chrome paints in the first two screens, matched by document order.
/// With `positions` false only width and height are compared, so one early
/// line-height difference does not mark every later box as wrong.
fn geometry_score(chrome: &[[i64; 4]], wega: &[[i64; 4]], height: i64, positions: bool) -> f64 {
    let mut considered = 0;
    let mut matched = 0;
    for (i, c) in chrome.iter().enumerate() {
        if c[2] <= 0 || c[3] <= 0 || c[1] >= height * 2 {
            continue;
        }
        considered += 1;
        let close = |a: i64, b: i64, size: i64| (a - b).abs() <= 4.max(size / 50);
        if wega.get(i).is_some_and(|w| {
            (!positions || (close(c[0], w[0], c[2]) && close(c[1], w[1], c[3])))
                && close(c[2], w[2], c[2])
                && close(c[3], w[3], c[3])
        }) {
            matched += 1;
        }
    }
    if considered == 0 {
        1.0
    } else {
        matched as f64 / considered as f64
    }
}

fn pixel_score(a: &[u8], b: &[u8]) -> f64 {
    let same = a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .filter(|(x, y)| (0..3).all(|c| x[c].abs_diff(y[c]) <= 48))
        .count();
    same as f64 / (a.len() / 4).max(1) as f64
}

/// The OS and a stable hash (FNV-1a) of the installed system font family names.
fn font_environment() -> String {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    let mut families: Vec<String> = db
        .faces()
        .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
        .collect();
    families.sort();
    families.dedup();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in families.join("\n").bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{}-{}-{hash:016x}", std::env::consts::OS, families.len())
}

#[test]
fn real_site_snapshots_do_not_regress_against_chrome() {
    let scores_path = dir().join("scores.json");
    let recorded: Value = std::fs::read_to_string(&scores_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    let update = std::env::var_os("WEGA_UPDATE_SITE_SCORES").is_some() || recorded == json!({});
    let environment = font_environment();
    let enforce = std::env::var_os("WEGA_SITE_SCORES_STRICT").is_some()
        || recorded["environment"].as_str() == Some(environment.as_str());
    let evidence = std::env::var_os("WEGA_NATIVE_EVIDENCE_DIR").map(std::path::PathBuf::from);
    let mut current = serde_json::Map::new();
    let mut regressions = Vec::new();
    for site in SITES {
        let reference: Value = serde_json::from_str(
            &std::fs::read_to_string(dir().join(format!("{site}-chrome.json"))).unwrap(),
        )
        .unwrap();
        let (width, height) = (
            reference["width"].as_u64().unwrap() as u32,
            reference["height"].as_u64().unwrap() as u32,
        );
        let chrome_geometry: Vec<[i64; 4]> =
            serde_json::from_value(reference["geometry"].clone()).unwrap();
        let (pixels, geometry) = render(site, width, height);
        if let Some(dir) = &evidence {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(
                dir.join(format!("site-{site}-wega.json")),
                serde_json::to_string(&geometry).unwrap(),
            );
            let info = skia_safe::ImageInfo::new(
                (width as i32, height as i32),
                skia_safe::ColorType::RGBA8888,
                skia_safe::AlphaType::Unpremul,
                None,
            );
            if let Some(png) = skia_safe::images::raster_from_data(
                &info,
                skia_safe::Data::new_copy(&pixels),
                width as usize * 4,
            )
            .and_then(|i| i.encode(None, skia_safe::EncodedImageFormat::PNG, None))
            {
                let _ = std::fs::write(dir.join(format!("site-{site}-wega.png")), png.as_bytes());
            }
        }
        let chrome_pixels = decode_png(&dir().join(format!("{site}-chrome.png")), width, height);
        let g = geometry_score(&chrome_geometry, &geometry, height as i64, true);
        let z = geometry_score(&chrome_geometry, &geometry, height as i64, false);
        let p = pixel_score(&pixels, &chrome_pixels);
        println!(
            "{site:<12} geometry {:>5.1}%  sizes {:>5.1}%  pixels {:>5.1}%",
            g * 100.0,
            z * 100.0,
            p * 100.0
        );
        let round = |v: f64| (v * 1000.0).round() / 1000.0;
        current.insert(
            site.to_string(),
            json!({"geometry": round(g), "sizes": round(z), "pixels": round(p)}),
        );
        for (metric, value) in [("geometry", g), ("sizes", z), ("pixels", p)] {
            if let Some(before) = recorded[site][metric].as_f64()
                && value + SLACK < before
            {
                regressions.push(format!(
                    "{site} {metric}: {value:.3} < recorded {before:.3}"
                ));
            }
        }
    }
    if update {
        current.insert("environment".into(), json!(environment));
        std::fs::write(
            &scores_path,
            serde_json::to_string_pretty(&Value::Object(current)).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
    if !enforce {
        println!(
            "site scores not enforced: fonts {environment} differ from the recorded {}; \
             set WEGA_SITE_SCORES_STRICT=1 to enforce. Differences:\n{}",
            recorded["environment"].as_str().unwrap_or("(none)"),
            regressions.join("\n")
        );
        return;
    }
    assert!(
        regressions.is_empty(),
        "site corpus regressed:\n{}",
        regressions.join("\n")
    );
}
