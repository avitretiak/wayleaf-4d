use crate::kms::{Monitor, Plane};
use crate::{AppError, Result};
use glow::HasContext;
use libloading::Library;
use std::cell::Cell;
use std::ffi::{c_void, CStr, CString};
use std::fs::OpenOptions;
use std::os::fd::{AsRawFd, OwnedFd};
use std::rc::Rc;

const EGL_NONE: i32 = 0x3038;
const EGL_SUCCESS: i32 = 0x3000;
const EGL_WIDTH: i32 = 0x3057;
const EGL_HEIGHT: i32 = 0x3056;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_DONT_CARE: i32 = -1;
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_OPENGL_ES3_BIT: i32 = 0x0040;
const EGL_RED_SIZE: i32 = 0x3024;
const EGL_GREEN_SIZE: i32 = 0x3023;
const EGL_BLUE_SIZE: i32 = 0x3022;
const EGL_ALPHA_SIZE: i32 = 0x3021;
const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
const EGL_OPENGL_ES_API: u32 = 0x30A0;
const EGL_PLATFORM_GBM_KHR: u32 = 0x31D7;
const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: i32 = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: i32 = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: i32 = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: i32 = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: i32 = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: i32 = 0x3444;
const EGL_EXTENSIONS: i32 = 0x3055;
const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;
const DRM_FORMAT_ABGR16161616: u32 = fourcc(*b"AB48");

const GL_VERTEX_SHADER: u32 = 0x8B31;
const GL_FRAGMENT_SHADER: u32 = 0x8B30;
const VERTEX_SHADER: &str = "#version 300 es\nprecision highp float;\nuniform vec4 dst;\nout vec2 uv;\nconst vec2 p[3] = vec2[3](vec2(0.0,0.0), vec2(2.0,0.0), vec2(0.0,2.0));\nvoid main(){ vec2 q=p[gl_VertexID]; uv=q; gl_Position=vec4(dst.xy+q*(dst.zw-dst.xy),0.0,1.0); }\n";
const FRAGMENT_SHADER: &str = "#version 300 es\nprecision highp float;\nuniform sampler2D image; uniform vec4 src; uniform int rotation; uniform float hdr_peak_pq; uniform float hdr_white_pq; uniform float hdr_white_scale;\nin vec2 uv; out vec4 color;\nconst float PQ_M1=0.1593017578125; const float PQ_M2=78.84375; const float PQ_C1=0.8359375; const float PQ_C2=18.8515625; const float PQ_C3=18.6875;\nfloat pq_lum(float p){float e=pow(max(p,0.0),1.0/PQ_M2); return pow(max(e-PQ_C1,0.0)/(PQ_C2-PQ_C3*e),1.0/PQ_M1);}\nfloat lum_pq(float l){float x=pow(clamp(l,0.0,1.0),PQ_M1); return pow((PQ_C1+PQ_C2*x)/(1.0+PQ_C3*x),PQ_M2);}\nfloat eetf(float p){float target=hdr_white_pq/hdr_peak_pq; float knee=1.5*target-0.5; float e=min(p/hdr_peak_pq,1.0); if(knee<1.0 && e>knee){float t=(e-knee)/(1.0-knee); float t2=t*t; float t3=t2*t; e=(2.0*t3-3.0*t2+1.0)*knee+(t3-2.0*t2+t)*(1.0-knee)+(-2.0*t3+3.0*t2)*target;} return e*hdr_peak_pq;}\nvec3 tone(vec3 pq){vec3 l=vec3(pq_lum(pq.r),pq_lum(pq.g),pq_lum(pq.b)); float m=max(l.r,max(l.g,l.b)); if(m>0.0) l*=pq_lum(eetf(lum_pq(m)))/m; l=mat3(1.660491,-0.124550,-0.018151,-0.587641,1.132900,-0.100579,-0.072850,-0.008349,1.118730)*l; return pow(clamp(l*hdr_white_scale,0.0,1.0),vec3(1.0/2.2));}\nvoid main(){ vec2 q=uv; if(rotation==1) q=vec2(q.y,1.0-q.x); else if(rotation==2) q=vec2(1.0-q.x,1.0-q.y); else if(rotation==3) q=vec2(1.0-q.y,q.x); q.y=1.0-q.y; vec2 tc=src.xy+q*src.zw; vec4 px=texture(image,tc); if(hdr_peak_pq>0.0) px.rgb=tone(px.rgb); color=px; }\n";

pub struct Gpu {
    egl: Egl,
    gl: glow::Context,
    program: Option<glow::NativeProgram>,
    vao: Option<glow::NativeVertexArray>,
    framebuffer: Option<glow::NativeFramebuffer>,
    output: Option<glow::NativeTexture>,
    inputs: Vec<glow::NativeTexture>,
    dst_uniform: Option<glow::UniformLocation>,
    src_uniform: Option<glow::UniformLocation>,
    rotation_uniform: Option<glow::UniformLocation>,
    hdr_peak_uniform: Option<glow::UniformLocation>,
    hdr_white_uniform: Option<glow::UniformLocation>,
    hdr_scale_uniform: Option<glow::UniformLocation>,
    sampler_uniform: Option<glow::UniformLocation>,
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    _thread_bound: std::marker::PhantomData<Rc<()>>,
}

