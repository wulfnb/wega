//! An owned display list crosses threads; the DOM and V8 isolate never do.
use anyrender::{ImageRenderer, PaintScene, Scene};
use anyrender_skia::SkiaImageRenderer;
use std::sync::{Arc, mpsc};
pub enum Pixels {
    Owned(Vec<u8>),
    Shared { slot: u8, sequence: u64 },
}
pub struct Painted {
    pub serial: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Pixels,
}
enum Job {
    Paint {
        serial: u64,
        width: u32,
        height: u32,
        scene: Scene,
        target: Option<(u8, u64)>,
    },
    Store(wega_platform::FrameStore),
    Release,
}
pub(crate) struct Raster {
    jobs: mpsc::SyncSender<Job>,
    results: mpsc::Receiver<Result<Painted, String>>,
    pending: bool,
}
impl Raster {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let (jobs, receiver) = mpsc::sync_channel(2);
        let (sender, results) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("wega-raster".into())
            .spawn(move || {
                let mut renderer: Option<SkiaImageRenderer> = None;
                let mut size = (0, 0);
                let mut store: Option<wega_platform::FrameStore> = None;
                while let Ok(job) = receiver.recv() {
                    match job {
                        Job::Store(value) => store = Some(value),
                        Job::Release => {
                            renderer = None;
                            size = (0, 0);
                            if let Some(store) = &mut store {
                                store.release_pages();
                            }
                            skia_safe::graphics::purge_all_caches();
                            wega_platform::trim_allocator();
                        }
                        Job::Paint {
                            serial,
                            width,
                            height,
                            scene,
                            target,
                        } => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    if renderer.is_none() || size != (width, height) {
                                        renderer = Some(SkiaImageRenderer::new(width, height));
                                        size = (width, height);
                                    }
                                    let renderer = renderer.as_mut().unwrap();
                                    renderer.reset();
                                    let pixels = if let (Some(store), Some((slot, sequence))) =
                                        (&mut store, target)
                                    {
                                        let bytes = store
                                            .slot(
                                                slot as usize,
                                                width as usize * height as usize * 4,
                                            )
                                            .map_err(|e| e.to_string())?;
                                        renderer.render(
                                            |painter| {
                                                painter.append_scene(scene, kurbo::Affine::IDENTITY)
                                            },
                                            bytes,
                                        );
                                        std::sync::atomic::fence(
                                            std::sync::atomic::Ordering::Release,
                                        );
                                        Pixels::Shared { slot, sequence }
                                    } else {
                                        let mut bytes = vec![];
                                        renderer.render_to_vec(
                                            |painter| {
                                                painter.append_scene(scene, kurbo::Affine::IDENTITY)
                                            },
                                            &mut bytes,
                                        );
                                        Pixels::Owned(bytes)
                                    };
                                    Ok(Painted {
                                        serial,
                                        width,
                                        height,
                                        pixels,
                                    })
                                }))
                                .unwrap_or_else(|_| {
                                    renderer = None;
                                    Err("Skia raster worker failed".into())
                                });
                            if sender.send(result).is_err() {
                                break;
                            }
                            wake();
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            jobs,
            results,
            pending: false,
        })
    }
    pub fn set_store(&self, store: wega_platform::FrameStore) -> Result<(), String> {
        self.jobs
            .send(Job::Store(store))
            .map_err(|_| "Raster worker stopped".into())
    }
    pub fn start(
        &mut self,
        serial: u64,
        width: u32,
        height: u32,
        scene: Scene,
        target: Option<(u8, u64)>,
    ) -> Result<(), String> {
        if self.pending {
            return Err("Raster worker is already painting".into());
        }
        self.jobs
            .try_send(Job::Paint {
                serial,
                width,
                height,
                scene,
                target,
            })
            .map_err(|_| "Raster queue is full or stopped")?;
        self.pending = true;
        Ok(())
    }
    pub fn pending(&self) -> bool {
        self.pending
    }
    pub fn take(&mut self) -> Option<Result<Painted, String>> {
        match self.results.try_recv() {
            Ok(result) => {
                self.pending = false;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) if self.pending => {
                self.pending = false;
                Some(Err("Raster worker stopped".into()))
            }
            Err(_) => None,
        }
    }
    pub fn wait(&mut self) -> Result<Painted, String> {
        let result = self.results.recv().map_err(|_| "Raster worker stopped")?;
        self.pending = false;
        result
    }
    pub fn release(&self) {
        let _ = self.jobs.send(Job::Release);
    }
}
