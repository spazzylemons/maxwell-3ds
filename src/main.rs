#![feature(allocator_api)]
#![feature(maybe_uninit_write_slice)]
#![feature(new_uninit)]

use std::{cell::RefMut, mem::MaybeUninit, f32::consts::{TAU, PI}, ffi::CString};

use citro3d::render::ClearFlags;
use ctru::{prelude::*, gfx::TopScreen3D, linear::LinearAllocator, services::gspgpu::FramebufferFormat};

include!(concat!(env!("OUT_DIR"), "/maxwell.rs"));

// TODO
static SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shader.shbin"));
// TODO
static VERTICES: &[f32] = MAXWELL_MODEL.vertices;
// TODO
static BODY_INDICES: &[u16] = MAXWELL_MODEL.body;
// TODO
static WHISKERS_INDICES: &[u16] = MAXWELL_MODEL.whiskers;
// TODO
static BODY_TEXTURE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/body.t3x"));
// TODO
static WHISKERS_TEXTURE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/whiskers.t3x"));

struct Material {
    vao: Box<[u16], LinearAllocator>,
    tex: citro3d_sys::C3D_Tex,
}

// copy of GPU_TEXTURE_MAG_FILTER in libctru
#[inline]
#[must_use]
fn mag_filter(v: ctru_sys::GPU_TEXTURE_FILTER_PARAM) -> ctru_sys::GPU_TEXTURE_FILTER_PARAM {
    (v & 0x1) << 1
}

// copy of GPU_TEXTURE_MIN_FILTER in libctru
#[inline]
#[must_use]
fn min_filter(v: ctru_sys::GPU_TEXTURE_FILTER_PARAM) -> ctru_sys::GPU_TEXTURE_FILTER_PARAM {
    (v & 0x1) << 2
}

impl Material {
    fn new(vao: &[u16], texture_data: &[u8]) -> Self {
        // put vao on 
        let vao = move_to_linear(vao);
        // import texture, panicking on failure
        let mut tex = unsafe {
            let mut tex = MaybeUninit::uninit();
            let texture = citro3d_sys::Tex3DS_TextureImport(
                texture_data.as_ptr() as _,
                texture_data.len(),
                tex.as_mut_ptr(),
                std::ptr::null_mut(),
                false,
            );
            if texture.is_null() {
                panic!("failed to import texture");
            }
            // we don't need the texture handle
            citro3d_sys::Tex3DS_TextureFree(texture);
            tex.assume_init()
        };
        // add linear filter to texture
        tex.param |= min_filter(ctru_sys::GPU_LINEAR);
        tex.param |= mag_filter(ctru_sys::GPU_LINEAR);
        // return self
        Self { vao, tex }
    }

    fn draw(&mut self) {
        unsafe {
            citro3d_sys::C3D_TexBind(0, &mut self.tex);
            citro3d_sys::C3D_DrawElements(
                ctru_sys::GPU_TRIANGLES,
                self.vao.len() as _,
                citro3d_sys::C3D_UNSIGNED_SHORT as _,
                self.vao.as_ptr() as _,
            );
        }
    }
}

fn move_to_linear<T>(memory: &[T]) -> Box<[T], LinearAllocator>
where T: Copy,
{
    // create uninit slice
    let mut slice = Box::new_uninit_slice_in(memory.len(), LinearAllocator);
    MaybeUninit::write_slice(&mut *slice, memory);
    // SAFETY: memory is valid because of write_slice call
    unsafe { slice.assume_init() }
}

impl Drop for Material {
    fn drop(&mut self) {
        // SAFETY: clears resources, and Material cannot be copied or cloned so
        // there are no double frees
        unsafe {
            citro3d_sys::C3D_TexDelete(&mut self.tex);
        }
    }
}

fn create_target<'screen>(screen: RefMut<'screen, dyn ctru::gfx::Screen>) -> citro3d::render::Target<'screen> {
    citro3d::render::Target::new(
        240,
        400,
        screen,
        Some(citro3d::render::DepthFormat::Depth24Stencil8),
    ).unwrap()
}

fn get_slider_state() -> f32 {
    // SAFETY: The pointer is valid because we know the address is properly
    // mapped on this hardware. However, we are accessing shared mutable memory.
    // As far as I am aware, there is no avoiding this unsafe access if we want
    // to query the state of the 3D slider.
    unsafe {
        let nasal_demons = std::mem::transmute::<_, *const ctru_sys::osSharedConfig_s>(ctru_sys::OS_SHAREDCFG_VADDR);
        (*nasal_demons).slider_3d
    }
}

fn get_uniform_location(program: &mut citro3d::shader::Program, name: &str) -> i32 {
    let name = CString::new(name).unwrap();
    unsafe {
        ctru_sys::shaderInstanceGetUniformLocation(
            (*program.as_raw()).vertexShader, 
            name.as_ptr(),
        ) as _
    }
}

struct Scene {
    angle: f32,

    body_mat: Material,
    whiskers_mat: Material,

    shader_projection: i32,
    shader_model_view: i32,
}