impl Gpu {
    pub fn new(monitor: &Monitor) -> Result<Self> {
        let (width, height) = output_size(monitor.width, monitor.height)?;
        Self::with_size(monitor, width, height)
    }

    fn with_size(monitor: &Monitor, width: usize, height: usize) -> Result<Self> {
        let egl = Egl::new(&monitor.card_path)?;
        let gl = unsafe {
            let egl_proc = egl.get_proc_address;
            let gles = &egl.gles;
            glow::Context::from_loader_function(|name| {
                let name = CString::new(name).expect("GL symbol name contains NUL");
                let proc = egl_proc(name.as_ptr());
                if !proc.is_null() {
                    proc
                } else {
                    gles.get::<*const c_void>(name.as_bytes_with_nul())
                        .map(|symbol| *symbol)
                        .unwrap_or(std::ptr::null())
                }
            })
        };
        let bytes = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| AppError::new("capture output size overflow"))?;
        let mut gpu = Self {
            egl,
            gl,
            program: None,
            vao: None,
            framebuffer: None,
            output: None,
            inputs: Vec::with_capacity(8),
            dst_uniform: None,
            src_uniform: None,
            rotation_uniform: None,
            hdr_peak_uniform: None,
            hdr_white_uniform: None,
            hdr_scale_uniform: None,
            sampler_uniform: None,
            width,
            height,
            rgba: vec![0; bytes],
            _thread_bound: std::marker::PhantomData,
        };
        gpu.initialize_gl()?;
        Ok(gpu)
    }

    fn initialize_gl(&mut self) -> Result<()> {
        unsafe {
            let vertex = compile_shader(&self.gl, GL_VERTEX_SHADER, VERTEX_SHADER)?;
            let fragment = match compile_shader(&self.gl, GL_FRAGMENT_SHADER, FRAGMENT_SHADER) {
                Ok(shader) => shader,
                Err(error) => {
                    self.gl.delete_shader(vertex);
                    return Err(error);
                }
            };
            let program = self
                .gl
                .create_program()
                .map_err(|e| AppError::new(format!("creating capture GL program: {e}")))?;
            self.program = Some(program);
            self.gl.attach_shader(program, vertex);
            self.gl.attach_shader(program, fragment);
            self.gl.link_program(program);
            self.gl.detach_shader(program, vertex);
            self.gl.detach_shader(program, fragment);
            self.gl.delete_shader(vertex);
            self.gl.delete_shader(fragment);
            if !self.gl.get_program_link_status(program) {
                return Err(AppError::new(format!(
                    "linking capture GL program: {}",
                    self.gl.get_program_info_log(program)
                )));
            }
            self.dst_uniform = self.gl.get_uniform_location(program, "dst");
            self.src_uniform = self.gl.get_uniform_location(program, "src");
            self.rotation_uniform = self.gl.get_uniform_location(program, "rotation");
            self.hdr_peak_uniform = self.gl.get_uniform_location(program, "hdr_peak_pq");
            self.hdr_white_uniform = self.gl.get_uniform_location(program, "hdr_white_pq");
            self.hdr_scale_uniform = self.gl.get_uniform_location(program, "hdr_white_scale");
            self.sampler_uniform = self.gl.get_uniform_location(program, "image");
            if self.dst_uniform.is_none()
                || self.src_uniform.is_none()
                || self.rotation_uniform.is_none()
                || self.hdr_peak_uniform.is_none()
                || self.hdr_white_uniform.is_none()
                || self.hdr_scale_uniform.is_none()
                || self.sampler_uniform.is_none()
            {
                return Err(AppError::new("capture shader uniforms are unavailable"));
            }

            let vao = self
                .gl
                .create_vertex_array()
                .map_err(|e| AppError::new(format!("creating capture VAO: {e}")))?;
            self.vao = Some(vao);
            self.gl.bind_vertex_array(Some(vao));

            for _ in 0..8 {
                let texture = self
                    .gl
                    .create_texture()
                    .map_err(|e| AppError::new(format!("creating DMA-BUF texture: {e}")))?;
                self.inputs.push(texture);
                self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
            }

            let output = self
                .gl
                .create_texture()
                .map_err(|e| AppError::new(format!("creating capture output texture: {e}")))?;
            self.output = Some(output);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(output));
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                self.width as i32,
                self.height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            let framebuffer = self
                .gl
                .create_framebuffer()
                .map_err(|e| AppError::new(format!("creating capture framebuffer: {e}")))?;
            self.framebuffer = Some(framebuffer);
            self.gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            self.gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(output),
                0,
            );
            if self.gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err(AppError::new("capture framebuffer is incomplete"));
            }
            self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            self.gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.bind_texture(glow::TEXTURE_2D, None);
            self.gl.bind_vertex_array(None);
        }
        Ok(())
    }

    pub fn render(&mut self, monitor: &Monitor, planes: &[Plane]) -> Result<()> {
        self.egl.make_current()?;
        let mut selected: [Option<&Plane>; 8] = [None; 8];
        let mut primary: Option<&Plane> = None;
        // Cursor planes are intentionally omitted; compose only primary and overlay content.
        for plane in planes {
            if plane.connector_id != monitor.connector_id || plane.plane_type != 0 {
                continue;
            }
            let area = i64::from(plane.src_w).max(0) * i64::from(plane.src_h).max(0);
            let replace = primary.is_none_or(|current| {
                area > i64::from(current.src_w).max(0) * i64::from(current.src_h).max(0)
            });
            if replace {
                primary = Some(plane);
            }
        }
        let primary = primary.ok_or_else(|| {
            AppError::new(format!(
                "KMS returned no primary plane for monitor {}",
                monitor.name
            ))
        })?;
        selected[0] = Some(primary);
        let mut count = 1;
        for plane in planes {
            if plane.connector_id == monitor.connector_id && plane.plane_type == 2 {
                if count == selected.len() {
                    return Err(AppError::new("KMS returned too many capture planes"));
                }
                selected[count] = Some(plane);
                count += 1;
            }
        }
        for index in 1..count {
            let plane = selected[index].unwrap();
            let mut j = index;
            while j > 0 && selected[j - 1].unwrap().zpos > plane.zpos {
                selected[j] = selected[j - 1];
                j -= 1;
            }
            selected[j] = Some(plane);
        }

        unsafe {
            let fbo = self
                .framebuffer
                .ok_or_else(|| AppError::new("capture framebuffer is not initialized"))?;
            let program = self
                .program
                .ok_or_else(|| AppError::new("capture shader is not initialized"))?;
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            self.gl
                .viewport(0, 0, self.width as i32, self.height as i32);
            self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
            self.gl.use_program(Some(program));
            self.gl.bind_vertex_array(self.vao);
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.uniform_1_i32(self.sampler_uniform.as_ref(), 0);
            for (slot, plane) in selected[..count].iter().enumerate() {
                let plane = plane.unwrap();
                validate_plane(plane)?;
                let image = self.egl.import_image(plane)?;
                let texture = self.inputs[slot];
                self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                self.egl.bind_image(texture, image.image);
                let gl_error = self.gl.get_error();
                if gl_error != glow::NO_ERROR {
                    return Err(AppError::new(format!(
                        "binding KMS DMA-BUF to GL failed (error 0x{gl_error:04x})"
                    )));
                }

                let sx = self.width as f32 / monitor.width as f32;
                let sy = self.height as f32 / monitor.height as f32;
                let left = plane.dst_x as f32 * sx;
                let right = (i64::from(plane.dst_x) + i64::from(plane.dst_w)) as f32 * sx;
                let bottom = self.height as f32
                    - (i64::from(plane.dst_y) + i64::from(plane.dst_h)) as f32 * sy;
                let top = self.height as f32 - plane.dst_y as f32 * sy;
                self.gl.uniform_4_f32(
                    self.dst_uniform.as_ref(),
                    left / self.width as f32 * 2.0 - 1.0,
                    bottom / self.height as f32 * 2.0 - 1.0,
                    right / self.width as f32 * 2.0 - 1.0,
                    top / self.height as f32 * 2.0 - 1.0,
                );
                self.gl.uniform_4_f32(
                    self.src_uniform.as_ref(),
                    plane.src_x as f32 / plane.width as f32,
                    plane.src_y as f32 / plane.height as f32,
                    plane.src_w as f32 / plane.width as f32,
                    plane.src_h as f32 / plane.height as f32,
                );
                self.gl
                    .uniform_1_i32(self.rotation_uniform.as_ref(), plane.rotation as i32);
                let (peak_pq, white_pq, white_scale) = hdr_uniforms(plane)?;
                self.gl
                    .uniform_1_f32(self.hdr_peak_uniform.as_ref(), peak_pq);
                self.gl
                    .uniform_1_f32(self.hdr_white_uniform.as_ref(), white_pq);
                self.gl
                    .uniform_1_f32(self.hdr_scale_uniform.as_ref(), white_scale);
                self.gl.draw_arrays(glow::TRIANGLES, 0, 3);
                let gl_error = self.gl.get_error();
                if gl_error != glow::NO_ERROR {
                    return Err(AppError::new(format!(
                        "rendering KMS plane failed (GL error 0x{gl_error:04x})"
                    )));
                }
                drop(image);
            }
            self.gl.bind_vertex_array(None);
            self.gl.use_program(None);
            self.gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fbo));
            self.gl.read_pixels(
                0,
                0,
                self.width as i32,
                self.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut self.rgba)),
            );
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            let gl_error = self.gl.get_error();
            if gl_error != glow::NO_ERROR {
                return Err(AppError::new(format!(
                    "reading capture pixels failed (GL error 0x{gl_error:04x})"
                )));
            }
            flip_rows(&mut self.rgba, self.width * 4, self.height);
        }
        Ok(())
    }

    pub fn synthetic_render_test(monitor: &Monitor) -> Result<()> {
        let mut gpu = Self::with_size(monitor, 2, 2)?;
        let texture = unsafe {
            gpu.gl
                .create_texture()
                .map_err(|e| AppError::new(format!("creating synthetic test texture: {e}")))?
        };
        let normalized_16 = gpu
            .gl
            .supported_extensions()
            .contains("GL_EXT_texture_norm16");
        let mut image = [0u8; 32];
        // Model imported KMS images, whose top row is at texture v=0.
        let levels = [pq_encode(203.0), pq_encode(100.0), pq_encode(10.0), 0.0];
        for (pixel, level) in image.chunks_exact_mut(8).zip(levels) {
            let value = if normalized_16 {
                (level.clamp(0.0, 1.0) * 65535.0).round() as u16
            } else {
                f32_to_f16(level)
            };
            for channel in 0..3 {
                pixel[channel * 2..channel * 2 + 2].copy_from_slice(&value.to_ne_bytes());
            }
            let alpha = if normalized_16 {
                u16::MAX
            } else {
                f32_to_f16(1.0)
            };
            pixel[6..8].copy_from_slice(&alpha.to_ne_bytes());
        }
        let mut baseline = [0u8; 4];
        let mut orientations_match = true;
        unsafe {
            gpu.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gpu.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            gpu.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            gpu.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                (if normalized_16 {
                    glow::RGBA16
                } else {
                    glow::RGBA16F
                }) as i32,
                2,
                2,
                0,
                glow::RGBA,
                if normalized_16 {
                    glow::UNSIGNED_SHORT
                } else {
                    glow::HALF_FLOAT
                },
                glow::PixelUnpackData::Slice(Some(&image)),
            );
            let fbo = gpu.framebuffer.unwrap();
            gpu.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gpu.gl.viewport(0, 0, 2, 2);
            gpu.gl.use_program(gpu.program);
            gpu.gl.bind_vertex_array(gpu.vao);
            gpu.gl.active_texture(glow::TEXTURE0);
            gpu.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gpu.gl.uniform_1_i32(gpu.sampler_uniform.as_ref(), 0);
            gpu.gl
                .uniform_4_f32(gpu.dst_uniform.as_ref(), -1.0, -1.0, 1.0, 1.0);
            gpu.gl
                .uniform_4_f32(gpu.src_uniform.as_ref(), 0.0, 0.0, 1.0, 1.0);
            gpu.gl
                .uniform_1_f32(gpu.hdr_peak_uniform.as_ref(), pq_encode(446.0));
            gpu.gl
                .uniform_1_f32(gpu.hdr_white_uniform.as_ref(), pq_encode(203.0));
            gpu.gl
                .uniform_1_f32(gpu.hdr_scale_uniform.as_ref(), 10000.0 / 203.0);
            for rotation in 0..4 {
                gpu.gl.clear_color(0.0, 0.0, 0.0, 1.0);
                gpu.gl.clear(glow::COLOR_BUFFER_BIT);
                gpu.gl
                    .uniform_1_i32(gpu.rotation_uniform.as_ref(), rotation);
                gpu.gl.draw_arrays(glow::TRIANGLES, 0, 3);
                gpu.gl.read_pixels(
                    0,
                    0,
                    2,
                    2,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut gpu.rgba)),
                );
                flip_rows(&mut gpu.rgba, 8, 2);
                let current = [gpu.rgba[0], gpu.rgba[4], gpu.rgba[8], gpu.rgba[12]];
                if rotation == 0 {
                    baseline = current;
                } else {
                    for y in 0..2 {
                        for x in 0..2 {
                            let u = (x as f32 + 0.5) / 2.0;
                            let v = 1.0 - (y as f32 + 0.5) / 2.0;
                            let (source_u, source_v) = rotate_uv(u, v, rotation as u32);
                            let source_x = (source_u * 2.0).floor().min(1.0) as usize;
                            let source_y = ((1.0 - source_v) * 2.0).floor().min(1.0) as usize;
                            orientations_match &=
                                current[y * 2 + x].abs_diff(baseline[source_y * 2 + source_x]) <= 2;
                        }
                    }
                }
            }
            gpu.gl.bind_vertex_array(None);
            gpu.gl.use_program(None);
            gpu.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gpu.gl.delete_texture(texture);
        }
        if !(orientations_match
            && baseline[0] > baseline[1]
            && baseline[1] > baseline[2]
            && baseline[2] > baseline[3])
        {
            return Err(AppError::new(
                "synthetic HDR capture orientation/color check failed",
            ));
        }
        if baseline[0] <= 230 || baseline[1] <= 100 || baseline[2] == 0 {
            return Err(AppError::new("synthetic HDR tone-map output out of range"));
        }
        if gpu.rgba.chunks_exact(4).any(|pixel| {
            pixel[0].abs_diff(pixel[1]) > 2 || pixel[1].abs_diff(pixel[2]) > 2 || pixel[3] < 250
        }) {
            return Err(AppError::new("synthetic HDR channel/alpha check failed"));
        }
        Ok(())
    }

    pub fn frame(&self) -> (&[u8], usize, usize, usize) {
        (&self.rgba, self.width, self.height, self.width * 4)
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            let _ = self.egl.make_current();
            for texture in self.inputs.drain(..) {
                self.gl.delete_texture(texture);
            }
            if let Some(texture) = self.output.take() {
                self.gl.delete_texture(texture);
            }
            if let Some(framebuffer) = self.framebuffer.take() {
                self.gl.delete_framebuffer(framebuffer);
            }
            if let Some(vao) = self.vao.take() {
                self.gl.delete_vertex_array(vao);
            }
            if let Some(program) = self.program.take() {
                self.gl.delete_program(program);
            }
        }
    }
}

