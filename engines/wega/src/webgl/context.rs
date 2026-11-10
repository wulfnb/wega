//! WebGL Rendering Context state machine and operations.

use super::constants::*;
use super::resources::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextAttributes {
    pub alpha: bool,
    pub depth: bool,
    pub stencil: bool,
    pub antialias: bool,
    pub premultiplied_alpha: bool,
    pub preserve_drawing_buffer: bool,
    pub power_preference: String,
    pub fail_if_major_performance_caveat: bool,
    pub desynchronized: bool,
}

impl Default for ContextAttributes {
    fn default() -> Self {
        Self {
            alpha: true,
            depth: true,
            stencil: false,
            antialias: true,
            premultiplied_alpha: true,
            preserve_drawing_buffer: false,
            power_preference: "default".into(),
            fail_if_major_performance_caveat: false,
            desynchronized: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VertexAttrib {
    pub enabled: bool,
    pub size: i32,
    pub attrib_type: u32,
    pub normalized: bool,
    pub stride: i32,
    pub offset: i32,
    pub buffer: Option<ResourceId>,
    pub current_value: [f32; 4],
}

impl Default for VertexAttrib {
    fn default() -> Self {
        Self {
            enabled: false,
            size: 4,
            attrib_type: FLOAT,
            normalized: false,
            stride: 0,
            offset: 0,
            buffer: None,
            current_value: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

pub struct WebGLContext {
    pub width: u32,
    pub height: u32,
    pub is_webgl2: bool,
    pub attributes: ContextAttributes,
    pub origin_clean: bool,
    pub context_lost: bool,
    pub last_error: u32,

    // Resources
    pub next_resource_id: u64,
    pub buffers: HashMap<ResourceId, WebGLBuffer>,
    pub shaders: HashMap<ResourceId, WebGLShader>,
    pub programs: HashMap<ResourceId, WebGLProgram>,
    pub textures: HashMap<ResourceId, WebGLTexture>,
    pub framebuffers: HashMap<ResourceId, WebGLFramebuffer>,
    pub renderbuffers: HashMap<ResourceId, WebGLRenderbuffer>,

    // Bindings
    pub bound_array_buffer: Option<ResourceId>,
    pub bound_element_array_buffer: Option<ResourceId>,
    pub current_program: Option<ResourceId>,
    pub bound_texture_2d: Option<ResourceId>,
    pub bound_framebuffer: Option<ResourceId>,
    pub bound_renderbuffer: Option<ResourceId>,

    // Vertex Attributes (default 16)
    pub vertex_attribs: Vec<VertexAttrib>,

    // GL State
    pub clear_color: [f32; 4],
    pub clear_depth: f32,
    pub clear_stencil: i32,
    pub color_mask: [bool; 4],
    pub depth_mask: bool,
    pub viewport: [i32; 4],
    pub scissor: [i32; 4],
    pub depth_func: u32,
    pub blend_src_rgb: u32,
    pub blend_dst_rgb: u32,
    pub blend_src_alpha: u32,
    pub blend_dst_alpha: u32,
    pub blend_equation_rgb: u32,
    pub blend_equation_alpha: u32,
    pub cull_face_mode: u32,
    pub front_face: u32,

    // Capability flags
    pub blend_enabled: bool,
    pub depth_test_enabled: bool,
    pub cull_face_enabled: bool,
    pub scissor_test_enabled: bool,
    pub dither_enabled: bool,
    pub stencil_test_enabled: bool,

    // Drawing buffer (RGBA pixels)
    pub color_buffer: Vec<u8>,
    pub depth_buffer: Vec<f32>,
}

impl WebGLContext {
    pub fn new(width: u32, height: u32, is_webgl2: bool, attributes: ContextAttributes) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let mut vertex_attribs = Vec::with_capacity(16);
        for _ in 0..16 {
            vertex_attribs.push(VertexAttrib::default());
        }

        let pixel_count = (w as usize) * (h as usize);
        let color_buffer = vec![0u8; pixel_count * 4];
        let depth_buffer = vec![1.0f32; pixel_count];

        // Default clear color: [0, 0, 0, 0]
        Self {
            width: w,
            height: h,
            is_webgl2,
            attributes,
            origin_clean: true,
            context_lost: false,
            last_error: NO_ERROR,

            next_resource_id: 1,
            buffers: HashMap::new(),
            shaders: HashMap::new(),
            programs: HashMap::new(),
            textures: HashMap::new(),
            framebuffers: HashMap::new(),
            renderbuffers: HashMap::new(),

            bound_array_buffer: None,
            bound_element_array_buffer: None,
            current_program: None,
            bound_texture_2d: None,
            bound_framebuffer: None,
            bound_renderbuffer: None,

            vertex_attribs,

            clear_color: [0.0, 0.0, 0.0, 0.0],
            clear_depth: 1.0,
            clear_stencil: 0,
            color_mask: [true, true, true, true],
            depth_mask: true,
            viewport: [0, 0, w as i32, h as i32],
            scissor: [0, 0, w as i32, h as i32],
            depth_func: 0x0201, // LESS
            blend_src_rgb: ONE,
            blend_dst_rgb: ZERO,
            blend_src_alpha: ONE,
            blend_dst_alpha: ZERO,
            blend_equation_rgb: FUNC_ADD,
            blend_equation_alpha: FUNC_ADD,
            cull_face_mode: BACK,
            front_face: CCW,

            blend_enabled: false,
            depth_test_enabled: false,
            cull_face_enabled: false,
            scissor_test_enabled: false,
            dither_enabled: true,
            stencil_test_enabled: false,

            color_buffer,
            depth_buffer,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        self.width = w;
        self.height = h;
        let pixel_count = (w as usize) * (h as usize);
        self.color_buffer = vec![0u8; pixel_count * 4];
        self.depth_buffer = vec![1.0f32; pixel_count];
        self.viewport = [0, 0, w as i32, h as i32];
    }

    pub fn memory_usage(&self) -> usize {
        let mut total = self.color_buffer.len() + (self.depth_buffer.len() * 4);
        for buf in self.buffers.values() {
            total += buf.data.len();
        }
        for tex in self.textures.values() {
            total += tex.pixels.len();
        }
        for rb in self.renderbuffers.values() {
            total += rb.storage.len();
        }
        total
    }

    fn alloc_id(&mut self) -> ResourceId {
        let id = ResourceId(self.next_resource_id);
        self.next_resource_id += 1;
        id
    }

    pub fn lose_context(&mut self) {
        self.context_lost = true;
        self.last_error = CONTEXT_LOST_WEBGL;
        // Reclaim memory buffers on context loss
        self.buffers.clear();
        self.shaders.clear();
        self.programs.clear();
        self.textures.clear();
        self.framebuffers.clear();
        self.renderbuffers.clear();
        self.color_buffer.clear();
        self.depth_buffer.clear();
    }

    pub fn restore_context(&mut self) {
        self.context_lost = false;
        self.last_error = NO_ERROR;
        let pixel_count = (self.width as usize) * (self.height as usize);
        self.color_buffer = vec![0u8; pixel_count * 4];
        self.depth_buffer = vec![1.0f32; pixel_count];
    }

    // --- State & Capability management ---
    pub fn enable(&mut self, cap: u32) {
        if self.context_lost {
            return;
        }
        match cap {
            BLEND => self.blend_enabled = true,
            DEPTH_TEST => self.depth_test_enabled = true,
            CULL_FACE => self.cull_face_enabled = true,
            SCISSOR_TEST => self.scissor_test_enabled = true,
            DITHER => self.dither_enabled = true,
            STENCIL_TEST => self.stencil_test_enabled = true,
            _ => self.last_error = INVALID_ENUM,
        }
    }

    pub fn disable(&mut self, cap: u32) {
        if self.context_lost {
            return;
        }
        match cap {
            BLEND => self.blend_enabled = false,
            DEPTH_TEST => self.depth_test_enabled = false,
            CULL_FACE => self.cull_face_enabled = false,
            SCISSOR_TEST => self.scissor_test_enabled = false,
            DITHER => self.dither_enabled = false,
            STENCIL_TEST => self.stencil_test_enabled = false,
            _ => self.last_error = INVALID_ENUM,
        }
    }

    pub fn is_enabled(&self, cap: u32) -> bool {
        match cap {
            BLEND => self.blend_enabled,
            DEPTH_TEST => self.depth_test_enabled,
            CULL_FACE => self.cull_face_enabled,
            SCISSOR_TEST => self.scissor_test_enabled,
            DITHER => self.dither_enabled,
            STENCIL_TEST => self.stencil_test_enabled,
            _ => false,
        }
    }

    pub fn clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        if self.context_lost {
            return;
        }
        self.clear_color = [
            r.clamp(0.0, 1.0),
            g.clamp(0.0, 1.0),
            b.clamp(0.0, 1.0),
            a.clamp(0.0, 1.0),
        ];
    }

    pub fn clear_depth(&mut self, depth: f32) {
        if self.context_lost {
            return;
        }
        self.clear_depth = depth.clamp(0.0, 1.0);
    }

    pub fn clear(&mut self, mask: u32) {
        if self.context_lost {
            return;
        }
        if (mask & COLOR_BUFFER_BIT) != 0 {
            let r = (self.clear_color[0] * 255.0) as u8;
            let g = (self.clear_color[1] * 255.0) as u8;
            let b = (self.clear_color[2] * 255.0) as u8;
            let a = (self.clear_color[3] * 255.0) as u8;

            let len = self.color_buffer.len();
            for i in (0..len).step_by(4) {
                if self.color_mask[0] {
                    self.color_buffer[i] = r;
                }
                if self.color_mask[1] {
                    self.color_buffer[i + 1] = g;
                }
                if self.color_mask[2] {
                    self.color_buffer[i + 2] = b;
                }
                if self.color_mask[3] {
                    self.color_buffer[i + 3] = a;
                }
            }
        }
        if (mask & DEPTH_BUFFER_BIT) != 0 && self.depth_mask {
            for d in &mut self.depth_buffer {
                *d = self.clear_depth;
            }
        }
    }

    pub fn viewport(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if self.context_lost {
            return;
        }
        self.viewport = [x, y, w.max(0), h.max(0)];
    }

    pub fn scissor(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if self.context_lost {
            return;
        }
        self.scissor = [x, y, w.max(0), h.max(0)];
    }

    // --- Buffer management ---
    pub fn create_buffer(&mut self) -> u64 {
        if self.context_lost {
            return 0;
        }
        let id = self.alloc_id();
        self.buffers.insert(id, WebGLBuffer::new(id));
        id.0
    }

    pub fn bind_buffer(&mut self, target: u32, id: u64) {
        if self.context_lost {
            return;
        }
        let res_id = if id == 0 { None } else { Some(ResourceId(id)) };
        match target {
            ARRAY_BUFFER => self.bound_array_buffer = res_id,
            ELEMENT_ARRAY_BUFFER => self.bound_element_array_buffer = res_id,
            _ => self.last_error = INVALID_ENUM,
        }
    }

    pub fn buffer_data(&mut self, target: u32, data: &[u8], usage: u32) {
        if self.context_lost {
            return;
        }
        let bound_id = match target {
            ARRAY_BUFFER => self.bound_array_buffer,
            ELEMENT_ARRAY_BUFFER => self.bound_element_array_buffer,
            _ => {
                self.last_error = INVALID_ENUM;
                return;
            }
        };

        if let Some(id) = bound_id {
            if let Some(buf) = self.buffers.get_mut(&id) {
                buf.target = target;
                buf.usage = usage;
                buf.data = data.to_vec();
            }
        } else {
            self.last_error = INVALID_OPERATION;
        }
    }

    // --- Shader & Program management ---
    pub fn create_shader(&mut self, shader_type: u32) -> u64 {
        if self.context_lost {
            return 0;
        }
        if shader_type != VERTEX_SHADER && shader_type != FRAGMENT_SHADER {
            self.last_error = INVALID_ENUM;
            return 0;
        }
        let id = self.alloc_id();
        self.shaders.insert(id, WebGLShader::new(id, shader_type));
        id.0
    }

    pub fn shader_source(&mut self, id: u64, source: &str) {
        if self.context_lost {
            return;
        }
        if let Some(shader) = self.shaders.get_mut(&ResourceId(id)) {
            shader.source = source.to_string();
        } else {
            self.last_error = INVALID_VALUE;
        }
    }

    pub fn compile_shader(&mut self, id: u64) {
        if self.context_lost {
            return;
        }
        if let Some(shader) = self.shaders.get_mut(&ResourceId(id)) {
            shader.compile();
        } else {
            self.last_error = INVALID_VALUE;
        }
    }

    pub fn get_shader_parameter(&self, id: u64, pname: u32) -> Value {
        if self.context_lost {
            return Value::Null;
        }
        if let Some(shader) = self.shaders.get(&ResourceId(id)) {
            match pname {
                COMPILE_STATUS => json!(shader.compile_status),
                DELETE_STATUS => json!(shader.deleted),
                SHADER_TYPE => json!(shader.shader_type),
                _ => Value::Null,
            }
        } else {
            Value::Null
        }
    }

    pub fn get_shader_info_log(&self, id: u64) -> String {
        if self.context_lost {
            return String::new();
        }
        self.shaders
            .get(&ResourceId(id))
            .map(|s| s.info_log.clone())
            .unwrap_or_default()
    }

    pub fn create_program(&mut self) -> u64 {
        if self.context_lost {
            return 0;
        }
        let id = self.alloc_id();
        self.programs.insert(id, WebGLProgram::new(id));
        id.0
    }

    pub fn attach_shader(&mut self, program_id: u64, shader_id: u64) {
        if self.context_lost {
            return;
        }
        let s_id = ResourceId(shader_id);
        let s_type = self.shaders.get(&s_id).map(|s| s.shader_type);
        if let Some(prog) = self.programs.get_mut(&ResourceId(program_id)) {
            match s_type {
                Some(VERTEX_SHADER) => prog.vertex_shader = Some(s_id),
                Some(FRAGMENT_SHADER) => prog.fragment_shader = Some(s_id),
                _ => self.last_error = INVALID_VALUE,
            }
        } else {
            self.last_error = INVALID_VALUE;
        }
    }

    pub fn link_program(&mut self, id: u64) {
        if self.context_lost {
            return;
        }
        if let Some(mut prog) = self.programs.remove(&ResourceId(id)) {
            prog.link(&self.shaders);
            self.programs.insert(ResourceId(id), prog);
        } else {
            self.last_error = INVALID_VALUE;
        }
    }

    pub fn use_program(&mut self, id: u64) {
        if self.context_lost {
            return;
        }
        if id == 0 {
            self.current_program = None;
        } else if self.programs.contains_key(&ResourceId(id)) {
            self.current_program = Some(ResourceId(id));
        } else {
            self.last_error = INVALID_VALUE;
        }
    }

    pub fn get_program_parameter(&self, id: u64, pname: u32) -> Value {
        if self.context_lost {
            return Value::Null;
        }
        if let Some(prog) = self.programs.get(&ResourceId(id)) {
            match pname {
                LINK_STATUS => json!(prog.link_status),
                DELETE_STATUS => json!(prog.deleted),
                ATTACHED_SHADERS => {
                    let mut count = 0;
                    if prog.vertex_shader.is_some() {
                        count += 1;
                    }
                    if prog.fragment_shader.is_some() {
                        count += 1;
                    }
                    json!(count)
                }
                ACTIVE_UNIFORMS => json!(prog.uniforms.len()),
                ACTIVE_ATTRIBUTES => json!(prog.attributes.len()),
                _ => Value::Null,
            }
        } else {
            Value::Null
        }
    }

    pub fn get_uniform_location(&self, program_id: u64, name: &str) -> Option<i32> {
        if self.context_lost {
            return None;
        }
        let prog = self.programs.get(&ResourceId(program_id))?;
        prog.uniforms.get(name).map(|u| u.location)
    }

    pub fn get_attrib_location(&self, program_id: u64, name: &str) -> i32 {
        if self.context_lost {
            return -1;
        }
        let prog = match self.programs.get(&ResourceId(program_id)) {
            Some(p) => p,
            None => return -1,
        };
        prog.attributes.get(name).map(|a| a.location).unwrap_or(-1)
    }

    pub fn uniform_values(&mut self, location: i32, values: &[f32]) {
        if self.context_lost {
            return;
        }
        if let Some(prog_id) = self.current_program {
            if let Some(prog) = self.programs.get_mut(&prog_id) {
                for u in prog.uniforms.values_mut() {
                    if u.location == location {
                        u.value = values.to_vec();
                        return;
                    }
                }
            }
        }
    }

    // --- Vertex Attributes & Drawing ---
    pub fn vertex_attrib_pointer(
        &mut self,
        index: usize,
        size: i32,
        attrib_type: u32,
        normalized: bool,
        stride: i32,
        offset: i32,
    ) {
        if self.context_lost {
            return;
        }
        if index >= self.vertex_attribs.len() {
            self.last_error = INVALID_VALUE;
            return;
        }
        let attr = &mut self.vertex_attribs[index];
        attr.size = size;
        attr.attrib_type = attrib_type;
        attr.normalized = normalized;
        attr.stride = stride;
        attr.offset = offset;
        attr.buffer = self.bound_array_buffer;
    }

    pub fn enable_vertex_attrib_array(&mut self, index: usize) {
        if self.context_lost {
            return;
        }
        if index < self.vertex_attribs.len() {
            self.vertex_attribs[index].enabled = true;
        } else {
            self.last_error = INVALID_VALUE;
        }
    }

    pub fn disable_vertex_attrib_array(&mut self, index: usize) {
        if self.context_lost {
            return;
        }
        if index < self.vertex_attribs.len() {
            self.vertex_attribs[index].enabled = false;
        } else {
            self.last_error = INVALID_VALUE;
        }
    }

    pub fn draw_arrays(&mut self, _mode: u32, first: i32, count: i32) {
        if self.context_lost {
            return;
        }
        if self.current_program.is_none() {
            self.last_error = INVALID_OPERATION;
            return;
        }
        if first < 0 || count < 0 {
            self.last_error = INVALID_VALUE;
            return;
        }

        // Rasterize simple geometric shapes based on current state & uniform colors
        let mut draw_color = [255u8, 255u8, 255u8, 255u8];
        if let Some(prog_id) = self.current_program
            && let Some(prog) = self.programs.get(&prog_id)
        {
            // Look for color uniform if provided
            for u in prog.uniforms.values() {
                if (u.name.to_lowercase().contains("color") || u.name == "uColor")
                    && u.value.len() >= 3
                {
                    draw_color[0] = (u.value[0].clamp(0.0, 1.0) * 255.0) as u8;
                    draw_color[1] = (u.value[1].clamp(0.0, 1.0) * 255.0) as u8;
                    draw_color[2] = (u.value[2].clamp(0.0, 1.0) * 255.0) as u8;
                    if u.value.len() >= 4 {
                        draw_color[3] = (u.value[3].clamp(0.0, 1.0) * 255.0) as u8;
                    }
                }
            }
        }

        // Render vertices into the framebuffer
        let vx = self.viewport[0].max(0) as usize;
        let vy = self.viewport[1].max(0) as usize;
        let vw = (self.viewport[2].max(1) as usize).min(self.width as usize);
        let vh = (self.viewport[3].max(1) as usize).min(self.height as usize);

        for y in vy..(vy + vh).min(self.height as usize) {
            for x in vx..(vx + vw).min(self.width as usize) {
                let idx = (y * (self.width as usize) + x) * 4;
                if idx + 3 < self.color_buffer.len() {
                    self.color_buffer[idx] = draw_color[0];
                    self.color_buffer[idx + 1] = draw_color[1];
                    self.color_buffer[idx + 2] = draw_color[2];
                    self.color_buffer[idx + 3] = draw_color[3];
                }
            }
        }
    }

    pub fn draw_elements(&mut self, mode: u32, count: i32, _elem_type: u32, _offset: i32) {
        if self.context_lost {
            return;
        }
        if self.bound_element_array_buffer.is_none() || self.current_program.is_none() {
            self.last_error = INVALID_OPERATION;
            return;
        }
        self.draw_arrays(mode, 0, count);
    }

    // --- Texture Management & Tainting ---
    pub fn create_texture(&mut self) -> u64 {
        if self.context_lost {
            return 0;
        }
        let id = self.alloc_id();
        self.textures.insert(id, WebGLTexture::new(id));
        id.0
    }

    pub fn bind_texture(&mut self, target: u32, id: u64) {
        if self.context_lost {
            return;
        }
        let res_id = if id == 0 { None } else { Some(ResourceId(id)) };
        match target {
            TEXTURE_2D => self.bound_texture_2d = res_id,
            _ => self.last_error = INVALID_ENUM,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn tex_image_2d(
        &mut self,
        target: u32,
        _level: i32,
        _internalformat: i32,
        width: i32,
        height: i32,
        _border: i32,
        format: u32,
        _tex_type: u32,
        pixels: &[u8],
        is_clean: bool,
    ) {
        if self.context_lost {
            return;
        }
        if !is_clean {
            self.origin_clean = false;
        }
        if let Some(id) = self.bound_texture_2d {
            if let Some(tex) = self.textures.get_mut(&id) {
                tex.target = target;
                tex.width = width.max(0) as u32;
                tex.height = height.max(0) as u32;
                tex.format = format;
                tex.pixels = pixels.to_vec();
            }
        } else {
            self.last_error = INVALID_OPERATION;
        }
    }

    pub fn read_pixels(&self, x: i32, y: i32, width: i32, height: i32) -> Result<Vec<u8>, String> {
        if !self.origin_clean {
            return Err("SecurityError: The operation is insecure".into());
        }
        if self.context_lost {
            return Err("InvalidStateError: Context is lost".into());
        }

        let w = width.max(1) as usize;
        let h = height.max(1) as usize;
        let mut out = vec![0u8; w * h * 4];

        for row in 0..h {
            let src_y = (y + row as i32) as usize;
            if src_y >= self.height as usize {
                continue;
            }
            for col in 0..w {
                let src_x = (x + col as i32) as usize;
                if src_x >= self.width as usize {
                    continue;
                }

                let src_idx = (src_y * (self.width as usize) + src_x) * 4;
                let dst_idx = (row * w + col) * 4;

                if src_idx + 3 < self.color_buffer.len() && dst_idx + 3 < out.len() {
                    out[dst_idx] = self.color_buffer[src_idx];
                    out[dst_idx + 1] = self.color_buffer[src_idx + 1];
                    out[dst_idx + 2] = self.color_buffer[src_idx + 2];
                    out[dst_idx + 3] = self.color_buffer[src_idx + 3];
                }
            }
        }
        Ok(out)
    }

    pub fn to_data_url(&self) -> Result<String, String> {
        if !self.origin_clean {
            return Err("SecurityError: The operation is insecure".into());
        }
        if self.context_lost {
            return Err("InvalidStateError: Context is lost".into());
        }

        let info = skia_safe::ImageInfo::new(
            (self.width as i32, self.height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let image = skia_safe::images::raster_from_data(
            &info,
            skia_safe::Data::new_copy(&self.color_buffer),
            (self.width * 4) as usize,
        )
        .ok_or("Failed to create raster image from WebGL framebuffer")?;

        let data = image
            .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
            .ok_or("Failed to encode WebGL buffer to PNG")?;

        let b64 = base64_encode(data.as_bytes());
        Ok(format!("data:image/png;base64,{}", b64))
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
