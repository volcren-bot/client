use std::{
    ffi::{c_void, CString},
    mem::replace,
    ptr::{copy_nonoverlapping, null, null_mut},
    sync::{Mutex, OnceLock},
};

use gl::{
    types::{
        GLbitfield, GLchar, GLenum, GLfloat, GLint, GLintptr, GLsizei, GLsizeiptr, GLsync,
        GLuint, GLuint64, GLvoid,
    },
    ALREADY_SIGNALED, BGRA, BLEND, BLEND_DST, BLEND_SRC, CLAMP_TO_EDGE, CONDITION_SATISFIED,
    FRAGMENT_SHADER, LINEAR, MAP_COHERENT_BIT, MAP_PERSISTENT_BIT, MAP_READ_BIT,
    ONE_MINUS_SRC_ALPHA, PIXEL_PACK_BUFFER, PIXEL_PACK_BUFFER_BINDING, POINTS, RGBA8, SRC_ALPHA,
    SYNC_FLUSH_COMMANDS_BIT, SYNC_GPU_COMMANDS_COMPLETE, TEXTURE0, TEXTURE_2D, TEXTURE_MAG_FILTER,
    TEXTURE_MIN_FILTER, TEXTURE_WRAP_S, TEXTURE_WRAP_T, TRIANGLE_STRIP, UNSIGNED_BYTE,
    VERTEX_SHADER,
};

use windows::{
    core::PCSTR,
    Win32::{
        Graphics::OpenGL::{
            glBindTexture, glBlendFunc, glDeleteTextures, glDisable, glDrawArrays, glEnable,
            glGenTextures, glGetIntegerv, glIsEnabled, glPointSize, glReadPixels, glTexParameteri,
            glTexSubImage2D, glViewport, wglGetProcAddress,
        },
        System::LibraryLoader::{GetModuleHandleA, GetProcAddress},
    },
};

type GlGenBuffersFn = unsafe extern "system" fn(n: GLsizei, buffers: *mut GLuint);
type GlDeleteBuffersFn = unsafe extern "system" fn(n: GLsizei, buffers: *const GLuint);
type GlBindBufferFn = unsafe extern "system" fn(target: GLenum, buffer: GLuint);
type GlBufferStorageFn = unsafe extern "system" fn(
    target: GLenum,
    size: GLsizeiptr,
    data: *const GLvoid,
    flags: GLbitfield,
);
type GlMapBufferRangeFn = unsafe extern "system" fn(
    target: GLenum,
    offset: GLintptr,
    length: GLsizeiptr,
    access: GLbitfield,
) -> *mut c_void;
type GlFenceSyncFn = unsafe extern "system" fn(condition: GLenum, flags: GLbitfield) -> GLsync;
type GlClientWaitSyncFn =
    unsafe extern "system" fn(sync: GLsync, flags: GLbitfield, timeout: GLuint64) -> GLenum;
type GlDeleteSyncFn = unsafe extern "system" fn(sync: GLsync);
type GlCreateShaderFn = unsafe extern "system" fn(shader_type: GLenum) -> GLuint;
type GlShaderSourceFn = unsafe extern "system" fn(
    shader: GLuint,
    count: GLsizei,
    string: *const *const GLchar,
    length: *const GLint,
);
type GlCompileShaderFn = unsafe extern "system" fn(shader: GLuint);
type GlCreateProgramFn = unsafe extern "system" fn() -> GLuint;
type GlAttachShaderFn = unsafe extern "system" fn(program: GLuint, shader: GLuint);
type GlLinkProgramFn = unsafe extern "system" fn(program: GLuint);
type GlDeleteShaderFn = unsafe extern "system" fn(shader: GLuint);
type GlBindVertexArrayFn = unsafe extern "system" fn(array: GLuint);
type GlUseProgramFn = unsafe extern "system" fn(program: GLuint);
type GlCreateVertexArraysFn = unsafe extern "system" fn(n: GLsizei, arrays: *mut GLuint);
type GlGenVertexArraysFn = unsafe extern "system" fn(n: GLsizei, arrays: *mut GLuint);
type GLUniform2FvFn =
    unsafe extern "system" fn(location: GLint, count: GLsizei, value: *const GLfloat);