fn validate_plane(plane: &Plane) -> Result<()> {
    if plane.width == 0
        || plane.height == 0
        || plane.width > i32::MAX as u32
        || plane.height > i32::MAX as u32
        || plane.buffers.is_empty()
        || plane.buffers.len() > 4
    {
        return Err(AppError::new(
            "KMS plane has invalid framebuffer dimensions or DMA-BUF planes",
        ));
    }
    if !is_supported_rgb(plane.pixel_format) {
        return Err(AppError::new(format!(
            "unsupported KMS RGB format 0x{:08x}",
            plane.pixel_format
        )));
    }
    if plane.rotation > 3 {
        return Err(AppError::new(format!(
            "unsupported KMS plane rotation {}",
            plane.rotation
        )));
    }
    let (x, y, w, h) = (plane.src_x, plane.src_y, plane.src_w, plane.src_h);
    if x < 0
        || y < 0
        || w <= 0
        || h <= 0
        || i64::from(x) + i64::from(w) > i64::from(plane.width)
        || i64::from(y) + i64::from(h) > i64::from(plane.height)
    {
        return Err(AppError::new(
            "KMS plane source crop is outside its framebuffer",
        ));
    }
    if plane.dst_w <= 0 || plane.dst_h <= 0 {
        return Err(AppError::new(
            "KMS plane has an empty destination rectangle",
        ));
    }
    for buffer in &plane.buffers {
        if buffer.pitch == 0 || buffer.pitch > i32::MAX as u32 || buffer.offset > i32::MAX as u32 {
            return Err(AppError::new(
                "KMS plane has an invalid DMA-BUF pitch or offset",
            ));
        }
    }
    Ok(())
}

