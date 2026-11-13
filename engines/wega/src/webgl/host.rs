//! Host operations shared by document and worker realms.
// Each arm returns explicitly, as it did inside the document dispatcher.
#![allow(clippy::needless_return)]
use serde_json::{Value, json};

/// Runs one `webgl*` operation; `None` means the operation belongs elsewhere.
pub(crate) fn host_call(
    engine: &mut super::WebGLEngine,
    args: &[Value],
) -> Option<Result<Value, String>> {
    let text = |index: usize| args.get(index).and_then(Value::as_str).unwrap_or("");
    if !text(0).starts_with("webgl") {
        return None;
    }
    Some((|| -> Result<Value, String> {
        match text(0) {
            "webglCreate" => {
                let w = text(1).parse::<u32>().unwrap_or(300);
                let h = text(2).parse::<u32>().unwrap_or(150);
                let is_webgl2 = text(3) == "webgl2";
                let id = engine.create_context(
                    w,
                    h,
                    is_webgl2,
                    crate::webgl::ContextAttributes::default(),
                )?;
                return Ok(json!(id.0));
            }
            "webglResize" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid WebGL ID")?;
                let w = text(2).parse::<u32>().unwrap_or(300);
                let h = text(3).parse::<u32>().unwrap_or(150);
                engine.resize_context(crate::webgl::WebGLContextId(id), w, h)?;
                return Ok(Value::Null);
            }
            "webglDestroy" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid WebGL ID")?;
                engine.destroy_context(crate::webgl::WebGLContextId(id));
                return Ok(Value::Null);
            }
            "webglGetMemory" => {
                let mem = engine.total_pixel_bytes();
                return Ok(json!(mem));
            }
            "webglOp" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid WebGL ID")?;
                let op = text(2);
                let op_args: Vec<Value> = serde_json::from_str(text(3)).unwrap_or_default();
                let ctx = engine
                    .get_mut(crate::webgl::WebGLContextId(id))
                    .ok_or("WebGL context not found")?;

                match op {
                    "getError" => return Ok(json!(ctx.last_error)),
                    "isContextLost" => return Ok(json!(ctx.context_lost)),
                    "loseContext" => {
                        ctx.lose_context();
                        return Ok(Value::Null);
                    }
                    "restoreContext" => {
                        ctx.restore_context();
                        return Ok(Value::Null);
                    }
                    "enable" => {
                        let cap = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        ctx.enable(cap);
                        return Ok(Value::Null);
                    }
                    "disable" => {
                        let cap = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        ctx.disable(cap);
                        return Ok(Value::Null);
                    }
                    "isEnabled" => {
                        let cap = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        return Ok(json!(ctx.is_enabled(cap)));
                    }
                    "clearColor" => {
                        let r = op_args.get(0).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                        let g = op_args.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                        let b = op_args.get(2).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                        let a = op_args.get(3).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                        ctx.clear_color(r, g, b, a);
                        return Ok(Value::Null);
                    }
                    "clearDepth" => {
                        let d = op_args.get(0).and_then(Value::as_f64).unwrap_or(1.0) as f32;
                        ctx.clear_depth(d);
                        return Ok(Value::Null);
                    }
                    "clear" => {
                        let mask = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        ctx.clear(mask);
                        return Ok(Value::Null);
                    }
                    "viewport" => {
                        let x = op_args.get(0).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let y = op_args.get(1).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let w = op_args.get(2).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let h = op_args.get(3).and_then(Value::as_i64).unwrap_or(0) as i32;
                        ctx.viewport(x, y, w, h);
                        return Ok(Value::Null);
                    }
                    "scissor" => {
                        let x = op_args.get(0).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let y = op_args.get(1).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let w = op_args.get(2).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let h = op_args.get(3).and_then(Value::as_i64).unwrap_or(0) as i32;
                        ctx.scissor(x, y, w, h);
                        return Ok(Value::Null);
                    }
                    "createBuffer" => return Ok(json!(ctx.create_buffer())),
                    "bindBuffer" => {
                        let target = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let buf_id = op_args.get(1).and_then(Value::as_u64).unwrap_or(0);
                        ctx.bind_buffer(target, buf_id);
                        return Ok(Value::Null);
                    }
                    "bufferData" => {
                        let target = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let data: Vec<u8> = op_args
                            .get(1)
                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                            .unwrap_or_default();
                        let usage = op_args.get(2).and_then(Value::as_u64).unwrap_or(0) as u32;
                        ctx.buffer_data(target, &data, usage);
                        return Ok(Value::Null);
                    }
                    "createShader" => {
                        let stype = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        return Ok(json!(ctx.create_shader(stype)));
                    }
                    "shaderSource" => {
                        let sid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        let src = op_args.get(1).and_then(Value::as_str).unwrap_or("");
                        ctx.shader_source(sid, src);
                        return Ok(Value::Null);
                    }
                    "compileShader" => {
                        let sid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        ctx.compile_shader(sid);
                        return Ok(Value::Null);
                    }
                    "getShaderParameter" => {
                        let sid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        let pname = op_args.get(1).and_then(Value::as_u64).unwrap_or(0) as u32;
                        return Ok(ctx.get_shader_parameter(sid, pname));
                    }
                    "getShaderInfoLog" => {
                        let sid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        return Ok(json!(ctx.get_shader_info_log(sid)));
                    }
                    "createProgram" => return Ok(json!(ctx.create_program())),
                    "attachShader" => {
                        let pid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        let sid = op_args.get(1).and_then(Value::as_u64).unwrap_or(0);
                        ctx.attach_shader(pid, sid);
                        return Ok(Value::Null);
                    }
                    "linkProgram" => {
                        let pid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        ctx.link_program(pid);
                        return Ok(Value::Null);
                    }
                    "useProgram" => {
                        let pid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        ctx.use_program(pid);
                        return Ok(Value::Null);
                    }
                    "getProgramParameter" => {
                        let pid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        let pname = op_args.get(1).and_then(Value::as_u64).unwrap_or(0) as u32;
                        return Ok(ctx.get_program_parameter(pid, pname));
                    }
                    "getUniformLocation" => {
                        let pid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        let name = op_args.get(1).and_then(Value::as_str).unwrap_or("");
                        return Ok(json!(ctx.get_uniform_location(pid, name)));
                    }
                    "getAttribLocation" => {
                        let pid = op_args.get(0).and_then(Value::as_u64).unwrap_or(0);
                        let name = op_args.get(1).and_then(Value::as_str).unwrap_or("");
                        return Ok(json!(ctx.get_attrib_location(pid, name)));
                    }
                    "uniformValues" => {
                        let loc = op_args.get(0).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let vals: Vec<f32> = op_args
                            .get(1)
                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                            .unwrap_or_default();
                        ctx.uniform_values(loc, &vals);
                        return Ok(Value::Null);
                    }
                    "vertexAttribPointer" => {
                        let idx = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as usize;
                        let size = op_args.get(1).and_then(Value::as_i64).unwrap_or(4) as i32;
                        let atype = op_args.get(2).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let norm = op_args.get(3).and_then(Value::as_bool).unwrap_or(false);
                        let stride = op_args.get(4).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let offset = op_args.get(5).and_then(Value::as_i64).unwrap_or(0) as i32;
                        ctx.vertex_attrib_pointer(idx, size, atype, norm, stride, offset);
                        return Ok(Value::Null);
                    }
                    "enableVertexAttribArray" => {
                        let idx = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as usize;
                        ctx.enable_vertex_attrib_array(idx);
                        return Ok(Value::Null);
                    }
                    "disableVertexAttribArray" => {
                        let idx = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as usize;
                        ctx.disable_vertex_attrib_array(idx);
                        return Ok(Value::Null);
                    }
                    "drawArrays" => {
                        let mode = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let first = op_args.get(1).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let count = op_args.get(2).and_then(Value::as_i64).unwrap_or(0) as i32;
                        ctx.draw_arrays(mode, first, count);
                        return Ok(Value::Null);
                    }
                    "drawElements" => {
                        let mode = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let count = op_args.get(1).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let etype = op_args.get(2).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let offset = op_args.get(3).and_then(Value::as_i64).unwrap_or(0) as i32;
                        ctx.draw_elements(mode, count, etype, offset);
                        return Ok(Value::Null);
                    }
                    "createTexture" => return Ok(json!(ctx.create_texture())),
                    "bindTexture" => {
                        let target = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let tid = op_args.get(1).and_then(Value::as_u64).unwrap_or(0);
                        ctx.bind_texture(target, tid);
                        return Ok(Value::Null);
                    }
                    "texImage2D" => {
                        let target = op_args.get(0).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let level = op_args.get(1).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let iformat = op_args.get(2).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let w = op_args.get(3).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let h = op_args.get(4).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let border = op_args.get(5).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let format = op_args.get(6).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let ttype = op_args.get(7).and_then(Value::as_u64).unwrap_or(0) as u32;
                        let pixels: Vec<u8> = op_args
                            .get(8)
                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                            .unwrap_or_default();
                        let is_clean = op_args.get(9).and_then(Value::as_bool).unwrap_or(true);
                        ctx.tex_image_2d(
                            target, level, iformat, w, h, border, format, ttype, &pixels, is_clean,
                        );
                        return Ok(Value::Null);
                    }
                    "taint" => {
                        ctx.origin_clean = false;
                        return Ok(Value::Null);
                    }
                    "readPixels" => {
                        let x = op_args.get(0).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let y = op_args.get(1).and_then(Value::as_i64).unwrap_or(0) as i32;
                        let w = op_args.get(2).and_then(Value::as_i64).unwrap_or(1) as i32;
                        let h = op_args.get(3).and_then(Value::as_i64).unwrap_or(1) as i32;
                        let pixels = ctx.read_pixels(x, y, w, h)?;
                        return Ok(json!(pixels));
                    }
                    "toDataURL" => {
                        let data_url = ctx.to_data_url()?;
                        return Ok(json!(data_url));
                    }
                    _ => return Err(format!("Unknown WebGL operation: {}", op)),
                }
            }
            other => return Err(format!("Unknown host operation: {other}")),
        }
    })())
}
