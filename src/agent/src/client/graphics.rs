use std::{
    ffi::{c_void, CStr},
    mem::replace,
    ptr::{copy_nonoverlapping, null, null_mut},
    sync::{Mutex, OnceLock},
};

use gl::{
    types::{
        GLbitfield, GLchar, GLenum, GLfloat, GLint, GLintptr, GLsizei, GLsizeiptr, GLsync, GLuint,
        GLuint64, GLvoid,
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

macro_rules! gl_functions {
    ($($field:ident = $name:literal: fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        struct Gl {
            $($field: unsafe extern "system" fn($($arg),*) $(-> $ret)?,)*
        }

        unsafe fn load_gl() -> Option<Gl> {
            Some(Gl {
                $($field: std::mem::transmute::<
                    *const c_void,
                    unsafe extern "system" fn($($arg),*) $(-> $ret)?,
                >(proc_address($name)?),)*
            })
        }
    };
}

gl_functions! {
    gen_buffers = c"glGenBuffers": fn(GLsizei, *mut GLuint);
    delete_buffers = c"glDeleteBuffers": fn(GLsizei, *const GLuint);
    bind_buffer = c"glBindBuffer": fn(GLenum, GLuint);
    buffer_storage = c"glBufferStorage": fn(GLenum, GLsizeiptr, *const GLvoid, GLbitfield);
    map_buffer_range = c"glMapBufferRange": fn(GLenum, GLintptr, GLsizeiptr, GLbitfield) -> *mut c_void;
    fence_sync = c"glFenceSync": fn(GLenum, GLbitfield) -> GLsync;
    client_wait_sync = c"glClientWaitSync": fn(GLsync, GLbitfield, GLuint64) -> GLenum;
    delete_sync = c"glDeleteSync": fn(GLsync);
    create_shader = c"glCreateShader": fn(GLenum) -> GLuint;
    shader_source = c"glShaderSource": fn(GLuint, GLsizei, *const *const GLchar, *const GLint);
    compile_shader = c"glCompileShader": fn(GLuint);
    create_program = c"glCreateProgram": fn() -> GLuint;
    attach_shader = c"glAttachShader": fn(GLuint, GLuint);
    link_program = c"glLinkProgram": fn(GLuint);
    delete_shader = c"glDeleteShader": fn(GLuint);
    bind_vertex_array = c"glBindVertexArray": fn(GLuint);
    use_program = c"glUseProgram": fn(GLuint);
    gen_vertex_arrays = c"glGenVertexArrays": fn(GLsizei, *mut GLuint);
    uniform_2fv = c"glUniform2fv": fn(GLint, GLsizei, *const GLfloat);
    tex_storage_2d = c"glTexStorage2D": fn(GLenum, GLsizei, GLenum, GLsizei, GLsizei);
    active_texture = c"glActiveTexture": fn(GLenum);
    bind_texture = c"glBindTexture": fn(GLenum, GLuint);
}

static GL: OnceLock<Gl> = OnceLock::new();

static POINT_SHADER: OnceLock<GLuint> = OnceLock::new();
static VAO: OnceLock<GLuint> = OnceLock::new();
static OVERLAY_SHADER: OnceLock<GLuint> = OnceLock::new();

fn gl() -> &'static Gl {
    GL.get().unwrap()
}

pub fn restore_state(prev_program: i32, prev_vao: i32) {
    let gl = gl();
    unsafe {
        (gl.use_program)(prev_program as GLuint);
        (gl.bind_vertex_array)(prev_vao as GLuint);
    }
}

unsafe fn proc_address(name: &CStr) -> Option<*const c_void> {
    let name = PCSTR(name.as_ptr().cast());
    match wglGetProcAddress(name) {
        Some(f) if !matches!(f as usize, usize::MAX | 0..=3) => Some(f as *const c_void),
        _ => {
            let module = GetModuleHandleA(PCSTR(c"opengl32.dll".as_ptr().cast())).ok()?;
            GetProcAddress(module, name).map(|f| f as *const c_void)
        }
    }
}

pub unsafe fn load_opengl_extensions() -> bool {
    GL.get().is_some() || load_gl().is_some_and(|gl| GL.set(gl).is_ok())
}

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

const READ_TIMEOUT_NS: GLuint64 = 1_000_000_000;

impl FrameBuffers {
    unsafe fn create(&mut self, size: i32) -> bool {
        let gl = gl();
        let flags = MAP_READ_BIT | MAP_PERSISTENT_BIT | MAP_COHERENT_BIT;
        (gl.gen_buffers)(2, self.pbos.as_mut_ptr());
        for (pbo, mapped) in self.pbos.iter().zip(self.mapped.iter_mut()) {
            (gl.bind_buffer)(PIXEL_PACK_BUFFER, *pbo);
            (gl.buffer_storage)(PIXEL_PACK_BUFFER, size as GLsizeiptr, null(), flags);
            *mapped =
                (gl.map_buffer_range)(PIXEL_PACK_BUFFER, 0, size as GLsizeiptr, flags) as *const u8;
            if mapped.is_null() {
                return false;
            }
        }
        self.size = size;
        true
    }

    unsafe fn release(&mut self) {
        let gl = gl();
        for fence in self.fences.iter_mut() {
            if !fence.is_null() {
                (gl.delete_sync)(replace(fence, null()));
            }
        }
        if self.pbos[0] != 0 {
            (gl.delete_buffers)(2, self.pbos.as_ptr());
        }
        self.pbos = [0; 2];
        self.mapped = [null(); 2];
        self.size = 0;
        self.index = 0;
    }
}

pub fn read_frame(offset_x: i32, offset_y: i32, width: i32, height: i32, dest: *mut u8) {
    if dest.is_null() {
        return;
    }

    let gl = gl();
    let size = width * height * 4;
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
                (gl.bind_buffer)(PIXEL_PACK_BUFFER, prev_pbo as GLuint);
                return;
            }
        }

        let read_index = buffers.index;
        let copy_index = (read_index + 1) % 2;

        (gl.bind_buffer)(PIXEL_PACK_BUFFER, buffers.pbos[read_index]);
        glReadPixels(
            offset_x,
            offset_y,
            width,
            height,
            BGRA,
            UNSIGNED_BYTE,
            null_mut(),
        );
        buffers.fences[read_index] = (gl.fence_sync)(SYNC_GPU_COMMANDS_COMPLETE, 0);

        let fence = replace(&mut buffers.fences[copy_index], null());
        if !fence.is_null() {
            let status = (gl.client_wait_sync)(fence, SYNC_FLUSH_COMMANDS_BIT, READ_TIMEOUT_NS);
            (gl.delete_sync)(fence);

            if status == ALREADY_SIGNALED || status == CONDITION_SATISFIED {
                let src = buffers.mapped[copy_index];
                for row in 0..height as usize {
                    let src_row = src.add(row * row_stride);
                    let dest_row = dest.add((height as usize - 1 - row) * row_stride);
                    copy_nonoverlapping(src_row, dest_row, row_stride);
                }
            }
        }

        buffers.index = copy_index;
        (gl.bind_buffer)(PIXEL_PACK_BUFFER, prev_pbo as GLuint);
    }
}