fn is_supported_rgb(format: u32) -> bool {
    matches!(
        format,
        DRM_FORMAT_ABGR16161616
            | 0x3432_5258 // XR24
            | 0x3432_5241 // AR24
            | 0x3432_4258 // XB24
            | 0x3432_4241 // AB24
            | 0x3033_5258 // XR30
            | 0x3033_5241 // AR30
            | 0x3033_4258 // XB30
            | 0x3033_4241 // AB30
    )
}

fn hdr_uniforms(plane: &Plane) -> Result<(f32, f32, f32)> {
    let Some(hdr) = plane.hdr.as_ref() else {
        return Ok((0.0, 0.0, 1.0));
    };
    if hdr.metadata_type != 0 || hdr.infoframe_metadata_type != 0 {
        return Err(AppError::new("unsupported KMS HDR metadata type"));
    }
    match hdr.eotf {
        0 => Ok((0.0, 0.0, 1.0)),
        2 => {
            let white = 203.0f32;
            let reported_peak = if hdr.max_cll > 0 {
                hdr.max_cll
            } else {
                hdr.max_luminance
            };
            let peak = f32::from(if reported_peak > 0 {
                reported_peak
            } else {
                1000
            })
            .max(white)
            .min(10000.0);
            Ok((pq_encode(peak), pq_encode(white), 10000.0 / white))
        }
        eotf => Err(AppError::new(format!(
            "unsupported KMS HDR transfer function {eotf}"
        ))),
    }
}

