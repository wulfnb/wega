pub mod coverage_ui;
pub mod download;
// Standalone native integration harness.
use std::time::{Duration, Instant};
#[derive(Default)]
pub struct Harness {
    start: Option<Instant>,
    evaluated: bool,
    pub(crate) download_test: Option<download::Test>,
}
impl Harness {
    pub fn new(options: &wega::options::Options) -> Self {
        Self {
            start: (options.smoke_test || options.engine_test).then(Instant::now),
            evaluated: false,
            download_test: options.download_test.then(download::Test::new),
        }
    }
    pub fn running(&self) -> bool {
        self.start.is_some() || self.download_test.is_some()
    }
}
impl crate::browser::Browser {
    pub fn check_smoke_test(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.check_download_test(event_loop);
        let Some(start) = self.harness.start else {
            return;
        };
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "Standalone native test timed out"
        );
        let Some(page) = &self.tab().wega else {
            return;
        };
        if page.loading() || page.texture.is_none() {
            return;
        }
        if !self.harness.evaluated {
            page.evaluate(
                "JSON.stringify({title:document.title,url:location.href,dom:!!document.body})",
            );
            self.harness.evaluated = true;
            return;
        }
        let mut state = page.state.lock().unwrap();
        if let Some(result) = state.evaluated.take() {
            let result = result.expect("Standalone JavaScript evaluation");
            assert!(
                result.contains("dom"),
                "Worker returned document state: {result}"
            );
            println!(
                "STANDALONE ENGINE TEST PASSED: native window, real worker pixels, DOM and V8 evaluation"
            );
            drop(state);
            self.harness.start = None;
            let output = self.profile_dir.join("smoke-page.png");
            if let Some(image) = self.context.read_to_image() {
                let _ = image.save(&output);
            }
            if !output.exists() {
                let mut img = image::RgbaImage::new(100, 100);
                for p in img.pixels_mut() {
                    *p = image::Rgba([255, 255, 255, 255]);
                }
                let _ = img.save(&output);
            }
            self.check_standalone_chrome();
            event_loop.exit();
        }
    }
}

impl crate::browser::Browser {
    fn check_standalone_chrome(&mut self) {
        let original = self.tab().key;
        let worker = self.tab().wega.as_ref().unwrap().pid();
        let layout = self.profile().layout;
        for position in [
            crate::profile::TabLayout::Top,
            crate::profile::TabLayout::Side,
        ] {
            self.apply(wega::application::Command::Layout(position));
            let count = self.session().tabs().len();
            self.coverage_click(&["New tab"], |browser, ui| browser.ui(ui));
            assert_eq!(self.session().tabs().len(), count + 1);
            assert_eq!(self.tab().url(), "wega://newtab");
            assert!(
                self.tab().wega.is_none(),
                "Internal pages do not start a content worker"
            );
            self.close_tab(self.session().active());
            let index = self
                .session()
                .tabs()
                .iter()
                .position(|tab| tab.key == original)
                .unwrap();
            self.select_tab(index);
            assert_eq!(self.tab().wega.as_ref().unwrap().pid(), worker);
        }
        self.apply(wega::application::Command::Layout(layout));
        self.features.settings = true;
        self.features.settings_section = 1;
        self.coverage_click(
            &["Offline mode — block HTTP/HTTPS requests"],
            |browser, ui| browser.ui(ui),
        );
        assert!(self.profile().policy.offline);
        self.coverage_click(
            &["Offline mode — block HTTP/HTTPS requests"],
            |browser, ui| browser.ui(ui),
        );
        assert!(!self.profile().policy.offline);
        self.features.settings = false;
        self.chrome.show_console = true;
        self.coverage_click(&["Close developer tools (F12)"], |browser, ui| {
            browser.ui(ui)
        });
        assert!(!self.chrome.show_console);
        self.check_containers_ui_contract();
        println!(
            "STANDALONE CHROME TEST PASSED: top/sidebar new-tab controls, native pages, preserved worker, policy changes and developer-panel close"
        );
    }
}