type GLTexStorage2DFn = unsafe extern "system" fn(
    target: GLenum,
    levels: GLsizei,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
);
type GLActiveTextureFn = unsafe extern "system" fn(texture: GLenum);
type GLBindTextureFn = unsafe extern "system" fn(target: GLenum, texture: GLuint);
type GLUniform1iFn = unsafe extern "system" fn(location: GLint, v0: GLint);
type GLGetUniformLocationFn =
    unsafe extern "system" fn(program: GLuint, name: *const GLchar) -> GLint;

static GL_GEN_BUFFERS: OnceLock<GlGenBuffersFn> = OnceLock::new();
static GL_DELETE_BUFFERS: OnceLock<GlDeleteBuffersFn> = OnceLock::new();
static GL_BIND_BUFFER: OnceLock<GlBindBufferFn> = OnceLock::new();
static GL_BUFFER_STORAGE: OnceLock<GlBufferStorageFn> = OnceLock::new();
static GL_MAP_BUFFER_RANGE: OnceLock<GlMapBufferRangeFn> = OnceLock::new();
static GL_FENCE_SYNC: OnceLock<GlFenceSyncFn> = OnceLock::new();
static GL_CLIENT_WAIT_SYNC: OnceLock<GlClientWaitSyncFn> = OnceLock::new();
static GL_DELETE_SYNC: OnceLock<GlDeleteSyncFn> = OnceLock::new();
static GL_CREATE_SHADER: OnceLock<GlCreateShaderFn> = OnceLock::new();
static GL_SHADER_SOURCE: OnceLock<GlShaderSourceFn> = OnceLock::new();
static GL_COMPILE_SHADER: OnceLock<GlCompileShaderFn> = OnceLock::new();
static GL_CREATE_PROGRAM: OnceLock<GlCreateProgramFn> = OnceLock::new();
static GL_ATTACH_SHADER: OnceLock<GlAttachShaderFn> = OnceLock::new();
static GL_LINK_PROGRAM: OnceLock<GlLinkProgramFn> = OnceLock::new();
static GL_DELETE_SHADER: OnceLock<GlDeleteShaderFn> = OnceLock::new();
static GL_BIND_VERTEX_ARRAY: OnceLock<GlBindVertexArrayFn> = OnceLock::new();
static GL_USE_PROGRAM: OnceLock<GlUseProgramFn> = OnceLock::new();
static GL_CREATE_VERTEX_ARRAYS: OnceLock<GlCreateVertexArraysFn> = OnceLock::new();
static GL_GEN_VERTEX_ARRAYS: OnceLock<GlGenVertexArraysFn> = OnceLock::new();
static GL_UNIFORM_2FV: OnceLock<GLUniform2FvFn> = OnceLock::new();
static GL_TEX_STORAGE_2D: OnceLock<GLTexStorage2DFn> = OnceLock::new();
static GL_ACTIVE_TEXTURE: OnceLock<GLActiveTextureFn> = OnceLock::new();
static GL_BIND_TEXTURE: OnceLock<GLBindTextureFn> = OnceLock::new();
static GL_UNIFORM_1I: OnceLock<GLUniform1iFn> = OnceLock::new();
static GL_GET_UNIFORM_LOCATION: OnceLock<GLGetUniformLocationFn> = OnceLock::new();

static POINT_SHADER: OnceLock<GLuint> = OnceLock::new();
static VAO: OnceLock<GLuint> = OnceLock::new();
static OVERLAY_SHADER: OnceLock<GLuint> = OnceLock::new();