fn pq_encode(luminance: f32) -> f32 {
    let l = (luminance.clamp(0.0, 10000.0) / 10000.0).powf(0.159_301_76);
    ((0.8359375 + 18.851_563 * l) / (1.0 + 18.6875 * l)).powf(78.84375)
}
fn f32_to_f16(value: f32) -> u16 {
    if value == 0.0 {
        return 0;
    }
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = (((bits >> 23) & 0xff) as i32 - 112) as u16;
    let mantissa = ((bits & 0x7f_ffff) + 0x1000) >> 13;
    sign | (exponent << 10) | mantissa as u16
}

fn pq_decode(pq: f32) -> f32 {
    let e = pq.max(0.0).powf(1.0 / 78.84375);
    ((e - 0.8359375).max(0.0) / (18.851_563 - 18.6875 * e)).powf(1.0 / 0.159_301_76)
}

fn output_size(width: u32, height: u32) -> Result<(usize, usize)> {
    if width == 0 || height == 0 {
        return Err(AppError::new("selected monitor has an empty mode"));
    }
    let out_width = width.min(320) as usize;
    let scaled_height = out_width as f64 * height as f64 / width as f64;
    if !scaled_height.is_finite() || scaled_height > 8192.0 {
        return Err(AppError::new(
            "capture monitor aspect ratio exceeds output limits",
        ));
    }
    let out_height = (scaled_height.round() as usize).max(1);
    out_width
        .checked_mul(out_height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| AppError::new("capture output size overflow"))?;
    Ok((out_width, out_height))
}

fn flip_rows(data: &mut [u8], stride: usize, height: usize) {
    for y in 0..height / 2 {
        let opposite = height - 1 - y;
        let (before, after) = data.split_at_mut(opposite * stride);
        before[y * stride..(y + 1) * stride].swap_with_slice(&mut after[..stride]);
    }
}

const fn fourcc(bytes: [u8; 4]) -> u32 {
    u32::from_le_bytes(bytes)
}

fn compile_shader(gl: &glow::Context, kind: u32, source: &str) -> Result<glow::NativeShader> {
    unsafe {
        let shader = gl
            .create_shader(kind)
            .map_err(|e| AppError::new(format!("creating capture shader: {e}")))?;
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let error = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            return Err(AppError::new(format!("compiling capture shader: {error}")));
        }
        Ok(shader)
    }
}

