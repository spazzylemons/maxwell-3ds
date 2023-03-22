#![feature(allocator_api)]
#![feature(maybe_uninit_write_slice)]
#![feature(new_uninit)]

use std::{cell::RefMut, mem::MaybeUninit, f32::consts::{TAU, PI}, ffi::CString, io::Cursor};

use citro3d::render::ClearFlags;
use ctru::{prelude::*, gfx::TopScreen3D, linear::LinearAllocator, services::{gspgpu::FramebufferFormat, ndsp::{Ndsp, OutputMode, InterpolationType, AudioFormat, wave::WaveInfo}}};
use symphonia::core::{io::{MediaSourceStream, MediaSourceStreamOptions}, codecs::{CodecRegistry, CODEC_TYPE_NULL, DecoderOptions}, probe::Hint, meta::MetadataOptions, formats::FormatOptions, audio::SampleBuffer};

include!(concat!(env!("OUT_DIR"), "/maxwell.rs"));

static SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shader.shbin"));
static BODY_TEXTURE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/body.t3x"));
static WHISKERS_TEXTURE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/whiskers.t3x"));

static VERTICES: &[f32] = MAXWELL_MODEL.vertices;
static BODY_INDICES: &[u16] = MAXWELL_MODEL.body;
static WHISKERS_INDICES: &[u16] = MAXWELL_MODEL.whiskers;

static MUSIC_OGG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maxwell.ogg"));

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

// TODO can this be done streaming until it completes?
fn decode_audio() -> Box<[u8], LinearAllocator> {
    let mut result = Vec::<u8, LinearAllocator>::new_in(LinearAllocator);

    let src = Cursor::new(MUSIC_OGG);
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("ogg");

    let meta_ops = MetadataOptions::default();
    let fmt_opts = FormatOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_ops)
        .unwrap();

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .unwrap();

    let dec_opts = DecoderOptions::default();

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .unwrap();

    let mut sample_buf = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(_) => break,
        };

        let audio_buf = decoder.decode(&packet).unwrap();

        if sample_buf.is_none() {
            let spec = *audio_buf.spec();
            let duration = audio_buf.capacity() as u64;
            sample_buf = Some(SampleBuffer::<i16>::new(duration, spec));
        }

        if let Some(buf) = &mut sample_buf {
            let frames = audio_buf.frames();
            buf.copy_planar_ref(audio_buf);
            // TODO faster way to do this?
            for frame in &buf.samples()[0..frames] {
                result.extend_from_slice(&frame.to_ne_bytes());
            }
        }
    }

    result.into_boxed_slice()
}

fn main() {
    ctru::use_panic_handler();

    let gfx = Gfx::init().unwrap();
    let apt = Apt::init().unwrap();
    let hid = Hid::init().unwrap();
    let mut ndsp = Ndsp::init().unwrap();
    ndsp.set_output_mode(OutputMode::Mono);

    let channel = ndsp.channel(0).unwrap();
    channel.reset();
    channel.set_interpolation(InterpolationType::Polyphase);
    channel.set_sample_rate(48000.0);
    channel.set_format(AudioFormat::PCM16Mono);

    let audio_buffer = decode_audio();
    let mut wave_info = WaveInfo::new(audio_buffer, AudioFormat::PCM16Mono, true);
    channel.queue_wave(&mut wave_info).unwrap();
    channel.set_paused(false);

    let _console = Console::init(gfx.bottom_screen.borrow_mut());

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