impl Scene {
    fn render<'screen>(&mut self, instance: &mut citro3d::Instance, target: &mut citro3d::render::Target<'screen>, iod: f32) {
        target.clear(ClearFlags::ALL, 0x808080ff, 0);

        instance.select_render_target(target).unwrap();

        // SAFETY: it's just matrix math
        unsafe {
            let mut projection = MaybeUninit::uninit();
            citro3d_sys::Mtx_PerspStereoTilt(
                projection.as_mut_ptr(),
                PI / 4.0,
                citro3d_sys::C3D_AspectRatioTop as _,
                0.01,
                100.0,
                iod,
                3.0,
                false,
            );
            let projection = projection.assume_init();

            let mut model_view = citro3d_sys::C3D_Mtx {
                r: [
                    citro3d_sys::C3D_FVec { c: [0.0, 0.0, 0.0, 1.0] },
                    citro3d_sys::C3D_FVec { c: [0.0, 0.0, 1.0, 0.0] },
                    citro3d_sys::C3D_FVec { c: [0.0, 1.0, 0.0, 0.0] },
                    citro3d_sys::C3D_FVec { c: [1.0, 0.0, 0.0, 0.0] },
                ],
            };
            citro3d_sys::Mtx_Translate(&mut model_view, 0.0, -10.0, -40.0, true);
            citro3d_sys::Mtx_RotateY(&mut model_view, self.angle, true);

            citro3d_sys::C3D_FVUnifMtx4x4(
                ctru_sys::GPU_VERTEX_SHADER,
                self.shader_projection,
                &projection,
            );

            citro3d_sys::C3D_FVUnifMtx4x4(
                ctru_sys::GPU_VERTEX_SHADER,
                self.shader_model_view,
                &model_view,
            );
        }

        self.body_mat.draw();
        self.whiskers_mat.draw();
    }
}

fn main() {
    let gfx = Gfx::init().unwrap();
    let apt = Apt::init().unwrap();
    let hid = Hid::init().unwrap();

    let top_screen = TopScreen3D::from(&gfx.top_screen);
    let (mut left, mut right) = top_screen.split_mut();

    let mut instance = citro3d::Instance::new().unwrap();

    left.set_framebuffer_format(FramebufferFormat::Rgba8);
    right.set_framebuffer_format(FramebufferFormat::Rgba8);

    let mut left = create_target(left);
    let mut right = create_target(right);

    let shader = citro3d::shader::Library::from_bytes(SHADER).unwrap();
    let vertex_shader = shader.get(0).unwrap();

    let mut program = citro3d::shader::Program::new(vertex_shader).unwrap();

    unsafe {
        citro3d_sys::C3D_BindProgram(program.as_raw());
        let attr_info = citro3d_sys::C3D_GetAttrInfo();
        citro3d_sys::AttrInfo_Init(attr_info);
        citro3d_sys::AttrInfo_AddLoader(attr_info, 0, ctru_sys::GPU_FLOAT, 3); // v0 = position
        citro3d_sys::AttrInfo_AddLoader(attr_info, 1, ctru_sys::GPU_FLOAT, 2); // v1 = uv
    }

    let vertices = move_to_linear(VERTICES);

    let mut scene = Scene {
        angle: 4.0,

        body_mat: Material::new(BODY_INDICES, BODY_TEXTURE),
        whiskers_mat: Material::new(WHISKERS_INDICES, WHISKERS_TEXTURE),

        shader_projection: get_uniform_location(&mut program, "projection"),
        shader_model_view: get_uniform_location(&mut program, "model_view"),
    };

    unsafe {
        let buf_info = citro3d_sys::C3D_GetBufInfo();
        citro3d_sys::BufInfo_Init(buf_info);
        citro3d_sys::BufInfo_Add(buf_info, vertices.as_ptr() as _, (std::mem::size_of::<f32>() * 5) as _, 2, 0x10);

        let env = citro3d_sys::C3D_GetTexEnv(0);
        citro3d_sys::C3D_TexEnvInit(env);
        citro3d_sys::C3D_TexEnvSrc(env, citro3d_sys::C3D_Both, ctru_sys::GPU_TEXTURE0, ctru_sys::GPU_PRIMARY_COLOR, 0);
        citro3d_sys::C3D_TexEnvFunc(env, citro3d_sys::C3D_Both, ctru_sys::GPU_MODULATE);

        citro3d_sys::C3D_CullFace(ctru_sys::GPU_CULL_NONE);
    }

    while apt.main_loop() {
        hid.scan_input();

        let held = hid.keys_held();
        if held.contains(KeyPad::KEY_START) {
            break;
        }

        if held.contains(KeyPad::KEY_DLEFT) {
            scene.angle -= 0.1;
        }

        if held.contains(KeyPad::KEY_DRIGHT) {
            scene.angle += 0.1;
        }

        scene.angle = scene.angle.rem_euclid(TAU);

        let depth = get_slider_state();

        instance.render_frame_with(|instance| {
            scene.render(instance, &mut left, -depth);
            if depth > 0.0 {
                scene.render(instance, &mut right, depth);
            }
        });
    }
}