fn rotate_uv(u: f32, v: f32, rotation: u32) -> (f32, f32) {
    match rotation {
        1 => (v, 1.0 - u),
        2 => (1.0 - u, 1.0 - v),
        3 => (1.0 - v, u),
        _ => (u, v),
    }
}

fn self_test_inner() -> Result<()> {
    let (w, h) = output_size(3840, 2160)?;
    if (w, h) != (320, 180) || output_size(1080, 1920)? != (320, 569) {
        return Err(AppError::new("capture output aspect-ratio check failed"));
    }
    if output_size(1, u32::MAX).is_ok() {
        return Err(AppError::new("capture output height limit check failed"));
    }
    if rotate_uv(0.25, 0.75, 1) != (0.75, 0.75)
        || rotate_uv(0.25, 0.75, 2) != (0.75, 0.25)
        || rotate_uv(0.25, 0.75, 3) != (0.25, 0.25)
    {
        return Err(AppError::new("capture rotation transform check failed"));
    }
    if (pq_decode(pq_encode(203.0)) - 203.0 / 10000.0).abs() > 0.0001 {
        return Err(AppError::new("PQ encode/decode check failed"));
    }
    let mut rows = [1, 2, 3, 4, 5, 6, 7, 8];
    flip_rows(&mut rows, 4, 2);
    if rows != [5, 6, 7, 8, 1, 2, 3, 4] {
        return Err(AppError::new("capture top-down row order check failed"));
    }
    if !is_supported_rgb(DRM_FORMAT_ABGR16161616) {
        return Err(AppError::new("16-bit AB48 capture format is not enabled"));
    }
    Ok(())
}

pub fn self_test() -> Result<()> {
    self_test_inner()
}

struct Egl {
    _library: Library,
    gles: Library,
    get_proc_address: GetProcAddress,
    get_error: GetError,
    initialize: Initialize,
    bind_api: BindApi,
    choose_config: ChooseConfig,
    create_context: CreateContext,
    create_pbuffer: CreatePbuffer,
    make_current_fn: MakeCurrent,
    destroy_context: DestroyContext,
    destroy_surface: DestroySurface,
    terminate: Terminate,
    get_platform_display: GetPlatformDisplay,
    create_image: CreateImage,
    destroy_image: DestroyImage,
    query_string: QueryString,
    image_target_texture: ImageTargetTexture,
    display: *mut c_void,
    context: *mut c_void,
    surface: *mut c_void,
    egl_extensions: String,
    no_modifiers_fallback: Cell<bool>,
    _gbm_library: Library,
    gbm_device: *mut c_void,
    gbm_device_destroy: GbmDeviceDestroy,
    _render_fd: OwnedFd,
    _thread_bound: std::marker::PhantomData<Rc<()>>,
}

type GetProcAddress = unsafe extern "system" fn(*const i8) -> *const c_void;
type GetError = unsafe extern "system" fn() -> i32;
type Initialize = unsafe extern "system" fn(*mut c_void, *mut i32, *mut i32) -> u32;
type BindApi = unsafe extern "system" fn(u32) -> u32;
type ChooseConfig =
    unsafe extern "system" fn(*mut c_void, *const i32, *mut *mut c_void, i32, *mut i32) -> u32;
type CreateContext =
    unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const i32) -> *mut c_void;
type CreatePbuffer = unsafe extern "system" fn(*mut c_void, *mut c_void, *const i32) -> *mut c_void;
type MakeCurrent =
    unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> u32;
type DestroyContext = unsafe extern "system" fn(*mut c_void, *mut c_void) -> u32;
type DestroySurface = unsafe extern "system" fn(*mut c_void, *mut c_void) -> u32;
type Terminate = unsafe extern "system" fn(*mut c_void) -> u32;
type GetPlatformDisplay = unsafe extern "system" fn(u32, *mut c_void, *const i32) -> *mut c_void;
type QueryString = unsafe extern "system" fn(*mut c_void, i32) -> *const i8;
type CreateImage = unsafe extern "system" fn(
    *mut c_void,
    *mut c_void,
    u32,
    *mut c_void,
    *const i32,
) -> *mut c_void;
type DestroyImage = unsafe extern "system" fn(*mut c_void, *mut c_void) -> u32;
type ImageTargetTexture = unsafe extern "system" fn(u32, *mut c_void);
type GbmCreateDevice = unsafe extern "C" fn(i32) -> *mut c_void;
type GbmDeviceDestroy = unsafe extern "C" fn(*mut c_void);