pub fn restore_state(prev_program: i32, prev_vao: i32) {
    let use_program = *GL_USE_PROGRAM.get().unwrap();
    let bind_vertex_array = *GL_BIND_VERTEX_ARRAY.get().unwrap();

    unsafe {
        use_program(prev_program as GLuint);
        bind_vertex_array(prev_vao as GLuint);
    }
}

pub unsafe fn load_opengl_extensions() -> bool {
    let load_fn = |name: &str| -> *const c_void {
        let cname = CString::new(name).unwrap();
        let pcstr = PCSTR(cname.as_ptr() as *const u8);
        match wglGetProcAddress(pcstr) {
            Some(f) if !matches!(f as usize, usize::MAX | 0..=3) => f as *const c_void,
            _ => match GetModuleHandleA(PCSTR(c"opengl32.dll".as_ptr().cast())) {
                Ok(module) => match GetProcAddress(module, pcstr) {
                    Some(f) => f as *const c_void,
                    None => null(),
                },
                Err(_) => null(),
            },
        }
    };

    macro_rules! load {
        ($sym:ident, $type:ty, $name:literal) => {{
            if $sym.get().is_some() {
                true
            } else {
                let ptr = load_fn($name);
                if ptr.is_null() {
                    return false;
                }
                $sym.set(std::mem::transmute::<*const c_void, $type>(ptr))
                    .is_ok()
            }
        }};
    }

    load!(GL_GEN_BUFFERS, GlGenBuffersFn, "glGenBuffers")
        && load!(GL_DELETE_BUFFERS, GlDeleteBuffersFn, "glDeleteBuffers")
        && load!(GL_BIND_BUFFER, GlBindBufferFn, "glBindBuffer")
        && load!(GL_BUFFER_STORAGE, GlBufferStorageFn, "glBufferStorage")
        && load!(GL_MAP_BUFFER_RANGE, GlMapBufferRangeFn, "glMapBufferRange")
        && load!(GL_FENCE_SYNC, GlFenceSyncFn, "glFenceSync")
        && load!(GL_CLIENT_WAIT_SYNC, GlClientWaitSyncFn, "glClientWaitSync")
        && load!(GL_DELETE_SYNC, GlDeleteSyncFn, "glDeleteSync")
        && load!(GL_CREATE_SHADER, GlCreateShaderFn, "glCreateShader")
        && load!(GL_SHADER_SOURCE, GlShaderSourceFn, "glShaderSource")
        && load!(GL_COMPILE_SHADER, GlCompileShaderFn, "glCompileShader")
        && load!(GL_CREATE_PROGRAM, GlCreateProgramFn, "glCreateProgram")
        && load!(GL_ATTACH_SHADER, GlAttachShaderFn, "glAttachShader")
        && load!(GL_LINK_PROGRAM, GlLinkProgramFn, "glLinkProgram")
        && load!(GL_DELETE_SHADER, GlDeleteShaderFn, "glDeleteShader")
        && load!(
            GL_BIND_VERTEX_ARRAY,
            GlBindVertexArrayFn,
            "glBindVertexArray"
        )
        && load!(GL_USE_PROGRAM, GlUseProgramFn, "glUseProgram")
        && load!(
            GL_CREATE_VERTEX_ARRAYS,
            GlCreateVertexArraysFn,
            "glCreateVertexArrays"
        )
        && load!(
            GL_GEN_VERTEX_ARRAYS,
            GlGenVertexArraysFn,
            "glGenVertexArrays"
        )
        && load!(GL_UNIFORM_2FV, GLUniform2FvFn, "glUniform2fv")
        && load!(GL_TEX_STORAGE_2D, GLTexStorage2DFn, "glTexStorage2D")
        && load!(GL_ACTIVE_TEXTURE, GLActiveTextureFn, "glActiveTexture")
        && load!(GL_BIND_TEXTURE, GLBindTextureFn, "glBindTexture")
        && load!(GL_UNIFORM_1I, GLUniform1iFn, "glUniform1i")
        && load!(
            GL_GET_UNIFORM_LOCATION,
            GLGetUniformLocationFn,
            "glGetUniformLocation"
        )
}

