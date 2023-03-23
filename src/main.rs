#![feature(allocator_api)]
#![feature(maybe_uninit_write_slice)]
#![feature(new_uninit)]

use std::{
    cell::RefMut,
    f32::consts::{PI, TAU},
    ffi::CString,
    mem::MaybeUninit,
    ptr::addr_of,
    sync::atomic::{AtomicU32, Ordering},
};

use citro3d::render::ClearFlags;
use ctru::{
    gfx::TopScreen3D,
    linear::LinearAllocator,
    prelude::*,
    services::{
        gspgpu::FramebufferFormat,
        hid::CirclePosition,
        ndsp::{Ndsp, OutputMode},
    },
};

#[cfg(not(debug_assertions))]
use ctru::services::ndsp::{wave::WaveInfo, AudioFormat, InterpolationType};
#[cfg(not(debug_assertions))]
use std::io::Cursor;
#[cfg(not(debug_assertions))]
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{DecoderOptions, CODEC_TYPE_NULL},
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

include!(concat!(env!("OUT_DIR"), "/maxwell.rs"));

static SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shader.shbin"));
static BODY_TEXTURE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/body.t3x"));
static WHISKERS_TEXTURE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/whiskers.t3x"));

static VERTICES: &[f32] = MAXWELL_MODEL.vertices;
static BODY_INDICES: &[u16] = MAXWELL_MODEL.body;
static WHISKERS_INDICES: &[u16] = MAXWELL_MODEL.whiskers;

#[cfg(not(debug_assertions))]
static MUSIC_OGG: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maxwell.ogg"));

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
                texture_data.as_ptr().cast(),
                texture_data.len(),
                tex.as_mut_ptr(),
                std::ptr::null_mut(),
                false,
            );
            assert!(!texture.is_null(), "failed to import texture");
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
                i32::try_from(self.vao.len()).unwrap(),
                i32::try_from(citro3d_sys::C3D_UNSIGNED_SHORT).unwrap(),
                self.vao.as_ptr().cast(),
            );
        }
    }
}