impl Egl {
    fn new(card_path: &str) -> Result<Self> {
        let render_node = crate::kms::render_node_for_card(card_path)?;
        let library = unsafe { Library::new("libEGL.so.1") }
            .map_err(|e| AppError::new(format!("loading EGL: {e}")))?;
        let gles = unsafe { Library::new("libGLESv2.so.2") }
            .map_err(|e| AppError::new(format!("loading GLES: {e}")))?;
        let gbm_library = unsafe { Library::new("libgbm.so.1") }
            .map_err(|e| AppError::new(format!("loading GBM: {e}")))?;
        let render_fd: OwnedFd = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&render_node)
            .map_err(|e| {
                AppError::new(format!(
                    "opening GPU render node {}: {e}",
                    render_node.display()
                ))
            })?
            .into();
        let get_proc_address: GetProcAddress = unsafe { symbol(&library, b"eglGetProcAddress\0")? };
        let gbm_create_device: GbmCreateDevice =
            unsafe { symbol(&gbm_library, b"gbm_create_device\0")? };
        let gbm_device_destroy: GbmDeviceDestroy =
            unsafe { symbol(&gbm_library, b"gbm_device_destroy\0")? };
        let mut egl = Self {
            get_error: unsafe { symbol(&library, b"eglGetError\0")? },
            initialize: unsafe { symbol(&library, b"eglInitialize\0")? },
            bind_api: unsafe { symbol(&library, b"eglBindAPI\0")? },
            choose_config: unsafe { symbol(&library, b"eglChooseConfig\0")? },
            create_context: unsafe { symbol(&library, b"eglCreateContext\0")? },
            create_pbuffer: unsafe { symbol(&library, b"eglCreatePbufferSurface\0")? },
            make_current_fn: unsafe { symbol(&library, b"eglMakeCurrent\0")? },
            destroy_context: unsafe { symbol(&library, b"eglDestroyContext\0")? },
            destroy_surface: unsafe { symbol(&library, b"eglDestroySurface\0")? },
            terminate: unsafe { symbol(&library, b"eglTerminate\0")? },
            query_string: unsafe { symbol(&library, b"eglQueryString\0")? },
            get_platform_display: unsafe {
                extension(get_proc_address, b"eglGetPlatformDisplayEXT\0")?
            },
            create_image: unsafe { extension(get_proc_address, b"eglCreateImageKHR\0")? },
            destroy_image: unsafe { extension(get_proc_address, b"eglDestroyImageKHR\0")? },
            image_target_texture: unsafe {
                extension(get_proc_address, b"glEGLImageTargetTexture2DOES\0")?
            },
            _library: library,
            gles,
            get_proc_address,
            display: std::ptr::null_mut(),
            context: std::ptr::null_mut(),
            surface: std::ptr::null_mut(),
            egl_extensions: String::new(),
            no_modifiers_fallback: Cell::new(false),
            _gbm_library: gbm_library,
            gbm_device: std::ptr::null_mut(),
            gbm_device_destroy,
            _render_fd: render_fd,
            _thread_bound: std::marker::PhantomData,
        };
        egl.gbm_device = unsafe { gbm_create_device(egl._render_fd.as_raw_fd()) };
        if egl.gbm_device.is_null() {
            return Err(AppError::new(
                "creating GBM device for selected monitor GPU failed",
            ));
        }
        egl.display = unsafe {
            (egl.get_platform_display)(EGL_PLATFORM_GBM_KHR, egl.gbm_device, std::ptr::null())
        };
        if egl.display.is_null() {
            return Err(egl.error("creating GBM EGL display"));
        }
        let mut major = 0;
        let mut minor = 0;
        if unsafe { (egl.initialize)(egl.display, &mut major, &mut minor) } == 0 {
            return Err(egl.error("initializing GBM EGL display"));
        }
        let extensions = unsafe { (egl.query_string)(egl.display, EGL_EXTENSIONS) };
        if extensions.is_null() {
            return Err(egl.error("querying EGL display extensions"));
        }
        egl.egl_extensions = unsafe { CStr::from_ptr(extensions) }
            .to_string_lossy()
            .into_owned();
        if !egl
            .egl_extensions
            .split_whitespace()
            .any(|ext| ext == "EGL_EXT_image_dma_buf_import")
        {
            return Err(AppError::new(
                "EGL GPU does not support DMA-BUF image import",
            ));
        }
        unsafe {
            if (egl.bind_api)(EGL_OPENGL_ES_API) == 0 {
                return Err(egl.error("binding OpenGL ES API"));
            }
            let config_attributes = [
                EGL_SURFACE_TYPE,
                EGL_DONT_CARE,
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_ES3_BIT,
                EGL_RED_SIZE,
                8,
                EGL_GREEN_SIZE,
                8,
                EGL_BLUE_SIZE,
                8,
                EGL_ALPHA_SIZE,
                8,
                EGL_NONE,
            ];
            let mut config = std::ptr::null_mut();
            let mut count = 0;
            if (egl.choose_config)(
                egl.display,
                config_attributes.as_ptr(),
                &mut config,
                1,
                &mut count,
            ) == 0
                || count != 1
            {
                return Err(egl.error("choosing an EGL OpenGL ES 3 config"));
            }
            let context_attributes = [EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE];
            egl.context = (egl.create_context)(
                egl.display,
                config,
                std::ptr::null_mut(),
                context_attributes.as_ptr(),
            );
            if egl.context.is_null() {
                return Err(egl.error("creating EGL OpenGL ES 3 context"));
            }
            if (egl.make_current_fn)(
                egl.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                egl.context,
            ) == 0
            {
                let surface_attributes = [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE];
                egl.surface =
                    (egl.create_pbuffer)(egl.display, config, surface_attributes.as_ptr());
                if egl.surface.is_null() {
                    return Err(egl.error("creating EGL pbuffer surface"));
                }
                egl.make_current()?;
            }
        }
        Ok(egl)
    }

    fn make_current(&self) -> Result<()> {
        if unsafe { (self.make_current_fn)(self.display, self.surface, self.surface, self.context) }
            == 0
        {
            Err(self.error("making EGL context current"))
        } else {
            Ok(())
        }
    }

    fn error(&self, operation: &str) -> AppError {
        AppError::new(format!("{operation} failed (EGL error 0x{:04x})", unsafe {
            (self.get_error)()
        }))
    }

    fn import_image<'a>(&'a self, plane: &Plane) -> Result<EglImage<'a>> {
        let modifier = plane.modifier;
        let has_modifier = modifier != DRM_FORMAT_MOD_INVALID;
        let use_modifiers = has_modifier
            && !self.no_modifiers_fallback.get()
            && self
                .egl_extensions
                .split_whitespace()
                .any(|ext| ext == "EGL_EXT_image_dma_buf_import_modifiers");
        let make_attributes = |use_modifiers: bool| {
            let mut attributes = [EGL_NONE; 64];
            let mut len = 0;
            let mut push = |key: i32, value: i32| {
                attributes[len] = key;
                attributes[len + 1] = value;
                len += 2;
            };
            push(EGL_WIDTH, plane.width as i32);
            push(EGL_HEIGHT, plane.height as i32);
            push(EGL_LINUX_DRM_FOURCC_EXT, plane.pixel_format as i32);
            for (index, buffer) in plane.buffers.iter().enumerate() {
                let n = index as i32;
                push(EGL_DMA_BUF_PLANE0_FD_EXT + n * 3, buffer.fd.as_raw_fd());
                push(EGL_DMA_BUF_PLANE0_OFFSET_EXT + n * 3, buffer.offset as i32);
                push(EGL_DMA_BUF_PLANE0_PITCH_EXT + n * 3, buffer.pitch as i32);
                if use_modifiers {
                    push(
                        EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT + n * 2,
                        modifier as u32 as i32,
                    );
                    push(
                        EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT + n * 2,
                        (modifier >> 32) as u32 as i32,
                    );
                }
            }
            attributes[len] = EGL_NONE;
            attributes
        };
        let mut attributes = make_attributes(use_modifiers);
        unsafe { while (self.get_error)() != EGL_SUCCESS {} }
        let mut image = unsafe {
            (self.create_image)(
                self.display,
                std::ptr::null_mut(),
                EGL_LINUX_DMA_BUF_EXT,
                std::ptr::null_mut(),
                attributes.as_ptr(),
            )
        };
        let mut error = unsafe { (self.get_error)() };
        if (image.is_null() || error != EGL_SUCCESS) && use_modifiers {
            if !image.is_null() {
                unsafe { (self.destroy_image)(self.display, image) };
            }
            self.no_modifiers_fallback.set(true);
            attributes = make_attributes(false);
            unsafe { while (self.get_error)() != EGL_SUCCESS {} }
            image = unsafe {
                (self.create_image)(
                    self.display,
                    std::ptr::null_mut(),
                    EGL_LINUX_DMA_BUF_EXT,
                    std::ptr::null_mut(),
                    attributes.as_ptr(),
                )
            };
            error = unsafe { (self.get_error)() };
        }
        if image.is_null() || error != EGL_SUCCESS {
            if !image.is_null() {
                unsafe { (self.destroy_image)(self.display, image) };
            }
            return Err(AppError::new(format!(
                "importing KMS DMA-BUF (format 0x{:08x}, modifier 0x{:016x}) failed (EGL error 0x{error:04x})",
                plane.pixel_format, plane.modifier
            )));
        }
        Ok(EglImage { egl: self, image })
    }

    unsafe fn bind_image(&self, _texture: glow::NativeTexture, image: *mut c_void) {
        (self.image_target_texture)(glow::TEXTURE_2D, image);
    }
}