pub fn release_frame_buffers() {
    let mut buffers = FRAME_BUFFERS.lock().unwrap();
    if buffers.pbos[0] != 0 {
        unsafe { buffers.release() };
    }
}

unsafe fn compile_shader(gl: &Gl, source: &CStr, shader_type: GLenum) -> GLuint {
    let shader = (gl.create_shader)(shader_type);
    (gl.shader_source)(shader, 1, &source.as_ptr(), null());
    (gl.compile_shader)(shader);
    shader
}

fn link_program(vertex: &CStr, fragment: &CStr) -> GLuint {
    let gl = gl();
    unsafe {
        let vs = compile_shader(gl, vertex, VERTEX_SHADER);
        let fs = compile_shader(gl, fragment, FRAGMENT_SHADER);
        let program = (gl.create_program)();
        (gl.attach_shader)(program, vs);
        (gl.attach_shader)(program, fs);
        (gl.link_program)(program);
        (gl.delete_shader)(vs);
        (gl.delete_shader)(fs);
        program
    }
}

fn vao() -> GLuint {
    *VAO.get_or_init(|| {
        let mut vao = 0;
        unsafe { (gl().gen_vertex_arrays)(1, &mut vao) };
        vao
    })
}

const OVERLAY_VS: &CStr = c"
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
    }";