// Two PBOs mapped for life: each frame reads into one and copies the other (the previous frame) to shared memory, a fence per PBO.
struct FrameBuffers {
    pbos: [GLuint; 2],
    mapped: [*const u8; 2],
    fences: [GLsync; 2],
    size: i32,
    index: usize,
    failed: bool,
}

unsafe impl Send for FrameBuffers {}

static FRAME_BUFFERS: Mutex<FrameBuffers> = Mutex::new(FrameBuffers {
    pbos: [0; 2],
    mapped: [null(); 2],
    fences: [null(); 2],
    size: 0,
    index: 0,
    failed: false,
});

// The read being waited for is a frame old, so it has normally landed; this only caps a stuck GPU.
const READ_TIMEOUT_NS: GLuint64 = 1_000_000_000;

impl FrameBuffers {
    unsafe fn create(&mut self, size: i32) -> bool {
        let gl_gen_buffers = *GL_GEN_BUFFERS.get().unwrap();
        let gl_bind_buffer = *GL_BIND_BUFFER.get().unwrap();
        let gl_buffer_storage = *GL_BUFFER_STORAGE.get().unwrap();
        let gl_map_buffer_range = *GL_MAP_BUFFER_RANGE.get().unwrap();

        let flags = MAP_READ_BIT | MAP_PERSISTENT_BIT | MAP_COHERENT_BIT;
        gl_gen_buffers(2, self.pbos.as_mut_ptr());
        for (pbo, mapped) in self.pbos.iter().zip(self.mapped.iter_mut()) {
            gl_bind_buffer(PIXEL_PACK_BUFFER, *pbo);
            gl_buffer_storage(PIXEL_PACK_BUFFER, size as GLsizeiptr, null(), flags);
            *mapped = gl_map_buffer_range(PIXEL_PACK_BUFFER, 0, size as GLsizeiptr, flags) as *const u8;
            if mapped.is_null() {
                return false;
            }
        }
        self.size = size;
        true
    }

    unsafe fn release(&mut self) {
        let gl_delete_sync = *GL_DELETE_SYNC.get().unwrap();
        let gl_delete_buffers = *GL_DELETE_BUFFERS.get().unwrap();

        for fence in self.fences.iter_mut() {
            if !fence.is_null() {
                gl_delete_sync(replace(fence, null()));
            }
        }
        // Deleting a mapped buffer unmaps it.
        if self.pbos[0] != 0 {
            gl_delete_buffers(2, self.pbos.as_ptr());
        }
        self.pbos = [0; 2];
        self.mapped = [null(); 2];
        self.size = 0;
        self.index = 0;
    }
}

