//! Actions and interchange controls for the native network monitor.
use crate::{browser::Browser, network, toolbox::pretty};
use egui::Ui;
use std::io::Read;
impl Browser {
    pub fn network_options(&mut self, ui: &mut Ui, owner: &str) {
        ui.menu_button("Options",|ui|{
            ui.checkbox(&mut self.tools.network.preserve,"Persist logs");
            ui.separator();
            if ui.button("Import HAR file…").clicked(){self.tools.network_ui.import_open=true;ui.close();}
            if ui.button("Save all as HAR…").clicked(){self.tools.export_path=self.profile_dir.join("network.har").display().to_string();self.tools.export_open=true;ui.close();}
            if ui.button("Copy all as HAR").clicked(){ui.ctx().copy_text(pretty(&self.tools.network.har(owner)));ui.close();}
            if ui.button("Host blocking / offline settings…").clicked(){self.features.settings=true;ui.close();}
            ui.separator();ui.label("Columns");
            ui.checkbox(&mut self.tools.network.show_initiator,"Initiator (when reported)");
            ui.checkbox(&mut self.tools.network.show_transferred,"Transferred size");
            ui.separator();ui.weak("WebSocket frame inspection and bandwidth limits are not exposed by this Servo build.");
        });
    }
    pub fn network_dialogs(&mut self, ctx: &egui::Context, owner: &str) {
        if self.tools.network_ui.import_open {
            let mut open = true;
            egui::Window::new("Import network HAR").open(&mut open).default_width(520.0).show(ctx,|ui|{
                ui.label("Import HAR 1.2 into this tab. Imported rows are marked and retain their recorded durations. Up to 500 requests and 16 MiB per file; bodies are limited to 64 KiB previews.");
                ui.horizontal(|ui|{
                    ui.add(egui::TextEdit::singleline(&mut self.tools.network_ui.import_path).hint_text("/path/to/network.har").desired_width((ui.available_width()-90.0).max(120.0)));
                    if ui.button("Choose…").clicked() && let Some(path)=rfd::FileDialog::new().add_filter("HAR",&["har","json"]).pick_file(){self.tools.network_ui.import_path=path.display().to_string();}
                });
                if let Some(error)=&self.tools.error {crate::components::inline_message(ui,crate::ux::Severity::Error,error);}
                if ui.button("Import").clicked(){
                    let result=(||{
                        let file=std::fs::File::open(&self.tools.network_ui.import_path).map_err(|e|e.to_string())?;
                        let mut bytes=Vec::new();file.take(16*1024*1024+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
                        self.tools.network.import_har(owner,&bytes)
                    })();
                    match result {Ok(count)=>{self.tools.notice=Some(format!("Imported {count} HAR requests"));self.tools.error=None;self.tools.network_ui.import_open=false;},Err(e)=>self.tools.error=Some(format!("Could not import the HAR file: {e}"))}
                }
            });
            if !open {
                self.tools.network_ui.import_open = false;
            }
        }
        if self.tools.network_ui.composer.is_some() {
            let mut open = true;
            let mut send = false;
            egui::Window::new("Edit and send request").open(&mut open).default_width(600.0).show(ctx,|ui|{
                let c=self.tools.network_ui.composer.as_mut().unwrap();
                ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut c.method).desired_width(70.0));ui.add(egui::TextEdit::singleline(&mut c.url).hint_text("https://example.com/api").desired_width(f32::INFINITY));});
                ui.label("Headers — Name: value, one per line");ui.add(egui::TextEdit::multiline(&mut c.headers).code_editor().desired_rows(4).desired_width(f32::INFINITY));
                ui.label("Request body");ui.add(egui::TextEdit::multiline(&mut c.body).code_editor().desired_rows(6).desired_width(f32::INFINITY));
                ui.weak("Sends using the active page’s fetch API. Same-origin credentials and CORS apply; forbidden headers are controlled by the engine. Sending may change data on the server.");
                send=ui.button("Send request").clicked();
            });
            if send {
                match self.tools.network_ui.composer.as_ref().unwrap().script() {
                    Ok(script) => {
                        self.view().evaluate_javascript(script, |_| {});
                        self.tools.notice=Some("Request sent through page fetch; inspect Network and Console for the result.".into());
                    }
                    Err(e) => self.tools.error = Some(e),
                }
            }
            if !open {
                self.tools.network_ui.composer = None;
            }
        }
    }
    pub fn network_row_menu(ui: &mut Ui, row: &network::Request) -> Option<&'static str> {
        let mut action = None;
        if ui.button("Copy URL").clicked() {
            ui.ctx().copy_text(row.url.clone());
            ui.close();
        }
        let curl = row.curl();
        if ui
            .add_enabled(curl.is_ok(), egui::Button::new("Copy as cURL"))
            .on_hover_text(
                curl.as_ref()
                    .err()
                    .map(String::as_str)
                    .unwrap_or("Includes captured request headers and body"),
            )
            .clicked()
        {
            if let Ok(command) = curl {
                ui.ctx().copy_text(command);
            }
            ui.close();
        }
        for (label, headers) in [
            ("Copy request headers", &row.request_headers),
            ("Copy response headers", &row.response_headers),
        ] {
            if ui.button(label).clicked() {
                ui.ctx().copy_text(
                    headers
                        .iter()
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                ui.close();
            }
        }
        if ui
            .add_enabled(
                row.response_body.text().is_some(),
                egui::Button::new("Copy response preview"),
            )
            .on_hover_text("Text response previews only")
            .clicked()
        {
            if let Some(text) = row.response_body.text() {
                ui.ctx().copy_text(text.into());
            }
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(
                !row.request_body.truncated() && row.request_body.text().is_some(),
                egui::Button::new("Edit and resend…"),
            )
            .clicked()
        {
            action = Some("compose");
            ui.close();
        }
        if ui.button("Open in new tab").clicked() {
            action = Some("open");
            ui.close();
        }
        if ui.button("Download file…").clicked() {
            action = Some("download");
            ui.close();
        }
        if ui
            .add_enabled(
                !row.domain().is_empty(),
                egui::Button::new("Block this domain"),
            )
            .clicked()
        {
            action = Some("block");
            ui.close();
        }
        action
    }
}

#[cfg(test)]
#[path = "network_controls_tests.rs"]
mod tests;