fn move_to_linear<T>(memory: &[T]) -> Box<[T], LinearAllocator>
where
    T: Copy,
{
    // create uninit slice
    let mut slice = Box::new_uninit_slice_in(memory.len(), LinearAllocator);
    MaybeUninit::write_slice(&mut slice, memory);
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

fn create_target(screen: RefMut<'_, dyn ctru::gfx::Screen>) -> citro3d::render::Target<'_> {
    citro3d::render::Target::new(
        240,
        400,
        screen,
        Some(citro3d::render::DepthFormat::Depth24Stencil8),
    )
    .unwrap()
}

fn get_slider_state() -> f32 {
    // SAFETY: The pointer is valid because we know the address is properly
    // mapped on this hardware. In addition, we use an atomic load, so reading
    // the data happens atomically, avoiding invalid reads.
    unsafe {
        // get a pointer to the slider data
        let config = ctru_sys::OS_SHAREDCFG_VADDR as *const ctru_sys::osSharedConfig_s;
        // cast to a pointer to an atomic dword, to ensure we access it atomically
        // this is safe because atomic types has the same in-memory representation as
        // their contained values - and u32 and f32 are pretty safely interchangable
        let pointer = &*(addr_of!((*config).slider_3d)).cast::<AtomicU32>();
        // load the data and cast to f32
        f32::from_bits(pointer.load(Ordering::SeqCst))
    }
}

fn get_uniform_location(program: &mut citro3d::shader::Program, name: &str) -> i32 {
    let name = CString::new(name).unwrap();
    unsafe {
        i32::from(ctru_sys::shaderInstanceGetUniformLocation(
            (*program.as_raw()).vertexShader,
            name.as_ptr(),
        ))
    }
}

struct Scene {
    angle_x: f32,
    angle_y: f32,

    do_spin: bool,
    do_bounce: bool,

    bounce_pos: f32,

    body_mat: Material,
    whiskers_mat: Material,

    shader_projection: i32,
    shader_model_view: i32,
    shader_light_angle: i32,
}

impl Scene {
    fn render(
        &mut self,
        instance: &mut citro3d::Instance,
        target: &mut citro3d::render::Target<'_>,
        iod: f32,
    ) {
        target.clear(ClearFlags::ALL, 0xff_ff_ff_ff, 0);

        instance.select_render_target(target).unwrap();

        // SAFETY: it's just matrix math
        unsafe {
            let mut projection = MaybeUninit::uninit();
            citro3d_sys::Mtx_PerspStereoTilt(
                projection.as_mut_ptr(),
                PI / 2.0,
                400.0 / 240.0,
                0.01,
                100.0,
                iod,
                3.0,
                false,
            );
            let projection = projection.assume_init();

            let mut model_view = citro3d_sys::C3D_Mtx {
                r: [
                    citro3d_sys::C3D_FVec {
                        c: [0.0, 0.0, 0.0, 1.0],
                    },
                    citro3d_sys::C3D_FVec {
                        c: [0.0, 0.0, 1.0, 0.0],
                    },
                    citro3d_sys::C3D_FVec {
                        c: [0.0, 1.0, 0.0, 0.0],
                    },
                    citro3d_sys::C3D_FVec {
                        c: [1.0, 0.0, 0.0, 0.0],
                    },
                ],
            };
            citro3d_sys::Mtx_Translate(&mut model_view, 0.0, -10.0, -25.0, true);
            // bouncing translation
            let bounce_sin = self.bounce_pos.sin();
            citro3d_sys::Mtx_RotateZ(&mut model_view, bounce_sin * 0.25, true);
            citro3d_sys::Mtx_Translate(&mut model_view, 0.0, bounce_sin.abs() * 4.0, 0.0, true);
            citro3d_sys::Mtx_RotateX(&mut model_view, self.angle_x, true);
            citro3d_sys::Mtx_RotateY(&mut model_view, self.angle_y, true);

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

            citro3d_sys::C3D_FVUnifSet(
                ctru_sys::GPU_VERTEX_SHADER,
                self.shader_light_angle,
                0.0,
                0.577_350_26,
                0.577_350_26,
                0.577_350_26,
            );
        }

        self.body_mat.draw();
        self.whiskers_mat.draw();
    }

    fn update(
        &mut self,
        down: KeyPad,
        instance: &mut citro3d::Instance,
        left: &mut citro3d::render::Target,
        right: &mut citro3d::render::Target,
    ) -> bool {
        if down.contains(KeyPad::KEY_START) {
            return false;
        }

        if down.contains(KeyPad::KEY_A) {
            self.do_spin = !self.do_spin;
        }

        if down.contains(KeyPad::KEY_B) {
            self.do_bounce = !self.do_bounce;
            if !self.do_bounce {
                self.bounce_pos = 0.0;
            }
        }

        if down.contains(KeyPad::KEY_X) {
            self.angle_x = 0.0;
            self.angle_y = INITIAL_ANGLE_Y;
        }

        let (mut x, mut y) = CirclePosition::new().get();
        // apply a deadzone to these values to avoid drift
        if x.abs() < 20 {
            x = 0;
        }
        if y.abs() < 20 {
            y = 0;
        }
        // intentionally reversed - rotations in 3d do not line up with
        // 2d location of circle pad
        self.angle_y += f32::from(x) * (1.0 / 2048.0);
        self.angle_x += f32::from(y) * (1.0 / 2048.0);

        if self.do_spin {
            self.angle_y += 0.0625;
        }
        if self.do_bounce {
            self.bounce_pos += 0.116_923_66;
            self.bounce_pos = self.bounce_pos.rem_euclid(TAU);
        }

        self.angle_x = self.angle_x.rem_euclid(TAU);
        self.angle_y = self.angle_y.rem_euclid(TAU);

        let depth = get_slider_state();

        instance.render_frame_with(|instance| {
            self.render(instance, left, -depth);
            if depth > 0.0 {
                self.render(instance, right, depth);
            }
        });

        true
    }
}

#[cfg(not(debug_assertions))]
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

const INITIAL_ANGLE_Y: f32 = 5.25;

fn main() {
    ctru::use_panic_handler();

    let gfx = Gfx::init().unwrap();
    let _console = Console::init(gfx.bottom_screen.borrow_mut());

    let apt = Apt::init().unwrap();
    let hid = Hid::init().unwrap();

    let mut ndsp = Ndsp::init().unwrap();
    ndsp.set_output_mode(OutputMode::Mono);
    let channel = ndsp.channel(0).unwrap();
    #[cfg(not(debug_assertions))]
    let mut wave_info;

    #[cfg(not(debug_assertions))]
    {
        println!("decoding audio stream...");

        channel.reset();
        channel.set_interpolation(InterpolationType::Polyphase);
        channel.set_sample_rate(48000.0);
        channel.set_format(AudioFormat::PCM16Mono);

        // ask for fast cpu while we decode
        unsafe { ctru_sys::osSetSpeedupEnable(true) };
        let audio_buffer = decode_audio();
        unsafe { ctru_sys::osSetSpeedupEnable(false) };

        wave_info = WaveInfo::new(audio_buffer, AudioFormat::PCM16Mono, true);
        channel.queue_wave(&mut wave_info).unwrap();
        channel.set_paused(false);
    }

    #[cfg(debug_assertions)]
    {
        let _ = channel;
        println!("audio decoding too slow in debug build");
    }

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
        citro3d_sys::AttrInfo_AddLoader(attr_info, 2, ctru_sys::GPU_FLOAT, 3); // v2 = normal
    }

    let vertices = move_to_linear(VERTICES);

    let mut scene = Scene {
        angle_x: 0.0,
        angle_y: INITIAL_ANGLE_Y,

        do_spin: true,
        do_bounce: false,

        bounce_pos: 0.0,

        body_mat: Material::new(BODY_INDICES, BODY_TEXTURE),
        whiskers_mat: Material::new(WHISKERS_INDICES, WHISKERS_TEXTURE),

        shader_projection: get_uniform_location(&mut program, "projection"),
        shader_model_view: get_uniform_location(&mut program, "model_view"),
        shader_light_angle: get_uniform_location(&mut program, "light_angle"),
    };

    unsafe {
        let buf_info = citro3d_sys::C3D_GetBufInfo();
        citro3d_sys::BufInfo_Init(buf_info);
        citro3d_sys::BufInfo_Add(
            buf_info,
            vertices.as_ptr().cast(),
            isize::try_from(std::mem::size_of::<f32>() * 8).unwrap(),
            3,
            0x210,
        );

        let env = citro3d_sys::C3D_GetTexEnv(0);
        citro3d_sys::C3D_TexEnvInit(env);
        citro3d_sys::C3D_TexEnvSrc(
            env,
            citro3d_sys::C3D_Both,
            ctru_sys::GPU_TEXTURE0,
            ctru_sys::GPU_PRIMARY_COLOR,
            0,
        );
        citro3d_sys::C3D_TexEnvFunc(env, citro3d_sys::C3D_Both, ctru_sys::GPU_MODULATE);

        citro3d_sys::C3D_CullFace(ctru_sys::GPU_CULL_NONE);
    }

    println!("press [A] to turn rotation on/off");
    println!("press [B] to turn bouncing on/off");
    println!("press [X] to reset rotation");
    println!("press [START] to quit");
    println!("use circle pad to rotate manually");
    println!();
    println!("github.com/spazzylemons/maxwell-3ds");

    while apt.main_loop() {
        hid.scan_input();
        let down = hid.keys_down();

        if !scene.update(down, &mut instance, &mut left, &mut right) {
            break;
        }
    }
}
