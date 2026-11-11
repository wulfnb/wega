//! Host operations shared by document and worker realms.
// Each arm returns explicitly, as it did inside the document dispatcher.
#![allow(clippy::needless_return)]
use serde_json::{Value, json};

/// Runs one `canvas*` operation; `None` means the operation belongs elsewhere.
pub(crate) fn host_call(
    engine: &mut super::CanvasEngine,
    args: &[Value],
) -> Option<Result<Value, String>> {
    let text = |index: usize| args.get(index).and_then(Value::as_str).unwrap_or("");
    if !text(0).starts_with("canvas") {
        return None;
    }
    Some((|| -> Result<Value, String> {
        match text(0) {
            "canvasCreate" => {
                let w = text(1).parse::<u32>().unwrap_or(300);
                let h = text(2).parse::<u32>().unwrap_or(150);
                let is_offscreen = text(3) == "true";
                let id = engine.create_canvas(w, h, is_offscreen)?;
                return Ok(json!(id.0));
            }
            "canvasResize" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                let w = text(2).parse::<u32>().unwrap_or(300);
                let h = text(3).parse::<u32>().unwrap_or(150);
                engine.resize_canvas(crate::canvas::CanvasId(id), w, h)?;
                return Ok(Value::Null);
            }
            "canvasDestroy" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                engine.destroy_canvas(crate::canvas::CanvasId(id));
                return Ok(Value::Null);
            }
            "canvasGetMemory" => {
                let mem = engine.total_pixel_bytes();
                return Ok(json!(mem));
            }
            "canvasExecute" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                let cmd = text(2);
                let cmd_args: Vec<Value> = serde_json::from_str(text(3)).unwrap_or_default();
                let ctx = engine
                    .get_mut(crate::canvas::CanvasId(id))
                    .ok_or("Canvas not found")?;
                let res = ctx.execute_command(cmd, &cmd_args)?;
                return Ok(res);
            }
            "canvasToDataURL" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                let mime = text(2);
                let ctx = engine
                    .get_mut(crate::canvas::CanvasId(id))
                    .ok_or("Canvas not found")?;
                let data_url = ctx.to_data_url(mime)?;
                return Ok(json!(data_url));
            }
            "canvasToBlob" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                let ctx = engine
                    .get_mut(crate::canvas::CanvasId(id))
                    .ok_or("Canvas not found")?;
                let bytes = ctx.to_blob_bytes()?;
                return Ok(json!(bytes));
            }
            "canvasGetImageData" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                let coords: Vec<i32> = serde_json::from_str(text(2)).unwrap_or_default();
                let x = coords.get(0).copied().unwrap_or(0);
                let y = coords.get(1).copied().unwrap_or(0);
                let w = coords.get(2).copied().unwrap_or(1);
                let h = coords.get(3).copied().unwrap_or(1);
                let ctx = engine
                    .get_mut(crate::canvas::CanvasId(id))
                    .ok_or("Canvas not found")?;
                let pixels = ctx.get_image_data(x, y, w, h)?;
                return Ok(json!(pixels));
            }
            "canvasPutImageData" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                #[derive(serde::Deserialize)]
                struct PutPayload {
                    data: Vec<u8>,
                    dx: i32,
                    dy: i32,
                    w: i32,
                    h: i32,
                }
                let payload: PutPayload =
                    serde_json::from_str(text(2)).map_err(|e| e.to_string())?;
                let ctx = engine
                    .get_mut(crate::canvas::CanvasId(id))
                    .ok_or("Canvas not found")?;
                ctx.put_image_data(&payload.data, payload.dx, payload.dy, payload.w, payload.h)?;
                return Ok(Value::Null);
            }
            "canvasMeasureText" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                let txt = text(2);
                let ctx = engine
                    .get(crate::canvas::CanvasId(id))
                    .ok_or("Canvas not found")?;
                let (width, ascent, descent) = ctx.measure_text(txt);
                return Ok(json!({
                    "width": width,
                    "actualBoundingBoxAscent": ascent,
                    "actualBoundingBoxDescent": descent,
                    "fontBoundingBoxAscent": ascent,
                    "fontBoundingBoxDescent": descent
                }));
            }
            "canvasTaint" => {
                let id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                if let Some(ctx) = engine.get_mut(crate::canvas::CanvasId(id)) {
                    ctx.origin_clean = false;
                }
                return Ok(Value::Null);
            }
            "canvasDrawImage" => {
                let target_id = text(1).parse::<u64>().map_err(|_| "Invalid canvas ID")?;
                let src_id = text(2)
                    .parse::<u64>()
                    .map_err(|_| "Invalid source canvas ID")?;
                let is_clean = text(3) != "tainted";
                let (src_pixels, src_w, src_h) = {
                    let src = engine
                        .get_mut(crate::canvas::CanvasId(src_id))
                        .ok_or("Source canvas not found")?;
                    let pixels = src.get_image_data(0, 0, src.width as i32, src.height as i32)?;
                    (pixels, src.width as i32, src.height as i32)
                };
                let target = engine
                    .get_mut(crate::canvas::CanvasId(target_id))
                    .ok_or("Target canvas not found")?;
                target.draw_image(
                    &src_pixels,
                    src_w,
                    src_h,
                    0.0,
                    0.0,
                    src_w as f32,
                    src_h as f32,
                    0.0,
                    0.0,
                    src_w as f32,
                    src_h as f32,
                    is_clean,
                )?;
                return Ok(Value::Null);
            }
            other => return Err(format!("Unknown host operation: {other}")),
        }
    })())
}
