//! WebGL resource structures: buffers, shaders, programs, textures, framebuffers, and renderbuffers.

use super::constants::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceId(pub u64);

#[derive(Clone, Debug)]
pub struct WebGLBuffer {
    pub id: ResourceId,
    pub target: u32,
    pub usage: u32,
    pub data: Vec<u8>,
    pub deleted: bool,
}

impl WebGLBuffer {
    pub fn new(id: ResourceId) -> Self {
        Self {
            id,
            target: ARRAY_BUFFER,
            usage: STATIC_DRAW,
            data: Vec::new(),
            deleted: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WebGLShader {
    pub id: ResourceId,
    pub shader_type: u32,
    pub source: String,
    pub compile_status: bool,
    pub info_log: String,
    pub deleted: bool,
}

impl WebGLShader {
    pub fn new(id: ResourceId, shader_type: u32) -> Self {
        Self {
            id,
            shader_type,
            source: String::new(),
            compile_status: false,
            info_log: String::new(),
            deleted: false,
        }
    }

    pub fn compile(&mut self) {
        // Inspect GLSL shader source for basic syntax validity
        let src = self.source.trim();
        if src.is_empty() {
            self.compile_status = false;
            self.info_log = "Shader source is empty".into();
            return;
        }

        // Basic GLSL ES sanity check: main function required
        if !src.contains("main") {
            self.compile_status = false;
            self.info_log = "Shader error: missing void main() function".into();
            return;
        }

        self.compile_status = true;
        self.info_log = String::new();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UniformInfo {
    pub name: String,
    pub uniform_type: u32,
    pub size: i32,
    pub location: i32,
    pub value: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttributeInfo {
    pub name: String,
    pub attrib_type: u32,
    pub size: i32,
    pub location: i32,
}

#[derive(Clone, Debug)]
pub struct WebGLProgram {
    pub id: ResourceId,
    pub vertex_shader: Option<ResourceId>,
    pub fragment_shader: Option<ResourceId>,
    pub link_status: bool,
    pub info_log: String,
    pub deleted: bool,
    pub uniforms: HashMap<String, UniformInfo>,
    pub uniform_locations: Vec<String>,
    pub attributes: HashMap<String, AttributeInfo>,
}

impl WebGLProgram {
    pub fn new(id: ResourceId) -> Self {
        Self {
            id,
            vertex_shader: None,
            fragment_shader: None,
            link_status: false,
            info_log: String::new(),
            deleted: false,
            uniforms: HashMap::new(),
            uniform_locations: Vec::new(),
            attributes: HashMap::new(),
        }
    }

    pub fn link(&mut self, shaders: &HashMap<ResourceId, WebGLShader>) {
        let vs = self.vertex_shader.and_then(|id| shaders.get(&id));
        let fs = self.fragment_shader.and_then(|id| shaders.get(&id));

        if vs.is_none() || fs.is_none() {
            self.link_status = false;
            self.info_log = "Program requires both vertex and fragment shaders attached".into();
            return;
        }

        let vs = vs.unwrap();
        let fs = fs.unwrap();

        if !vs.compile_status || !fs.compile_status {
            self.link_status = false;
            self.info_log = "Attached shaders failed compilation".into();
            return;
        }

        // Reflect uniforms and attributes from shader source code
        self.uniforms.clear();
        self.uniform_locations.clear();
        self.attributes.clear();

        let mut loc = 0;
        let combined = format!("{}\n{}", vs.source, fs.source);
        // Normalize statements by splitting on semicolon
        for stmt in combined.split(';') {
            let stmt = stmt.trim();
            if stmt.starts_with("uniform ") {
                let parts: Vec<&str> = stmt.split_whitespace().collect();
                if parts.len() >= 3 {
                    let name = parts[2]
                        .trim_end_matches(';')
                        .split('[')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !name.is_empty() && !self.uniforms.contains_key(&name) {
                        self.uniforms.insert(
                            name.clone(),
                            UniformInfo {
                                name: name.clone(),
                                uniform_type: FLOAT,
                                size: 1,
                                location: loc,
                                value: vec![0.0; 16],
                            },
                        );
                        self.uniform_locations.push(name);
                        loc += 1;
                    }
                }
            } else if stmt.starts_with("attribute ") || stmt.starts_with("in ") {
                let parts: Vec<&str> = stmt.split_whitespace().collect();
                if parts.len() >= 3 {
                    let name = parts[2].trim_end_matches(';').to_string();
                    if !name.is_empty() && !self.attributes.contains_key(&name) {
                        let attr_loc = self.attributes.len() as i32;
                        self.attributes.insert(
                            name.clone(),
                            AttributeInfo {
                                name,
                                attrib_type: FLOAT,
                                size: 1,
                                location: attr_loc,
                            },
                        );
                    }
                }
            }
        }

        self.link_status = true;
        self.info_log = String::new();
    }
}

#[derive(Clone, Debug)]
pub struct WebGLTexture {
    pub id: ResourceId,
    pub target: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub pixels: Vec<u8>,
    pub min_filter: u32,
    pub mag_filter: u32,
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub deleted: bool,
}

impl WebGLTexture {
    pub fn new(id: ResourceId) -> Self {
        Self {
            id,
            target: TEXTURE_2D,
            width: 0,
            height: 0,
            format: RGBA,
            pixels: Vec::new(),
            min_filter: NEAREST_MIPMAP_LINEAR,
            mag_filter: LINEAR,
            wrap_s: REPEAT,
            wrap_t: REPEAT,
            deleted: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WebGLFramebuffer {
    pub id: ResourceId,
    pub color_attachment: Option<ResourceId>,
    pub depth_attachment: Option<ResourceId>,
    pub stencil_attachment: Option<ResourceId>,
    pub deleted: bool,
}

impl WebGLFramebuffer {
    pub fn new(id: ResourceId) -> Self {
        Self {
            id,
            color_attachment: None,
            depth_attachment: None,
            stencil_attachment: None,
            deleted: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WebGLRenderbuffer {
    pub id: ResourceId,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub storage: Vec<u8>,
    pub deleted: bool,
}

impl WebGLRenderbuffer {
    pub fn new(id: ResourceId) -> Self {
        Self {
            id,
            format: RGBA,
            width: 0,
            height: 0,
            storage: Vec::new(),
            deleted: false,
        }
    }
}
