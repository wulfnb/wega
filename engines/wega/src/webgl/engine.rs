//! WebGL Engine managing contexts across canvases and enforcing quotas.

use super::context::{ContextAttributes, WebGLContext};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WebGLContextId(pub u64);

pub struct WebGLEngine {
    contexts: HashMap<WebGLContextId, WebGLContext>,
    next_id: u64,
    max_pixel_bytes: usize,
}

impl Default for WebGLEngine {
    fn default() -> Self {
        Self::new(64 * 1024 * 1024)
    }
}

impl WebGLEngine {
    pub fn new(max_pixel_bytes: usize) -> Self {
        Self {
            contexts: HashMap::new(),
            next_id: 1,
            max_pixel_bytes,
        }
    }

    pub fn total_pixel_bytes(&self) -> usize {
        let mut total = 0;
        for ctx in self.contexts.values() {
            total += ctx.memory_usage();
        }
        total
    }

    pub fn create_context(
        &mut self,
        width: u32,
        height: u32,
        is_webgl2: bool,
        attributes: ContextAttributes,
    ) -> Result<WebGLContextId, String> {
        let initial_bytes = (width as usize) * (height as usize) * 8; // color + depth
        if self.total_pixel_bytes() + initial_bytes > self.max_pixel_bytes {
            return Err("QuotaExceededError: WebGL memory quota exceeded".into());
        }

        let id = WebGLContextId(self.next_id);
        self.next_id += 1;
        let ctx = WebGLContext::new(width, height, is_webgl2, attributes);
        self.contexts.insert(id, ctx);
        Ok(id)
    }

    pub fn resize_context(
        &mut self,
        id: WebGLContextId,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let new_bytes = (width as usize) * (height as usize) * 8;
        let current_total = self.total_pixel_bytes();
        let old_ctx_bytes = self
            .contexts
            .get(&id)
            .map(|c| c.memory_usage())
            .unwrap_or(0);
        if current_total - old_ctx_bytes + new_bytes > self.max_pixel_bytes {
            return Err("QuotaExceededError: WebGL memory quota exceeded".into());
        }

        let ctx = self
            .contexts
            .get_mut(&id)
            .ok_or("WebGL context not found")?;
        ctx.resize(width, height);
        Ok(())
    }

    pub fn destroy_context(&mut self, id: WebGLContextId) -> bool {
        self.contexts.remove(&id).is_some()
    }

    pub fn get_mut(&mut self, id: WebGLContextId) -> Option<&mut WebGLContext> {
        self.contexts.get_mut(&id)
    }

    pub fn get(&self, id: WebGLContextId) -> Option<&WebGLContext> {
        self.contexts.get(&id)
    }
}
