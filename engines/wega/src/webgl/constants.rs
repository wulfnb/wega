//! WebGL constants conforming to OpenGL ES 2.0 / WebGL 1.0 and WebGL 2.0 specifications.

pub const DEPTH_BUFFER_BIT: u32 = 0x00000100;
pub const STENCIL_BUFFER_BIT: u32 = 0x00000400;
pub const COLOR_BUFFER_BIT: u32 = 0x00004000;

pub const POINTS: u32 = 0x0000;
pub const LINES: u32 = 0x0001;
pub const LINE_LOOP: u32 = 0x0002;
pub const LINE_STRIP: u32 = 0x0003;
pub const TRIANGLES: u32 = 0x0004;
pub const TRIANGLE_STRIP: u32 = 0x0005;
pub const TRIANGLE_FAN: u32 = 0x0006;

pub const ZERO: u32 = 0;
pub const ONE: u32 = 1;
pub const SRC_COLOR: u32 = 0x0300;
pub const ONE_MINUS_SRC_COLOR: u32 = 0x0301;
pub const SRC_ALPHA: u32 = 0x0302;
pub const ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
pub const DST_ALPHA: u32 = 0x0304;
pub const ONE_MINUS_DST_ALPHA: u32 = 0x0305;
pub const DST_COLOR: u32 = 0x0306;
pub const ONE_MINUS_DST_COLOR: u32 = 0x0307;
pub const SRC_ALPHA_SATURATE: u32 = 0x0308;

pub const FUNC_ADD: u32 = 0x8006;
pub const BLEND_EQUATION: u32 = 0x8009;
pub const BLEND_EQUATION_RGB: u32 = 0x8009;
pub const BLEND_EQUATION_ALPHA: u32 = 0x883D;
pub const FUNC_SUBTRACT: u32 = 0x800A;
pub const FUNC_REVERSE_SUBTRACT: u32 = 0x800B;

pub const BLEND_DST_RGB: u32 = 0x80C8;
pub const BLEND_SRC_RGB: u32 = 0x80C9;
pub const BLEND_DST_ALPHA: u32 = 0x80CA;
pub const BLEND_SRC_ALPHA: u32 = 0x80CB;
pub const CONSTANT_COLOR: u32 = 0x8001;
pub const ONE_MINUS_CONSTANT_COLOR: u32 = 0x8002;
pub const CONSTANT_ALPHA: u32 = 0x8003;
pub const ONE_MINUS_CONSTANT_ALPHA: u32 = 0x8004;
pub const BLEND_COLOR: u32 = 0x8005;

pub const ARRAY_BUFFER: u32 = 0x8892;
pub const ELEMENT_ARRAY_BUFFER: u32 = 0x8893;
pub const ARRAY_BUFFER_BINDING: u32 = 0x8894;
pub const ELEMENT_ARRAY_BUFFER_BINDING: u32 = 0x8895;

pub const STREAM_DRAW: u32 = 0x88E0;
pub const STATIC_DRAW: u32 = 0x88E4;
pub const DYNAMIC_DRAW: u32 = 0x88E8;

pub const BUFFER_SIZE: u32 = 0x8764;
pub const BUFFER_USAGE: u32 = 0x8765;

pub const CURRENT_VERTEX_ATTRIB: u32 = 0x8626;

pub const FRONT: u32 = 0x0404;
pub const BACK: u32 = 0x0405;
pub const FRONT_AND_BACK: u32 = 0x0408;

pub const CULL_FACE: u32 = 0x0B44;
pub const BLEND: u32 = 0x0BE2;
pub const DITHER: u32 = 0x0BD0;
pub const STENCIL_TEST: u32 = 0x0B90;
pub const DEPTH_TEST: u32 = 0x0B71;
pub const SCISSOR_TEST: u32 = 0x0C11;
pub const POLYGON_OFFSET_FILL: u32 = 0x8037;
pub const SAMPLE_ALPHA_TO_COVERAGE: u32 = 0x809E;
pub const SAMPLE_COVERAGE: u32 = 0x80A0;

pub const NO_ERROR: u32 = 0;
pub const INVALID_ENUM: u32 = 0x0500;
pub const INVALID_VALUE: u32 = 0x0501;
pub const INVALID_OPERATION: u32 = 0x0502;
pub const OUT_OF_MEMORY: u32 = 0x0505;
pub const CONTEXT_LOST_WEBGL: u32 = 0x9242;

pub const CW: u32 = 0x0900;
pub const CCW: u32 = 0x0901;

pub const DONT_CARE: u32 = 0x1100;
pub const FASTEST: u32 = 0x1101;
pub const NICEST: u32 = 0x1102;