pub fn read_frame(offset_x: i32, offset_y: i32, width: i32, height: i32, size: i32, dest: *mut u8) {
    if dest.is_null() {
        return;
    }

    let gl_bind_buffer = *GL_BIND_BUFFER.get().unwrap();
    let gl_fence_sync = *GL_FENCE_SYNC.get().unwrap();
    let gl_client_wait_sync = *GL_CLIENT_WAIT_SYNC.get().unwrap();
    let gl_delete_sync = *GL_DELETE_SYNC.get().unwrap();

    let mut buffers = FRAME_BUFFERS.lock().unwrap();
    if buffers.failed {
        return;
    }

    let row_stride = (width * 4) as usize;

    let mut prev_pbo: GLint = 0;
    unsafe { glGetIntegerv(PIXEL_PACK_BUFFER_BINDING, &mut prev_pbo) };

    unsafe {
        if buffers.size != size {
            buffers.release();
            if !buffers.create(size) {
                println!("[Agent]: Cannot map the frame capture buffers, capture is off.\r\n");
                buffers.release();
                buffers.failed = true;
                gl_bind_buffer(PIXEL_PACK_BUFFER, prev_pbo as GLuint);
                return;
            }
        }

        let read_index = buffers.index;
        let copy_index = (read_index + 1) % 2;

        // Read pixels into the read_index PBO
        gl_bind_buffer(PIXEL_PACK_BUFFER, buffers.pbos[read_index]);
        glReadPixels(offset_x, offset_y, width, height, BGRA, UNSIGNED_BYTE, null_mut());
        buffers.fences[read_index] = gl_fence_sync(SYNC_GPU_COMMANDS_COMPLETE, 0);

        // Copy out the previous frame's PBO once its read has landed
        let fence = replace(&mut buffers.fences[copy_index], null());
        if !fence.is_null() {
            let status = gl_client_wait_sync(fence, SYNC_FLUSH_COMMANDS_BIT, READ_TIMEOUT_NS);
            gl_delete_sync(fence);

            if status == ALREADY_SIGNALED || status == CONDITION_SATISFIED {
                let src = buffers.mapped[copy_index];
                for row in 0..height as usize {
                    let src_row = src.add(row * row_stride);
                    let dest_row = dest.add((height as usize - 1 - row) * row_stride);
                    copy_nonoverlapping(src_row, dest_row, row_stride);
                }
            }
        }

        buffers.index = copy_index; // Swap indices

        gl_bind_buffer(PIXEL_PACK_BUFFER, prev_pbo as GLuint);
    }
}

// Frees the PBOs while capture is off so it restarts fresh instead of copying a stale frame.
pub fn release_frame_buffers() {
    let mut buffers = FRAME_BUFFERS.lock().unwrap();
    if buffers.pbos[0] != 0 {
        unsafe { buffers.release() };
    }
}

fn compile_shader(source: &str, shader_type: GLenum) -> GLuint {
    let gl_create_shader = *GL_CREATE_SHADER.get().unwrap();
    let gl_shader_source = *GL_SHADER_SOURCE.get().unwrap();
    let gl_compile_shader = *GL_COMPILE_SHADER.get().unwrap();

    let c_str = CString::new(source.as_bytes()).unwrap();

    unsafe {
        let shader = gl_create_shader(shader_type);
        gl_shader_source(shader, 1, &c_str.as_ptr(), null());
        gl_compile_shader(shader);
        shader
    }
}

fn init_gl_resources_overlay() {
    let create_program = *GL_CREATE_PROGRAM.get().unwrap();
    let gen_vertex_arrays = *GL_GEN_VERTEX_ARRAYS.get().unwrap();
    let attach_shader = *GL_ATTACH_SHADER.get().unwrap();
    let link_program = *GL_LINK_PROGRAM.get().unwrap();
    let delete_shader = *GL_DELETE_SHADER.get().unwrap();

    const VS_SRC: &str = r#"
    #version 460 core
    const vec2 verts[4] = vec2[4](
        vec2(-1.0, -1.0),
        vec2( 1.0, -1.0),
        vec2(-1.0,  1.0),
        vec2( 1.0,  1.0)
    );
    out vec2 texCoord;
    void main() {
        texCoord = (verts[gl_VertexID].xy + 1.0) * 0.5;
        gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
    }"#;

    const FS_SRC: &str = r#"
    #version 460 core
    in vec2 texCoord;
    out vec4 FragColor;
    uniform sampler2D screenTex;
    void main() {
        FragColor = texture(screenTex, vec2(texCoord.x, 1.0 - texCoord.y));
    }"#;

    let vs = compile_shader(VS_SRC, VERTEX_SHADER);
    let fs = compile_shader(FS_SRC, FRAGMENT_SHADER);

    unsafe {
        let program = create_program();
        attach_shader(program, vs);
        attach_shader(program, fs);
        link_program(program);

        delete_shader(vs);
        delete_shader(fs);
        OVERLAY_SHADER.set(program).unwrap();
    };

    if VAO.get().is_none() {
        let mut vao = 0;
        unsafe { gen_vertex_arrays(1, &mut vao) };
        VAO.set(vao).unwrap();
    }
}

