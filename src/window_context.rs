//! Native GPU context for egui; page pixels arrive from the Wega worker.
use glow::HasContext;
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};
use surfman::{
    Connection, Context, ContextAttributeFlags, ContextAttributes, Device, Error, GLApi, GLVersion,
    SurfaceAccess, SurfaceType,
};
use winit::{
    dpi::PhysicalSize,
    raw_window_handle::{DisplayHandle, WindowHandle},
};
pub struct WindowRenderingContext {
    device: Device,
    context: RefCell<Context>,
    gl: Arc<glow::Context>,
    size: Cell<PhysicalSize<u32>>,
}
impl WindowRenderingContext {
    pub fn new(
        display: DisplayHandle<'_>,
        window: WindowHandle<'_>,
        size: PhysicalSize<u32>,
    ) -> Result<Self, Error> {
        if size.width == 0 || size.height == 0 {
            return Err(Error::Failed);
        }
        let connection = Connection::from_display_handle(display)?;
        let adapter = connection.create_adapter()?;
        let device = connection.create_device(&adapter)?;
        let version = match connection.gl_api() {
            GLApi::GLES => GLVersion::new(3, 0),
            GLApi::GL => GLVersion::new(3, 2),
        };
        let descriptor = device.create_context_descriptor(&ContextAttributes {
            version,
            flags: ContextAttributeFlags::ALPHA
                | ContextAttributeFlags::DEPTH
                | ContextAttributeFlags::STENCIL,
        })?;
        let mut context = device.create_context(&descriptor, None)?;
        let result = (|| {
            let native_widget = connection.create_native_widget_from_window_handle(
                window,
                euclid::size2(size.width as i32, size.height as i32),
            )?;
            let surface = device.create_surface(
                &context,
                SurfaceAccess::GPUOnly,
                SurfaceType::Widget { native_widget },
            )?;
            device
                .bind_surface_to_context(&mut context, surface)
                .map_err(|(error, mut surface)| {
                    let _ = device.destroy_surface(&mut context, &mut surface);
                    error
                })?;
            device.make_context_current(&context)?;
            // SAFETY: this context is current; its symbols remain valid while it lives.
            let gl = Arc::new(unsafe {
                glow::Context::from_loader_function(|name| device.get_proc_address(&context, name))
            });
            Ok(gl)
        })();
        match result {
            Ok(gl) => Ok(Self {
                device,
                context: RefCell::new(context),
                gl,
                size: Cell::new(size),
            }),
            Err(error) => {
                let _ = device.destroy_context(&mut context);
                Err(error)
            }
        }
    }
    pub fn make_current(&self) -> Result<(), Error> {
        self.device.make_context_current(&self.context.borrow())
    }
    pub fn glow_gl_api(&self) -> Arc<glow::Context> {
        self.gl.clone()
    }
    pub fn prepare_for_rendering(&self) {
        let framebuffer = self
            .device
            .context_surface_info(&self.context.borrow())
            .ok()
            .flatten()
            .and_then(|info| info.framebuffer_object);
        // SAFETY: the caller made this window's context current before rendering.
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, framebuffer);
        }
    }
    pub fn resize(&self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        match self.device.resize_bound_surface(
            &mut self.context.borrow_mut(),
            euclid::size2(size.width as i32, size.height as i32),
        ) {
            Ok(()) => self.size.set(size),
            Err(error) => log::warn!("Cannot resize window surface: {error:?}"),
        }
    }
    pub fn present(&self) {
        if let Err(error) = self
            .device
            .present_bound_surface(&mut self.context.borrow_mut())
        {
            log::warn!("Cannot present window surface: {error:?}");
        }
    }
    pub fn read_to_image(&self) -> Option<image::RgbaImage> {
        self.make_current().ok()?;
        self.prepare_for_rendering();
        let size = self.size.get();
        let mut pixels = vec![0; size.width as usize * size.height as usize * 4];
        // SAFETY: the current framebuffer has this size and the buffer holds all RGBA bytes.
        unsafe {
            self.gl.read_pixels(
                0,
                0,
                size.width as i32,
                size.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
        }
        let mut image = image::RgbaImage::from_raw(size.width, size.height, pixels)?;
        image::imageops::flip_vertical_in_place(&mut image);
        Some(image)
    }
}
impl Drop for WindowRenderingContext {
    fn drop(&mut self) {
        let _ = self.device.destroy_context(self.context.get_mut());
    }
}