pub const GENERATE_MIPMAP_HINT: u32 = 0x8192;

pub const BYTE: u32 = 0x1400;
pub const UNSIGNED_BYTE: u32 = 0x1401;
pub const SHORT: u32 = 0x1402;
pub const UNSIGNED_SHORT: u32 = 0x1403;
pub const INT: u32 = 0x1404;
pub const UNSIGNED_INT: u32 = 0x1405;
pub const FLOAT: u32 = 0x1406;

pub const DEPTH_COMPONENT: u32 = 0x1902;
pub const ALPHA: u32 = 0x1906;
pub const RGB: u32 = 0x1907;
pub const RGBA: u32 = 0x1908;
pub const LUMINANCE: u32 = 0x1909;
pub const LUMINANCE_ALPHA: u32 = 0x190A;

pub const FRAGMENT_SHADER: u32 = 0x8B30;
pub const VERTEX_SHADER: u32 = 0x8B31;

pub const COMPILE_STATUS: u32 = 0x8B81;
pub const DELETE_STATUS: u32 = 0x8B80;
pub const LINK_STATUS: u32 = 0x8B82;
pub const VALIDATE_STATUS: u32 = 0x8B83;
pub const ATTACHED_SHADERS: u32 = 0x8B85;
pub const ACTIVE_UNIFORMS: u32 = 0x8B86;
pub const ACTIVE_ATTRIBUTES: u32 = 0x8B89;
pub const SHADER_TYPE: u32 = 0x8B4F;

pub const TEXTURE_2D: u32 = 0x0DE1;
pub const TEXTURE_CUBE_MAP: u32 = 0x8513;
pub const TEXTURE_BINDING_2D: u32 = 0x8069;
pub const TEXTURE_BINDING_CUBE_MAP: u32 = 0x8514;
pub const TEXTURE_MAG_FILTER: u32 = 0x2800;
pub const TEXTURE_MIN_FILTER: u32 = 0x2801;
pub const TEXTURE_WRAP_S: u32 = 0x2802;
pub const TEXTURE_WRAP_T: u32 = 0x2803;

pub const NEAREST: u32 = 0x2600;
pub const LINEAR: u32 = 0x2601;
pub const NEAREST_MIPMAP_NEAREST: u32 = 0x2700;
pub const LINEAR_MIPMAP_NEAREST: u32 = 0x2701;
pub const NEAREST_MIPMAP_LINEAR: u32 = 0x2702;
pub const LINEAR_MIPMAP_LINEAR: u32 = 0x2703;

pub const REPEAT: u32 = 0x2901;
pub const CLAMP_TO_EDGE: u32 = 0x812F;
pub const MIRRORED_REPEAT: u32 = 0x8370;

pub const FRAMEBUFFER: u32 = 0x8D40;
pub const RENDERBUFFER: u32 = 0x8D41;
pub const COLOR_ATTACHMENT0: u32 = 0x8CE0;
pub const DEPTH_ATTACHMENT: u32 = 0x8D00;
pub const STENCIL_ATTACHMENT: u32 = 0x8D20;
pub const DEPTH_STENCIL_ATTACHMENT: u32 = 0x821A;
pub const FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;

pub const VIEWPORT: u32 = 0x0BA2;
pub const MAX_TEXTURE_SIZE: u32 = 0x0D33;
pub const MAX_VIEWPORT_DIMS: u32 = 0x0D3A;
pub const MAX_VERTEX_ATTRIBS: u32 = 0x8869;
pub const MAX_VERTEX_UNIFORM_VECTORS: u32 = 0x8DFB;
pub const MAX_VARYING_VECTORS: u32 = 0x8DFC;
pub const MAX_COMBINED_TEXTURE_IMAGE_UNITS: u32 = 0x8B4D;
pub const MAX_VERTEX_TEXTURE_IMAGE_UNITS: u32 = 0x8B4C;
pub const MAX_TEXTURE_IMAGE_UNITS: u32 = 0x8872;
pub const MAX_FRAGMENT_UNIFORM_VECTORS: u32 = 0x8DFD;
pub const MAX_RENDERBUFFER_SIZE: u32 = 0x84E8;

pub const UNPACK_FLIP_Y_WEBGL: u32 = 0x9240;
pub const UNPACK_PREMULTIPLY_ALPHA_WEBGL: u32 = 0x9241;
pub const UNPACK_COLORSPACE_CONVERSION_WEBGL: u32 = 0x9243;
pub const BROWSER_DEFAULT_WEBGL: u32 = 0x9244;