// The overlay texture and the `overlay_serial` it holds (`None`: nothing yet, or Simba's per-frame overlay).
struct OverlayTexture {
    texture: GLuint,
    width: i32,
    height: i32,
    serial: Option<u32>,
}

static OVERLAY_TEXTURE: Mutex<OverlayTexture> = Mutex::new(OverlayTexture {
    texture: 0,
    width: 0,
    height: 0,
    serial: None,
});

fn resize_overlay_texture(overlay: &mut OverlayTexture, width: i32, height: i32) {
    if overlay.texture != 0 && overlay.width == width && overlay.height == height {
        return;
    }

    if overlay.texture != 0 {
        unsafe { glDeleteTextures(1, &overlay.texture) };
    }

    overlay.texture = init_gl_texture(width, height);
    overlay.width = width;
    overlay.height = height;
    overlay.serial = None;
}

fn init_gl_texture(width: i32, height: i32) -> GLuint {
    let gl_tex_storage_2d = *GL_TEX_STORAGE_2D.get().unwrap();
    let mut texture: GLuint = 0;
    unsafe {
        glGenTextures(1, &mut texture);
        glBindTexture(TEXTURE_2D, texture);
        glTexParameteri(TEXTURE_2D, TEXTURE_MIN_FILTER, LINEAR as i32);
        glTexParameteri(TEXTURE_2D, TEXTURE_MAG_FILTER, LINEAR as i32);
        glTexParameteri(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE as i32);
        glTexParameteri(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE as i32);
        gl_tex_storage_2d(TEXTURE_2D, 1, RGBA8, width, height);
    };
    texture
}

// `serial` is the overlay's `overlay_serial`, or `None` to upload it every frame (Simba).
pub fn draw_overlay(
    offset_x: i32,
    offset_y: i32,
    width: i32,
    height: i32,
    src: *const u8,
    serial: Option<u32>,
) {
    if width <= 0 || height <= 0 {
        return;
    }

    if OVERLAY_SHADER.get().is_none() {
        init_gl_resources_overlay();
    }

    let mut overlay = OVERLAY_TEXTURE.lock().unwrap();
    resize_overlay_texture(&mut overlay, width, height);
    let texture = overlay.texture;
    let upload = serial.is_none() || overlay.serial != serial;
    overlay.serial = serial;
    drop(overlay);

    let use_program = *GL_USE_PROGRAM.get().unwrap();
    let active_texture = *GL_ACTIVE_TEXTURE.get().unwrap();
    let bind_texture = *GL_BIND_TEXTURE.get().unwrap();
    let bind_vertex_array = *GL_BIND_VERTEX_ARRAY.get().unwrap();

    let program = *OVERLAY_SHADER.get().unwrap();
    let vao = *VAO.get().unwrap();

    unsafe {
        glViewport(offset_x, offset_y, width, height);
        // Upload only when the overlay changed since the last upload
        if upload {
            glBindTexture(TEXTURE_2D, texture);
            glTexSubImage2D(
                TEXTURE_2D,
                0,
                0,
                0,
                width,
                height,
                BGRA,
                UNSIGNED_BYTE,
                src as *const c_void,
            );
        }

        use_program(program);
        active_texture(TEXTURE0);
        bind_texture(TEXTURE_2D, texture);

        let blend_was_enabled = glIsEnabled(BLEND) != 0;
        let mut prev_src = 0;
        let mut prev_dst = 0;
        glGetIntegerv(BLEND_SRC, &mut prev_src);
        glGetIntegerv(BLEND_DST, &mut prev_dst);
        glEnable(BLEND);
        glBlendFunc(SRC_ALPHA, ONE_MINUS_SRC_ALPHA);

        bind_vertex_array(vao);
        glDrawArrays(TRIANGLE_STRIP, 0, 4);

        glBlendFunc(prev_src as GLenum, prev_dst as GLenum);
        if !blend_was_enabled {
            glDisable(BLEND);
        }
    }
}

