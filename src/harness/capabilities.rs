//! Engine integration checks; isolated profile and local HTTP fixture only.
use crate::browser::Browser;
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
type ControlPoint = Rc<RefCell<Option<Result<[f64; 2], String>>>>;

pub struct Test {
    stage: u8,
    since: Instant,
    start: Instant,
    pending: bool,
    click_pending: bool,
    point: ControlPoint,
    fixture: String,
    pub frame_output: Option<std::path::PathBuf>,
    pub frame_checked: bool,
    result: Rc<RefCell<Option<Result<servo::JSValue, servo::JavaScriptEvaluationError>>>>,
}
impl Test {
    pub fn frame_ready(&self) -> bool {
        self.since.elapsed() > Duration::from_millis(500)
            && !self.frame_checked
            && self.frame_output.is_some()
    }
    pub fn new() -> Self {
        Self {
            stage: 0,
            since: Instant::now(),
            start: Instant::now(),
            pending: false,
            click_pending: false,
            point: Default::default(),
            fixture: String::new(),
            frame_output: None,
            frame_checked: false,
            result: Default::default(),
        }
    }
}
impl Browser {
    pub fn check_capabilities_test(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(test) = &self.harness.capabilities_test else {
            return;
        };
        assert!(
            test.start.elapsed() < Duration::from_secs(180),
            "Capabilities test timed out at {}",
            test.stage
        );
        let stage = test.stage;
        if stage == 0 {
            if self.view().load_status() != servo::LoadStatus::Complete
                || self.view().page_title().is_none()
            {
                return;
            }
            let script = Rc::new(servo::UserScript::new(
                "window.__wegaUserScript = 73".into(),
                None,
            ));
            self.content_manager
                .get()
                .unwrap()
                .add_script(script.clone());
            self.features.script = Some(script);
            let fixture = self
                .view()
                .url()
                .filter(|url| matches!(url.scheme(), "http" | "https"))
                .and_then(|url| url.join("capabilities.html").ok())
                .map(|url| url.to_string())
                .unwrap_or_else(|| "http://127.0.0.1:8767/capabilities.html".into());
            self.harness.capabilities_test.as_mut().unwrap().fixture = fixture.clone();
            self.navigate(&fixture);
            self.feature_stage(1);
            return;
        }
        if test.since.elapsed() < Duration::from_millis(500)
            || (self.view().load_status() != servo::LoadStatus::Complete
                && !matches!(stage, 56..=58))
        {
            return;
        }
        if stage == 1
            && (self.tab().url() != test.fixture
                || self.view().page_title().as_deref() != Some("Capability fixture"))
        {
            return;
        }
        // LoadStatus can still describe the previous document while a queued
        // navigation is starting. Wait for the destination before querying its DOM.
        if stage == 9 && self.view().page_title().as_deref() != Some("Request blocked") {
            assert!(
                test.since.elapsed() < Duration::from_secs(10),
                "Offline probe never reached its blocked document: URL={}, title={:?}",
                self.tab().url(),
                self.view().page_title()
            );
            return;
        }
        if stage == 11 && self.view().page_title().as_deref() != Some("History") {
            assert!(
                test.since.elapsed() < Duration::from_secs(10),
                "History navigation never reached its document: URL={}, title={:?}",
                self.tab().url(),
                self.view().page_title()
            );
            return;
        }
        if test.pending {
            let result = test.result.borrow_mut().take();
            let Some(result) = result else { return };
            if (stage == 9 || stage == 11)
                && matches!(result, Ok(servo::JSValue::Boolean(false)))
                && test.since.elapsed() < Duration::from_secs(10)
            {
                self.harness.capabilities_test.as_mut().unwrap().pending = false;
                return;
            }
            if matches!(stage, 19 | 22 | 25) {
                assert!(
                    self.chrome.dialog_key.is_none(),
                    "Completed prompt must restore the dialog focus lifecycle"
                );
            }
            assert!(
                matches!(result, Ok(servo::JSValue::Boolean(true))),
                "Stage {stage}: {result:?}"
            );
            self.feature_stage(stage + 1);
            return;
        }
        let source=match stage {
            1=>"(()=>{const values={script:window.__wegaUserScript,screen:screen.width,width:innerWidth,title:document.title};return values.script===73 && values.screen>=values.width && values.title==='Capability fixture' ? true : JSON.stringify(values)})()".to_owned(),
            2=>{self.view().evaluate_javascript("localStorage.setItem('wega-feature','yes');document.cookie='wega_feature=73;path=/'",|_|{});self.feature_stage(3);return;}
            3=>{self.refresh_site_data();assert!(self.features.cookies.iter().any(|c|c.starts_with("wega_feature=73")),"Engine cookie API");assert!(!self.features.sites.is_empty(),"Engine site data API");self.features.site_info=true;self.coverage_click(&["Site permissions","Block Camera"],|b,ui|b.ui(ui));let origin=self.view().url().unwrap().origin().ascii_serialization();assert!(self.profile().policy.permission_blocked(&origin,"Camera"));self.coverage_click(&["Site permissions","Block Camera"],|b,ui|b.ui(ui));assert!(!self.profile().policy.permission_blocked(&origin,"Camera"));self.coverage_click(&["Clear data for this site…"],|b,ui|b.ui(ui));assert!(self.features.sites.iter().any(|(site,_)|Some(site)==self.features.delete_site.as_ref()),"Current-site confirmation targets a Servo site name");self.coverage_click(&["Cancel"],|b,ui|b.ui(ui));assert!(self.features.delete_site.is_none());self.features.site_info=false;let names:Vec<_>=self.features.sites.iter().map(|s|s.0.clone()).collect();let names:Vec<_>=names.iter().map(String::as_str).collect();self.servo.site_data_manager().clear_site_data(&names,servo::StorageType::all());self.feature_stage(4);return;}
            4=>"localStorage.getItem('wega-feature')===null && !document.cookie.includes('wega_feature=')".to_owned(),
            5=>{let find=include_str!("../page_actions.js").replace("QUERY","\"needle\"").replace("BACKWARDS","false");format!("({find})==='1 of 2' && window.getSelection().toString()==='needle'")}
            6=>{self.view().evaluate_javascript(include_str!("../reader.js"),|_|{});self.feature_stage(7);return;}
            7=>"!!document.getElementById('__wega_reader') && document.getElementById('__wega_reader').textContent.includes('needle')".to_owned(),
            8=>{let fixture=test.fixture.clone();let mut policy=self.profile().policy.clone();policy.offline=true;self.apply(wega::application::Command::Policy(policy));let probe = url::Url::parse(&fixture).unwrap().join("offline-probe").unwrap();self.navigate(probe.as_str());self.feature_stage(9);return;}
            9=>"document.title==='Request blocked' && document.body.textContent.includes('Offline mode')".to_owned(),
            10=>{let mut policy=self.profile().policy.clone();policy.offline=false;self.apply(wega::application::Command::Policy(policy));self.navigate("wega://history");self.feature_stage(11);return;}
            11=>"document.title==='History' && document.querySelector('h1').textContent==='History'".to_owned(),
            12=>{self.screenshot_page();self.feature_stage(13);return;}
            13=>{let s=self.profile_dir.join("screenshots"); if !s.is_dir() || !s.read_dir().ok().into_iter().flatten().filter_map(Result::ok).any(|e|e.path().extension().is_some_and(|e|e=="png")){return;} self.view().adjust_pinch_zoom(1.5,servo::DevicePoint::new(100.0,100.0));self.feature_stage(14);return;}
            14=>{assert!(self.view().pinch_zoom()>1.0,"Engine pinch zoom");self.view().clear_session_history();self.save();self.features.settings=true;self.feature_stage(15);self.harness.capabilities_test.as_mut().unwrap().frame_output=Some(self.profile_dir.join("capabilities-settings.png"));return;}
            15=>{if !test.frame_checked{return;}self.features.settings=false;self.refresh_site_data();self.features.site_info=true;self.feature_stage(16);let test=self.harness.capabilities_test.as_mut().unwrap();test.frame_checked=false;test.frame_output=Some(self.profile_dir.join("capabilities-site-info.png"));return;}
            16=>{if !test.frame_checked{return;}self.features.site_info=false;let fixture=test.fixture.clone();self.navigate(&fixture);self.feature_stage(17);return;}
            17=>{self.view().evaluate_javascript("window.__uxPrompt='pending';setTimeout(()=>{window.__uxPrompt=prompt('Name','Alice')},0)",|_|{});self.feature_stage(18);return;}
            18=>{if self.dialogs.controls.is_empty() || self.chrome.prompt_text!="Alice"{return;}self.ux_test_key(egui::Key::Enter);self.feature_stage(19);return;}
            19=>"window.__uxPrompt==='Alice'".to_owned(),
            20=>{self.view().evaluate_javascript("window.__uxPrompt='pending';setTimeout(()=>{window.__uxPrompt=prompt('Name','Alice')},0)",|_|{});self.feature_stage(21);return;}
            21=>{if self.dialogs.controls.is_empty() || self.chrome.dialog_key.is_none(){return;}self.chrome.prompt_text.clear();self.ux_test_key(egui::Key::Enter);self.feature_stage(22);return;}
            22=>"window.__uxPrompt===''".to_owned(),
            23=>{self.view().evaluate_javascript("window.__uxPrompt='pending';setTimeout(()=>{window.__uxPrompt=prompt('Name','Alice')},0)",|_|{});self.feature_stage(24);return;}
            24=>{if self.dialogs.controls.is_empty(){return;}self.ux_test_key(egui::Key::Escape);self.feature_stage(25);return;}
            25=>"window.__uxPrompt===null".to_owned(),
            26=>{for _ in 0..20 {self.apply(wega::application::Command::Visit(crate::profile::Bookmark{title:"Frequent & useful".into(),url:"https://frequent.example.test/".into(),visited_at:crate::profile::visit_time(),visit_count:0}));}let ctx=self.gui.as_ref().unwrap().egui_ctx.clone();self.set_theme(&ctx,crate::theme::Theme::Paper);self.navigate("wega://newtab");self.feature_stage(27);return;}
            27=>{assert!(self.chrome.page_focused && !self.chrome.focus_address,"New tab routes typing to its search field");
                "(()=>{const ok=document.activeElement?.id==='search' && document.querySelector('nav')===null && document.querySelectorAll('.tiles .tile').length<=8 && document.querySelectorAll('.tiles .tile')[2].getAttribute('href')==='https://frequent.example.test/' && getComputedStyle(document.documentElement).backgroundColor==='rgb(251, 249, 244)' && (()=>{const i=document.querySelector('form input[name=q]').getBoundingClientRect(),b=document.querySelector('form button').getBoundingClientRect();return Math.abs(i.top-b.top)<1 && i.height===48 && b.height===48 && b.left-i.right>=8})();return ok?true:JSON.stringify({focus:document.activeElement?.id,value:document.getElementById('search')?.value,title:document.title})})()".to_owned()},
            28=>{if test.frame_output.as_ref().is_none_or(|path| !path.ends_with("capabilities-newtab.png")){let test=self.harness.capabilities_test.as_mut().unwrap();test.frame_output=Some(self.profile_dir.join("capabilities-newtab.png"));test.frame_checked=false;self.window.request_redraw();return;}
                if !test.frame_checked{return;}let _ = self.window.request_inner_size(winit::dpi::LogicalSize::new(720.0,480.0));self.chrome.show_history=true;self.feature_stage(29);return;}
            29=>{assert!(self.chrome.page_rect.width()>600.0,"Compact library must overlay rather than squeeze the page");assert!(self.chrome.library_rect.is_positive(),"Compact library must remain reachable");self.chrome.show_history=false;let ctx=self.gui.as_ref().unwrap().egui_ctx.clone();self.set_theme(&ctx,crate::theme::Theme::Emerald);self.feature_stage(30);return;}
            30=>"getComputedStyle(document.documentElement).backgroundColor==='rgb(6, 27, 24)'".to_owned(),
            31..=35=>{self.features.settings=true;self.features.settings_section=(stage-31) as usize;self.feature_stage(stage+1);return;}
            36=>{self.ux_test_key(egui::Key::Escape);self.features.diagnostics=true;self.feature_stage(37);return;}
            37=>{assert!(!self.features.settings,"Escape closes the settings sheet");self.features.diagnostics=false;self.features.clear_data=true;self.feature_stage(38);return;}
            38=>{self.ux_test_key(egui::Key::Escape);self.feature_stage(39);return;}
            39=>{assert!(!self.features.clear_data,"Escape cancels data deletion");self.tools.network_ui.import_open=true;self.tools.network_ui.composer=Some(crate::network::Composer::default());self.feature_stage(40);return;}
            40=>{self.tools.network_ui.import_open=false;self.tools.network_ui.composer=None;self.features.find_open=true;self.features.find="Wega".into();self.feature_stage(41);return;}
            41=>{self.features.find_open=false;self.accessibility_event(&egui_winit::accesskit_winit::Event{window_id:self.window.id(),window_event:egui_winit::accesskit_winit::WindowEvent::InitialTreeRequested});self.feature_stage(42);return;}
            42=>{if self.accessibility.grafted != Some(self.view().id()){return;}assert!(self.accessibility.active);self.accessibility_event(&egui_winit::accesskit_winit::Event{window_id:self.window.id(),window_event:egui_winit::accesskit_winit::WindowEvent::ActionRequested(accesskit::ActionRequest{action:accesskit::Action::Focus,target_tree:accesskit::TreeId::ROOT,target_node:egui::Id::unique("web-page-accessibility").accesskit_id(),data:None})});assert!(self.chrome.page_focused);self.chrome.show_console=true;self.apply(wega::application::Command::Dock(crate::tool_window::Dock::Window));self.sync_tool_window(event_loop);self.feature_stage(43);return;}
            43=>{let id=self.tool_window.as_ref().expect("Detached tool window").window.id();self.accessibility_event(&egui_winit::accesskit_winit::Event{window_id:id,window_event:egui_winit::accesskit_winit::WindowEvent::InitialTreeRequested});self.feature_stage(44);return;}
            44=>{let id=self.tool_window.as_ref().unwrap().window.id();for window_event in [egui_winit::accesskit_winit::WindowEvent::ActionRequested(accesskit::ActionRequest{action:accesskit::Action::Focus,target_tree:accesskit::TreeId::ROOT,target_node:accesskit::NodeId(0),data:None}),egui_winit::accesskit_winit::WindowEvent::AccessibilityDeactivated]{self.accessibility_event(&egui_winit::accesskit_winit::Event{window_id:id,window_event});}self.apply(wega::application::Command::Dock(crate::tool_window::Dock::Bottom));self.sync_tool_window(event_loop);self.chrome.show_console=false;self.feature_stage(45);return;}
            45=>{self.accessibility_event(&egui_winit::accesskit_winit::Event{window_id:self.window.id(),window_event:egui_winit::accesskit_winit::WindowEvent::AccessibilityDeactivated});assert!(!self.accessibility.active);assert!(self.accessibility.pending.is_empty());let mut cookies=crate::cookie_inspector::State::default();cookies.refresh(self.view(),&self.servo);assert!(cookies.error.is_some(),"Internal URLs cannot inspect HTTP cookies");self.feature_stage(46);return;}
            46=>{let fixture=test.fixture.clone();self.check_native_ui_contracts(&fixture);self.feature_stage(47);return;}
            47=>{let fixture=test.fixture.clone();self.navigate(&fixture);self.feature_stage(48);return;}
            48=>{let origin=self.view().url().unwrap().origin().ascii_serialization();let mut policy=self.profile().policy.clone();policy.set_permission_blocked(&origin,"Notifications",true);self.apply(wega::application::Command::Policy(policy));"window.__permissionResult='pending';setTimeout(()=>{Notification.requestPermission().then(value=>{window.__permissionResult=value})},0);true".to_owned()},
            49=>{assert!(self.pending_permission().is_none(),"Remembered block denies without prompting");self.feature_stage(50);return;}
            50=>"window.__permissionResult==='denied'".to_owned(),
            51=>{let fixture=test.fixture.replace("127.0.0.1","localhost");self.navigate(&fixture);self.feature_stage(52);return;}
            52=>"window.__permissionResult='pending';setTimeout(()=>{Notification.requestPermission().then(value=>{window.__permissionResult=value})},0);true".to_owned(),
            53=>{if self.pending_permission().is_none(){return;}self.coverage_click(&["Allow for this page"],|b,ui|b.ui(ui));assert!(self.pending_permission().is_none());self.feature_stage(54);return;}
            54=>"window.__permissionResult==='granted'".to_owned(),
            55=>{let url=test.fixture.replace("/capabilities.html","/auth-fixture-cancel");self.navigate(&url);self.feature_stage(56);return;}
            56=>{if self.dialogs.authentications.is_empty(){return;}self.coverage_click(&["Cancel"],|b,ui|b.ui(ui));assert!(self.dialogs.authentications.is_empty());assert!(self.chrome.password.is_empty());self.feature_stage(57);return;}
            57=>{let url=test.fixture.replace("/capabilities.html","/auth-fixture-sign-in");self.navigate(&url);self.feature_stage(58);return;}
            58=>{if self.dialogs.authentications.is_empty(){return;}self.chrome.username="fixture".into();self.chrome.password="test-pass".into();self.coverage_click(&["Sign in"],|b,ui|b.ui(ui));assert!(self.dialogs.authentications.is_empty());assert!(self.chrome.username.is_empty() && self.chrome.password.is_empty());self.feature_stage(59);return;}
            59=>"document.body.textContent.includes('authenticated') && document.body.textContent.includes('true')".to_owned(),
            60=>{let view=self.view();view.adjust_pinch_zoom(1.0/view.pinch_zoom(),servo::DevicePoint::new(0.0,0.0));self.feature_stage(61);return;}
            61=>r#"document.body.innerHTML=`<button id='upload-button' style='position:fixed;left:20px;top:20px;width:200px;height:60px' onclick='document.getElementById("upload").click()'>Choose fixture file</button><input id='upload' type='file' accept='.txt' hidden>`;true"#.to_owned(),
            62=>{if !self.feature_click("#upload-button"){return;}self.feature_stage(63);return;}
            63=>{if !self.dialogs.controls.iter().any(|(_,c)|matches!(c,servo::EmbedderControl::FilePicker(_))){return;}self.chrome.file_paths="relative.txt".into();self.coverage_click(&["Choose"],|b,ui|b.ui(ui));assert!(self.chrome.file_error.is_some());assert!(!self.dialogs.controls.is_empty());let path=self.profile_dir.join("coverage upload.txt");std::fs::write(&path,b"fixture file bytes").unwrap();if std::env::var_os("WEGA_INTERACTIVE_COVERAGE").is_some(){println!("MANUAL FILE PICKER: {}",path.display());self.coverage_click(&["Browse files…"],|b,ui|b.ui(ui));assert!(self.chrome.file_paths.contains("coverage upload.txt"));self.coverage_click(&["Choose"],|b,ui|b.ui(ui));}else{
                let directory=self.profile_dir.join("chooser-ipc");std::fs::create_dir_all(&directory).unwrap();
                self.chrome.file_handoff=Some(crate::file_handoff::Request::new(&directory,&self.profile_dir,false,"https://fixture.test".into(),vec!["txt".into()]).unwrap());
                let manifest:serde_json::Value=serde_json::from_slice(&std::fs::read(directory.join("current.json")).unwrap()).unwrap();
                let id=manifest["id"].as_str().unwrap();std::fs::write(directory.join(format!("reply-{id}.json")),serde_json::json!({"id":id,"paths":[path]}).to_string()).unwrap();
                self.coverage_click(&[""],|b,ui|b.ui(ui));assert!(!directory.join("current.json").exists(),"Picker consumes and retires the viewer request");
                }assert!(self.dialogs.controls.is_empty());assert!(self.chrome.dialog_key.is_none());self.feature_stage(64);return;}
            64=>"document.getElementById('upload').files[0].name==='coverage upload.txt'".to_owned(),
            65=>"document.body.innerHTML=`<select id='choice' style='position:fixed;left:20px;top:20px;width:200px;height:60px'><optgroup label='Letters'><option value='a'>Alpha</option><option value='b'>Beta</option><option disabled>Unavailable</option></optgroup></select>`;true".to_owned(),
            66=>{if !self.feature_click("#choice"){return;}self.feature_stage(67);return;}
            67=>{if !self.dialogs.controls.iter().any(|(_,c)|matches!(c,servo::EmbedderControl::SelectElement(_))){return;}self.coverage_click(&["Beta"],|b,ui|b.ui(ui));assert!(self.dialogs.controls.is_empty());assert!(self.chrome.dialog_key.is_none());self.feature_stage(68);return;}
            68=>"document.getElementById('choice').value==='b'".to_owned(),
            69=>{if !self.feature_click("#choice"){return;}self.feature_stage(70);return;}
            70=>{if self.dialogs.controls.is_empty(){return;}self.ux_test_key(egui::Key::Escape);self.feature_stage(71);return;}
            71=>"document.getElementById('choice').value==='b'".to_owned(),
            72=>"document.body.innerHTML=`<input id='color' type='color' value='#123456' style='position:fixed;left:20px;top:20px;width:200px;height:60px'>`;true".to_owned(),
            73=>{if !self.feature_click("#color"){return;}self.feature_stage(74);return;}
            74=>{if !self.dialogs.controls.iter().any(|(_,c)|matches!(c,servo::EmbedderControl::ColorPicker(_))){return;}self.coverage_click(&["Done"],|b,ui|b.ui(ui));assert!(self.dialogs.controls.is_empty());assert!(self.chrome.dialog_key.is_none());self.feature_stage(75);return;}
            75=>"document.getElementById('color').value==='#123456'".to_owned(),
            76=>{if !self.feature_click("#color"){return;}self.feature_stage(77);return;}
            77=>{if self.dialogs.controls.is_empty(){return;}
                if let Some((_,servo::EmbedderControl::ColorPicker(picker)))=self.dialogs.controls.front_mut(){picker.select(Some(servo::RgbColor{red:255,green:0,blue:0}));}self.ux_test_key(egui::Key::Escape);self.feature_stage(78);return;}
            78=>"document.getElementById('color').value==='#123456'".to_owned(),
            79=>r#"document.body.innerHTML=`<button id='upload-button' style='position:fixed;left:20px;top:20px;width:200px;height:60px' onclick='document.getElementById("upload").click()'>Choose fixture file</button><input id='upload' type='file' hidden>`;true"#.to_owned(),
            80=>{if !self.feature_click("#upload-button"){return;}self.feature_stage(81);return;}
            81=>{if self.dialogs.controls.is_empty(){return;}self.coverage_click(&["Cancel"],|b,ui|b.ui(ui));assert!(self.dialogs.controls.is_empty());assert!(self.chrome.file_paths.is_empty());assert!(self.chrome.dialog_key.is_none());self.feature_stage(82);return;}
            82=>"document.getElementById('upload').files.length===0".to_owned(),
            83=>{self.new_tab();self.feature_stage(84);return;}
            84=>{assert!(self.chrome.page_focused && !self.chrome.focus_address);
                for character in "typed search".chars() {
                    for state in [servo::KeyState::Down,servo::KeyState::Up] {
                        self.view().notify_input_event(servo::InputEvent::Keyboard(servo::KeyboardEvent::from_state_and_key(state,servo::Key::Character(character.to_string()))));
                    }
                }
                self.feature_stage(85);return;}
            85=>"document.activeElement?.id==='search' && document.getElementById('search').value==='typed search'".to_owned(),
            86=>{println!("CAPABILITIES TEST PASSED: engine policies, site data, prompt/focus semantics, settings, themes, native accessibility, permissions, HTTP authentication, validated file selection and select/color controls");self.harness.capabilities_test=None;event_loop.exit();return;}
            _=>return,
        };
        let result = self
            .harness
            .capabilities_test
            .as_ref()
            .unwrap()
            .result
            .clone();
        self.harness.capabilities_test.as_mut().unwrap().pending = true;
        self.view()
            .evaluate_javascript(source, move |r| *result.borrow_mut() = Some(r));
    }
    fn feature_click(&mut self, selector: &str) -> bool {
        let point = self
            .harness
            .capabilities_test
            .as_ref()
            .unwrap()
            .point
            .borrow_mut()
            .take();
        if let Some(point) = point {
            let [x, y] = point.expect("Page control must have a measured position");
            self.harness
                .capabilities_test
                .as_mut()
                .unwrap()
                .click_pending = false;
            self.smoke_click(x, y);
            return true;
        }
        let test = self.harness.capabilities_test.as_mut().unwrap();
        if !test.click_pending {
            test.click_pending = true;
            let point = test.point.clone();
            let selector = serde_json::to_string(selector).unwrap();
            self.view().evaluate_javascript(format!("(()=>{{const r=document.querySelector({selector}).getBoundingClientRect();return JSON.stringify([r.x+r.width/2,r.y+r.height/2])}})()"),move|result|{
                *point.borrow_mut()=Some(match result {Ok(servo::JSValue::String(text))=>serde_json::from_str(&text).map_err(|e|e.to_string()),other=>Err(format!("Control position query failed: {other:?}"))});
            });
        }
        false
    }
    fn ux_test_key(&mut self, key: egui::Key) {
        let gui = self.gui.as_mut().unwrap();
        for pressed in [true, false] {
            gui.egui_winit
                .egui_input_mut()
                .events
                .push(egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
        }
        self.window.request_redraw();
    }
    fn feature_stage(&mut self, stage: u8) {
        let test = self.harness.capabilities_test.as_mut().unwrap();
        test.stage = stage;
        test.since = Instant::now();
        test.pending = false;
        println!("CAPABILITIES stage {stage}");
    }
}
