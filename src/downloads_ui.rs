use crate::{browser::Browser, downloads::Phase};
pub(crate) struct State {
    pub open: bool,
    pub url: String,
    pub filename: String,
    pub connections: usize,
    pub notice: Option<String>,
    focus_url: bool,
    /// A downloaded program waiting for the user to confirm opening it: (path, source URL).
    pub confirm_open: Option<(std::path::PathBuf, String)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            url: String::new(),
            filename: String::new(),
            connections: 4,
            notice: None,
            focus_url: true,
            confirm_open: None,
        }
    }
}
impl Browser {
    pub fn downloads_window(&mut self, ctx: &egui::Context) {
        self.confirm_open_program(ctx);
        if !self.download_ui.open {
            self.download_ui.focus_url = true;
            return;
        }
        let mut open = true;
        let mut start = false;
        let remote = std::env::var_os("BROWSER_DOWNLOAD_DIR").is_some();
        let enter =
            ctx.memory(|m| m.has_focus(egui::Id::unique("download-url"))) && crate::ux::enter(ctx);
        let colors = crate::theme::palette(ctx);
        egui::Window::new("Downloads").open(&mut open).default_width(560.0)
            .max_width((ctx.content_rect().width()-40.0).max(280.0)).show(ctx,|ui|{
            let jobs=self.downloads.jobs();
            let mut open_path=None;
            let mut forget=None;
            if jobs.is_empty(){
                crate::components::empty_state(ui,"No downloads yet","Choose Download link from a page's context menu, or paste a link below.",None);
            }
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-330.0).max(100.0)).show(ui,|ui|{
                for job in &jobs{
                    let progress=job.snapshot();let bytes=job.bytes();
                    let (status,color)=match progress.phase{
                        Phase::Connecting=>("Starting…",colors.muted),
                        Phase::Downloading=>("Downloading",colors.accent),
                        Phase::Paused=>("Paused",colors.amber),
                        Phase::Complete=>("Done",colors.success),
                        Phase::Cancelled=>("Cancelled",colors.muted),
                        Phase::Failed=>("Failed",colors.danger),
                    };
                    egui::Frame::new().fill(colors.surface).corner_radius(8).inner_margin(10).show(ui,|ui|{
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui|{
                            ui.add(egui::Label::new(egui::RichText::new(&progress.filename).strong()).truncate());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                                if matches!(progress.phase,Phase::Complete|Phase::Failed|Phase::Cancelled)
                                    && crate::ui::icon(ui,"×","Remove from list").clicked(){forget=Some(job.clone());}
                                // Connection details are for diagnosis; keep them out of the row.
                                let label=ui.label(egui::RichText::new(status).color(color).strong());
                                if !progress.message.is_empty() && progress.phase!=Phase::Failed {label.on_hover_text(&progress.message);}
                            });
                        });
                        ui.add(egui::Label::new(egui::RichText::new(&job.url).small().color(colors.muted)).truncate()).on_hover_text(&job.url);
                        match progress.total {
                            Some(total) if progress.phase!=Phase::Complete => {ui.add(egui::ProgressBar::new((bytes as f64/total.max(1) as f64).min(1.0) as f32).text(format!("{} of {}",size(bytes),size(total))));}
                            Some(total) => {ui.label(egui::RichText::new(size(total)).color(colors.muted));}
                            None => {ui.label(egui::RichText::new(format!("{} · size unknown",size(bytes))).color(colors.muted));}
                        }
                        if progress.phase==Phase::Failed && !progress.message.is_empty(){
                            ui.add(egui::Label::new(egui::RichText::new(&progress.message).color(colors.danger)).wrap());
                        }
                        ui.horizontal_wrapped(|ui|{
                            match progress.phase {
                                Phase::Connecting|Phase::Downloading=>{
                                    if ui.button("Pause").clicked(){job.pause();}
                                    if ui.button("Cancel").clicked(){job.cancel();}
                                    ui.label(egui::RichText::new(format!("{}/s",size(job.speed() as u64))).color(colors.muted));
                                    ctx.request_repaint_after(std::time::Duration::from_millis(500));
                                }
                                Phase::Paused=>{
                                    if ui.button("Resume").clicked() && let Err(error)=self.downloads.resume(job){self.download_ui.notice=Some(error);}
                                    if ui.button("Cancel").clicked(){job.cancel();}
                                }
                                Phase::Complete=>{if let Some(path)=&progress.path{
                                    if !remote {
                                        if ui.add(crate::components::primary_button(ui,"Open file")).clicked(){
                                            if crate::downloads::opens_as_program(path){self.download_ui.confirm_open=Some((path.clone(),job.url.clone()));}
                                            else{open_path=Some((path.clone(),false));}
                                        }
                                        if ui.button("Show in folder").clicked(){open_path=Some((path.clone(),true));}
                                    }
                                    if ui.button("Copy saved path").clicked(){ctx.copy_text(path.display().to_string());}
                                }}
                                Phase::Failed|Phase::Cancelled=>{if ui.button("Retry…").clicked(){self.download_ui.url=job.url.clone();self.download_ui.filename=progress.filename.clone();self.download_ui.focus_url=true;}}
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });
            if let Some(job)=forget{self.downloads.remove(&job);}
            if let Some((path,reveal))=open_path && let Err(error)=crate::ux::open_path(&path,reveal){self.download_ui.notice=Some(error);}
            let finished=jobs.iter().any(|j|matches!(j.snapshot().phase,Phase::Complete|Phase::Failed|Phase::Cancelled));
            if finished && ui.button("Clear finished entries").clicked(){self.downloads.clear_finished();}
            if remote {ui.weak("Use Downloads in the web viewer's toolbar to save completed files to this computer.");}
            ui.separator();
            ui.label(egui::RichText::new("Download from a link").strong());
            let field=ui.add(egui::TextEdit::singleline(&mut self.download_ui.url).id(egui::Id::unique("download-url"))
                .hint_text("https://example.com/file.zip").desired_width(f32::INFINITY));
            if self.download_ui.focus_url {field.request_focus();self.download_ui.focus_url=false;}
            ui.horizontal_wrapped(|ui|{
                ui.label("File name");ui.add(egui::TextEdit::singleline(&mut self.download_ui.filename).hint_text("Automatic").desired_width(200.0));
                start=ui.button("Download").clicked() || enter;
            });
            if let Some(notice)=&self.download_ui.notice {crate::components::inline_message(ui,crate::ux::Severity::Error,notice);}
            ui.collapsing("Advanced",|ui|{
                ui.horizontal(|ui|{ui.label("Connections");ui.add(egui::DragValue::new(&mut self.download_ui.connections).range(1..=8));});
                ui.weak("Byte-range support and a stable file validator are required for multiple connections. Other servers use one connection. Pause/resume preserves completed segments for this session.");
                ui.label(format!("Save to: {}",self.downloads.directory.display()));
                if ui.button("Copy folder path").clicked(){ctx.copy_text(self.downloads.directory.display().to_string());}
            });
        });
        self.download_ui.open = open;
        if start {
            self.begin_download();
        }
    }
    /// Programs and scripts run code when opened; never launch one with a single click.
    fn confirm_open_program(&mut self, ctx: &egui::Context) {
        let Some((path, url)) = self.download_ui.confirm_open.clone() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut decision = crate::ux::escape(ctx).then_some(false);
        egui::Modal::new(egui::Id::unique("confirm-open-download")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.heading(format!("Open “{name}”?"));
            ui.label("This file can run programs or change settings on your computer. Open it only if you trust where it came from.");
            ui.add(egui::Label::new(egui::RichText::new(format!("Downloaded from {url}")).small()).truncate())
                .on_hover_text(&url);
            if let Some(choice) = crate::components::dialog_buttons(ui, Some("Cancel"), "Open anyway", true) {
                decision = Some(choice);
            }
        });
        match decision {
            Some(true) => {
                self.download_ui.confirm_open = None;
                if let Err(error) = crate::ux::open_path(&path, false) {
                    self.download_ui.notice = Some(error);
                }
            }
            Some(false) => self.download_ui.confirm_open = None,
            None => {}
        }
    }
}
fn size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else {
        format!("{:.2} GB", b / (KB * KB * KB))
    }
}