fn init_gl_resources() {
    let create_program = *GL_CREATE_PROGRAM.get().unwrap();
    let attach_shader = *GL_ATTACH_SHADER.get().unwrap();
    let link_program = *GL_LINK_PROGRAM.get().unwrap();
    let delete_shader = *GL_DELETE_SHADER.get().unwrap();
    let gen_vertex_arrays = *GL_GEN_VERTEX_ARRAYS.get().unwrap();

    const VS_SRC: &str = r#"
    #version 460 core
    layout(location = 0) uniform vec2 pointPos;
    void main() {
        gl_Position = vec4(pointPos, 0.0, 1.0);
    }"#;

    const FS_SRC: &str = r#"
    #version 460 core
    layout(location = 1) uniform vec2 colorRG;
    layout(location = 2) uniform vec2 colorBStyle;
    layout(location = 3) uniform vec2 pointSize;
    out vec4 FragColor;
    void main() {
        vec2 coord = gl_PointCoord * 2.0 - 1.0;
        // Arm half width in coord units: 1/8 of the size, at least 1 px.
        float arm = max(2.0 / pointSize.x, 0.125);
        if (colorBStyle.y > 2.5) {
            if (abs(abs(coord.x) - abs(coord.y)) > arm * 1.4142) discard;
        } else if (colorBStyle.y > 1.5) {
            if (length(coord) > 1.0) discard;
        } else if (min(abs(coord.x), abs(coord.y)) > arm) {
            discard;
        }
        FragColor = vec4(colorRG, colorBStyle.x, 1.0);
    }"#;

    let vs = compile_shader(VS_SRC, VERTEX_SHADER);
    let fs = compile_shader(FS_SRC, FRAGMENT_SHADER);

    unsafe {
        let program = create_program();
        attach_shader(program, vs);
        attach_shader(program, fs);
        link_program(program);

        delete_shader(vs);
        delete_shader(fs);

        POINT_SHADER.set(program).unwrap();
    };

    if VAO.get().is_none() {
        let mut vao = 0;
        unsafe { gen_vertex_arrays(1, &mut vao) };
        VAO.set(vao).unwrap();
    }
}

pub fn draw_cursor(x: i32, y: i32, w: i32, h: i32, style: u8, color: [u8; 3], size: u8) {
    if POINT_SHADER.get().is_none() {
        init_gl_resources();
    }

    let use_program = *GL_USE_PROGRAM.get().unwrap();
    let bind_vertex_array = *GL_BIND_VERTEX_ARRAY.get().unwrap();
    let uniform_2fv = *GL_UNIFORM_2FV.get().unwrap();

    let program = *POINT_SHADER.get().unwrap();
    let vao = *VAO.get().unwrap();

    let x_ndc = (x as f32 / w as f32) * 2.0 - 1.0;
    let y_ndc = 1.0 - (y as f32 / h as f32) * 2.0;
    let pos = [x_ndc, y_ndc];
    let [r, g, b] = color.map(|c| c as f32 / 255.0);
    let color_rg = [r, g];
    let point_size = [size as f32, 0.0];
    let color_b_style = [b, style as f32];

    unsafe {
        use_program(program);
        bind_vertex_array(vao);
        glPointSize(size as f32);
        uniform_2fv(0, 1, pos.as_ptr());
        uniform_2fv(1, 1, color_rg.as_ptr());
        uniform_2fv(2, 1, color_b_style.as_ptr());
        uniform_2fv(3, 1, point_size.as_ptr());
        glDrawArrays(POINTS, 0, 1);
    };
}