const OVERLAY_FS: &CStr = c"
    #version 460 core
    in vec2 texCoord;
    out vec4 FragColor;
    uniform sampler2D screenTex;
    void main() {
        FragColor = texture(screenTex, vec2(texCoord.x, 1.0 - texCoord.y));
    }";

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
    let mut texture: GLuint = 0;
    unsafe {
        glGenTextures(1, &mut texture);
        glBindTexture(TEXTURE_2D, texture);
        glTexParameteri(TEXTURE_2D, TEXTURE_MIN_FILTER, LINEAR as i32);
        glTexParameteri(TEXTURE_2D, TEXTURE_MAG_FILTER, LINEAR as i32);
        glTexParameteri(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE as i32);
        glTexParameteri(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE as i32);
        (gl().tex_storage_2d)(TEXTURE_2D, 1, RGBA8, width, height);
    };
    texture
}

pub fn draw_overlay(
    offset_x: i32,
    offset_y: i32,
    width: i32,
    height: i32,
    src: *const u8,
    serial: u32,
) {
    if width <= 0 || height <= 0 {
        return;
    }

    let program = *OVERLAY_SHADER.get_or_init(|| link_program(OVERLAY_VS, OVERLAY_FS));
    let vao = vao();

    let mut overlay = OVERLAY_TEXTURE.lock().unwrap();
    resize_overlay_texture(&mut overlay, width, height);
    let texture = overlay.texture;
    let upload = overlay.serial != Some(serial);
    overlay.serial = Some(serial);
    drop(overlay);

    let gl = gl();
    unsafe {
        glViewport(offset_x, offset_y, width, height);
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

        (gl.use_program)(program);
        (gl.active_texture)(TEXTURE0);
        (gl.bind_texture)(TEXTURE_2D, texture);

        let blend_was_enabled = glIsEnabled(BLEND) != 0;
        let mut prev_src = 0;
        let mut prev_dst = 0;
        glGetIntegerv(BLEND_SRC, &mut prev_src);
        glGetIntegerv(BLEND_DST, &mut prev_dst);
        glEnable(BLEND);
        glBlendFunc(SRC_ALPHA, ONE_MINUS_SRC_ALPHA);

        (gl.bind_vertex_array)(vao);
        glDrawArrays(TRIANGLE_STRIP, 0, 4);

        glBlendFunc(prev_src as GLenum, prev_dst as GLenum);
        if !blend_was_enabled {
            glDisable(BLEND);
        }
    }
}

const POINT_VS: &CStr = c"
    #version 460 core
    layout(location = 0) uniform vec2 pointPos;
    void main() {
        gl_Position = vec4(pointPos, 0.0, 1.0);
    }";

const POINT_FS: &CStr = c"
    #version 460 core
    layout(location = 1) uniform vec2 colorRG;
    layout(location = 2) uniform vec2 colorBStyle;
    layout(location = 3) uniform vec2 pointSize;
    out vec4 FragColor;
    void main() {
        vec2 coord = gl_PointCoord * 2.0 - 1.0;
        float arm = max(2.0 / pointSize.x, 0.125);
        if (colorBStyle.y > 2.5) {
            if (abs(abs(coord.x) - abs(coord.y)) > arm * 1.4142) discard;
        } else if (colorBStyle.y > 1.5) {
            if (length(coord) > 1.0) discard;
        } else if (min(abs(coord.x), abs(coord.y)) > arm) {
            discard;
        }
        FragColor = vec4(colorRG, colorBStyle.x, 1.0);
    }";

pub fn draw_cursor(x: i32, y: i32, w: i32, h: i32, style: u8, color: [u8; 3], size: u8) {
    let program = *POINT_SHADER.get_or_init(|| link_program(POINT_VS, POINT_FS));
    let vao = vao();
    let gl = gl();

    let x_ndc = (x as f32 / w as f32) * 2.0 - 1.0;
    let y_ndc = 1.0 - (y as f32 / h as f32) * 2.0;
    let pos = [x_ndc, y_ndc];
    let [r, g, b] = color.map(|c| c as f32 / 255.0);
    let color_rg = [r, g];
    let point_size = [size as f32, 0.0];
    let color_b_style = [b, style as f32];

    unsafe {
        (gl.use_program)(program);
        (gl.bind_vertex_array)(vao);
        glPointSize(size as f32);
        (gl.uniform_2fv)(0, 1, pos.as_ptr());
        (gl.uniform_2fv)(1, 1, color_rg.as_ptr());
        (gl.uniform_2fv)(2, 1, color_b_style.as_ptr());
        (gl.uniform_2fv)(3, 1, point_size.as_ptr());
        glDrawArrays(POINTS, 0, 1);
    };
}