impl Drop for Egl {
    fn drop(&mut self) {
        unsafe {
            if !self.display.is_null() {
                if !self.context.is_null() {
                    (self.make_current_fn)(
                        self.display,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                    (self.destroy_context)(self.display, self.context);
                }
                if !self.surface.is_null() {
                    (self.destroy_surface)(self.display, self.surface);
                }
                (self.terminate)(self.display);
            }
            if !self.gbm_device.is_null() {
                (self.gbm_device_destroy)(self.gbm_device);
            }
        }
    }
}

struct EglImage<'a> {
    egl: &'a Egl,
    image: *mut c_void,
}

impl Drop for EglImage<'_> {
    fn drop(&mut self) {
        unsafe { (self.egl.destroy_image)(self.egl.display, self.image) };
    }
}

unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T> {
    library
        .get::<T>(name)
        .map(|symbol| *symbol)
        .map_err(|e| AppError::new(format!("loading native graphics symbol: {e}")))
}

unsafe fn extension<T: Copy>(get_proc: GetProcAddress, name: &[u8]) -> Result<T> {
    let pointer = get_proc(name.as_ptr().cast());
    if pointer.is_null() {
        return Err(AppError::new(format!(
            "required EGL extension function {} is unavailable",
            CStr::from_bytes_with_nul(name)
                .map(|s| s.to_string_lossy())
                .unwrap_or_default()
        )));
    }
    Ok(std::mem::transmute_copy(&pointer))
}

pub fn synthetic_render_test(monitor: &Monitor) -> Result<()> {
    Gpu::synthetic_render_test(monitor)
}
