//! §5 stage 13 — does the export policy run where §0 says it has to?
//!
//! §16 #11 chose a gamut-mapping policy on colorimetric evidence (`tests/color/`,
//! `SPIKE-B.md`). That evidence is necessary and not sufficient. Stage 13 is the one
//! stage every pixel of *both* paths goes through — the preview writes it to the
//! canvas, the exporter writes it to a file — and §0 freezes those to one shader
//! source. A policy that cannot be expressed there is not a policy this project can
//! adopt, whatever its ΔE.
//!
//! That failure mode is not hypothetical here. It is exactly what killed the fork:
//! Spike A found stage order encoded inside a compute kernel with no GLSL ES 3.0
//! target, and a frozen register item turned out to be unimplementable. So this file
//! asks the same question of stage 13 *before* the register entry is written, not
//! after.
//!
//! Two things are checked, and the second is the one that matters:
//!
//! 1. The WGSL lowers to GLSL ES 3.00, which is what WebGL2 compiles.
//! 2. Run through wgpu, it computes **the same policy** as `photodesk_color::gamut` —
//!    the reference implementation the decision was actually measured on. A shader
//!    that compiles and computes something slightly different is precisely the
//!    WYSIWYG drift §0 freezes against, wearing a build step as a disguise.
//!
//! The reference is imported rather than transcribed, and since the colour code moved
//! into `photodesk::engine` it is the *shipped* policy rather than a harness copy of
//! it. A second copy of the maths could agree with itself and be wrong, which is the
//! mistake `tests/color/`'s "one workload definition, not two" rule already prevents.

use half::f16;
use naga::ShaderStage;
use photodesk::engine::colour::{LINEAR_P3, SRGB};
use photodesk::engine::gamut::{GamutPolicy, luma_weights};
use photodesk_renderer_spike::{ENCODE_WGSL, to_glsl_es300};
use wgpu::util::DeviceExt;

/// 128 × 128 is 16,384 samples of the policy, which is plenty — this is a question
/// about arithmetic, not about an image.
const SIZE: u32 = 128;

/// The uniform block, laid out to match the WGSL `Encode` struct.
///
/// `mat3x3<f32>` occupies three 16-byte columns in a uniform buffer, so the padding
/// is not decoration — getting it wrong shifts every field after it and the shader
/// reads garbage that still renders.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Encode {
    to_dst: [[f32; 4]; 3],
    luma: [f32; 4],
    params: [f32; 4],
}

fn uniforms(policy: GamutPolicy) -> Encode {
    let m = LINEAR_P3.linear_to(&SRGB).0;
    let w = luma_weights(&SRGB);
    // WGSL matrices are column-major; ours is row-major, so this transposes.
    let mut to_dst = [[0.0f32; 4]; 3];
    for (col, cell) in to_dst.iter_mut().enumerate() {
        for row in 0..3 {
            cell[row] = m[row][col] as f32;
        }
    }
    let params = match policy {
        GamutPolicy::PreserveLuma => [0.0, 0.0, 0.0, 0.0],
        GamutPolicy::CompressLuma { knee } => [1.0, knee, 0.0, 0.0],
        other => panic!("stage 13 implements the ray policies; {other:?} is not one of them"),
    };
    Encode {
        to_dst,
        luma: [w[0] as f32, w[1] as f32, w[2] as f32, 0.0],
        params,
    }
}

/// A source image in the working space that is deliberately *not* mostly in gamut.
///
/// A photograph would be 97% in-gamut and the agreement test would then be dominated
/// by the identity path, where any two implementations agree trivially. The policy
/// only does anything outside the gamut, so that is where the samples have to be.
fn source_image() -> Vec<f32> {
    let mut v = vec![0.0f32; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = ((y * SIZE + x) * 4) as usize;
            let u = x as f32 / (SIZE - 1) as f32;
            let t = y as f32 / (SIZE - 1) as f32;
            // A hue sweep across x, a lightness sweep up y, and a chroma that runs
            // well past the sRGB boundary — including channels below zero, which is
            // what a P3 colour becomes after the matrix.
            let angle = u * std::f32::consts::TAU;
            let radius = 0.15 + t * 0.85;
            let base = 0.05 + t * 0.9;
            v[i] = base + radius * angle.cos();
            v[i + 1] = base + radius * (angle - 2.094).cos();
            v[i + 2] = base + radius * (angle + 2.094).cos();
            v[i + 3] = 1.0;
        }
    }
    v
}

/// The reference: the same chain in Rust, using the policy implementation the
/// decision was measured on.
fn reference(src: &[f32], policy: GamutPolicy) -> Vec<f32> {
    let m = LINEAR_P3.linear_to(&SRGB);
    let w = luma_weights(&SRGB);
    let mut out = vec![0.0f32; src.len()];
    for px in 0..src.len() / 4 {
        let i = px * 4;
        // Through f16 first: the GPU samples an RGBA16F texture, so the reference has
        // to start from the same value the shader actually sees rather than from the
        // f32 the texture was built out of.
        let working = [
            f16::from_f32(src[i]).to_f32(),
            f16::from_f32(src[i + 1]).to_f32(),
            f16::from_f32(src[i + 2]).to_f32(),
        ];
        let dst_linear = m.apply(working);
        let mapped = policy.map_with(dst_linear, w);
        for k in 0..3 {
            out[i + k] = SRGB.transfer.from_linear(mapped[k]);
        }
        out[i + 3] = f16::from_f32(src[i + 3]).to_f32();
    }
    out
}

// ------------------------------------------------------------------- 1. it lowers

#[test]
fn stage_13_lowers_to_glsl_es_300() {
    for (entry, stage) in [("vs_main", ShaderStage::Vertex), ("fs_main", ShaderStage::Fragment)] {
        match to_glsl_es300(ENCODE_WGSL, entry, stage) {
            Ok(t) => {
                println!("{entry}: lowered, {} bytes of GLSL", t.source.len());
                assert!(
                    t.source.contains("#version 300 es"),
                    "backend did not emit an ES 3.00 header for {entry}"
                );
                for forbidden in ["layout(std430", "imageStore", "buffer "] {
                    assert!(
                        !t.source.contains(forbidden),
                        "{entry} lowered to something containing `{forbidden}`, which GLSL \
                         ES 3.00 has no notion of"
                    );
                }
                if stage == ShaderStage::Fragment {
                    println!("bound resources: {:?}", t.uniform_names);
                }
            }
            Err(e) => panic!(
                "§16 #11's policy does not lower to the preview path.\n{e}\n\
                 That is a finding about the policy, not a build failure: a gamut map \
                 stage 13 cannot run is not a gamut map this project can adopt, and the \
                 register entry has to change before anything depends on it."
            ),
        }
    }

    // The specific constructs the policy needs, checked by name rather than by the
    // absence of an error — a backend that silently dropped one would still "lower".
    let t = to_glsl_es300(ENCODE_WGSL, "fs_main", ShaderStage::Fragment).expect("fragment lowers");
    for needed in ["mat3", "clamp", "dot"] {
        assert!(
            t.source.contains(needed),
            "the lowered fragment shader has no `{needed}` in it; the policy's maths did \
             not survive the transpilation"
        );
    }
    println!(
        "\n  the policy is a matrix multiply, a dot product, three divides and a min — \n\
         \x20 no loop, no table, no per-pixel search. That is why it was chosen from among \n\
         \x20 the closed forms rather than from among the colorimetrically best."
    );
}

// ------------------------------------------------- 2. it computes the same policy

#[test]
fn stage_13_computes_the_same_policy_as_the_reference() {
    let src = source_image();

    // How much of the corpus is actually exercising the policy. Reported because an
    // agreement test on in-gamut content would agree trivially and prove nothing.
    let m = LINEAR_P3.linear_to(&SRGB);
    let outside = (0..src.len() / 4)
        .filter(|px| {
            let i = px * 4;
            m.apply([src[i], src[i + 1], src[i + 2]])
                .iter()
                .any(|c| !(0.0..=1.0).contains(c))
        })
        .count();
    println!(
        "source: {}x{}, {} of {} samples outside sRGB ({:.1}%)",
        SIZE,
        SIZE,
        outside,
        src.len() / 4,
        100.0 * outside as f64 / (src.len() / 4) as f64
    );
    assert!(
        outside > src.len() / 8,
        "only {outside} samples leave the gamut; this corpus would agree on the identity \
         path and call it agreement"
    );

    for policy in [
        GamutPolicy::PreserveLuma,
        GamutPolicy::CompressLuma { knee: 0.95 },
    ] {
        let want = reference(&src, policy);
        let got = match render(&src, policy) {
            Ok(v) => v,
            Err(e) => {
                // A missing GPU is an environment fact, not a finding about the policy.
                eprintln!("SKIP: wgpu unavailable in this environment: {e}");
                return;
            }
        };
        assert_eq!(got.len(), want.len());

        // Colour channels only; alpha is carried through, not computed.
        let channels: Vec<usize> = (0..want.len()).filter(|i| i % 4 != 3).collect();

        // What the render target's own precision costs, before anything is attributed
        // to the shader. The comparison is an RGBA16F attachment against an f32
        // reference, so a disagreement below one f16 step is the attachment, not the
        // maths — and this adapter turns out not even to round it to nearest.
        let nearest = channels.iter().filter(|i| got[**i] == f16::from_f32(want[**i]).to_f32()).count();
        let toward_zero = channels.iter().filter(|i| got[**i] == f16_toward_zero(want[**i])).count();
        let steps = channels
            .iter()
            .map(|i| f16_steps_apart(got[*i], want[*i]))
            .fold(0.0f32, f32::max);
        let absolute = channels
            .iter()
            .map(|i| (got[*i] - want[*i]).abs())
            .fold(0.0f32, f32::max);
        let used = channels
            .iter()
            .map(|i| budget_used(got[*i], want[*i]))
            .fold(0.0f32, f32::max);
        let worst_at = *channels
            .iter()
            .max_by(|a, b| budget_used(got[**a], want[**a]).total_cmp(&budget_used(got[**b], want[**b])))
            .expect("channels");
        let px = worst_at / 4;

        println!(
            "{:<18} worst channel uses {:.1}% of the precision budget ({:.3} attachment \
             steps, |Δ| {absolute:.3e}), at sample {px}\n\
             \x20                  shader {:?}\n\
             \x20                  reference {:?}\n\
             \x20                  {} of {} channels are the reference rounded to nearest f16; \
             {} are truncated toward zero",
            policy.label(),
            used * 100.0,
            steps,
            &got[px * 4..px * 4 + 3],
            &want[px * 4..px * 4 + 3],
            nearest,
            channels.len(),
            toward_zero,
        );

        // The bar, and the reason it is stated in f16 steps rather than in absolute
        // difference: both are the same claim, but only one of them says what the
        // limit *is*. The shader and the reference read the same f16 texel, apply the
        // same matrix and the same policy, and differ only in the order the GPU
        // associates its arithmetic — which is ulp territory. One step of the
        // attachment's own grid is therefore the whole budget, and anything beyond it
        // is the shader computing a different policy, which is the WYSIWYG drift §0
        // freezes against.
        assert!(
            used <= 1.0,
            "{} disagrees with its reference by {:.1}% of the precision budget ({steps:.3} \
             attachment steps). §0 freezes preview and export to one shader source; a \
             shader that computes a different policy from the one §16 #11 measured is that \
             invariant failing quietly.",
            policy.label(),
            used * 100.0
        );

        // The strongest form of the claim, and the reason the budget above is not just
        // a loose tolerance with a story attached: the shader's output is *bit-exactly*
        // the reference truncated to f16 for all but a handful of channels. Two
        // different policies do not agree bit for bit.
        let residual = channels.len() - toward_zero;
        assert!(
            toward_zero * 100 >= channels.len() * 95,
            "only {toward_zero} of {} channels are bit-exactly the reference truncated to \
             f16. Below that the agreement is a tolerance rather than an identity, and the \
             shader should be re-read before the policy is trusted in it.",
            channels.len()
        );
        println!(
            "  {residual} channels are not bit-exact, which is where a sub-ulp arithmetic \
             difference crosses a bucket."
        );
    }

    println!(
        "\n  Recorded because §12.1 and §12.2 will both hit it: on this adapter the RGBA16F \n\
         \x20 colour attachment does not round to nearest, it truncates toward zero. That is a \n\
         \x20 half-step bias on every stored channel, it is the driver's rounding mode rather \n\
         \x20 than the shader's arithmetic, and a golden-image threshold that does not allow \n\
         \x20 for it will fail on a correct render."
    );
}

/// How far apart two outputs are, in units of the coarser of the two precisions that
/// produced them.
///
/// An absolute tolerance would be far too loose near one and far too tight near zero,
/// so the unit is the f16 attachment's own grid spacing — except in the deep shadows,
/// where f16 has subnormals finer than the f32 arithmetic feeding them and a
/// difference of 7e-7 would read as twelve steps of a grid that is not the limiting
/// precision. The floor is the f32 chain's own noise: about ten operations at f32
/// epsilon on values near one.
const F32_CHAIN_NOISE: f32 = 1.0e-6;

/// The whole precision budget for one channel: one step of the colour attachment's
/// grid, plus the f32 chain's own noise.
///
/// Both terms are needed and neither is slack. The attachment costs a full step
/// rather than half a one because this adapter truncates instead of rounding; and a
/// sub-ulp difference in the order the GPU associates its arithmetic can then push a
/// value across into the next-lower bucket, which costs slightly more than the step
/// on its own. Nothing in this budget is the policy — a shader computing a *different*
/// policy is wrong by ΔE-scale amounts, four orders of magnitude above it.
fn precision_budget(a: f32, b: f32) -> f32 {
    f16_ulp(a.abs().max(b.abs())) + F32_CHAIN_NOISE
}

/// The share of that budget a channel's disagreement uses. Over 1.0 is a finding.
fn budget_used(a: f32, b: f32) -> f32 {
    (a - b).abs() / precision_budget(a, b)
}

/// Raw distance in attachment steps, reported alongside so the truncation stays
/// visible rather than being absorbed into a ratio.
fn f16_steps_apart(a: f32, b: f32) -> f32 {
    (a - b).abs() / f16_ulp(a.abs().max(b.abs())).max(F32_CHAIN_NOISE)
}

fn f16_ulp(magnitude: f32) -> f32 {
    let h = f16::from_f32(magnitude.max(f16::MIN_POSITIVE.to_f32()));
    let step = f16::from_bits(h.to_bits() + 1).to_f32() - h.to_f32();
    if step > 0.0 { step } else { f16::MIN_POSITIVE.to_f32() }
}

/// `v` rounded to f16 *toward zero* rather than to nearest — what this adapter's
/// colour attachment actually does.
fn f16_toward_zero(v: f32) -> f32 {
    let n = f16::from_f32(v);
    if n.to_f32().abs() > v.abs() && n.to_bits() & 0x7FFF != 0 {
        f16::from_bits(n.to_bits() - 1).to_f32()
    } else {
        n.to_f32()
    }
}

/// Render the encode stage once through wgpu.
fn render(src: &[f32], policy: GamutPolicy) -> Result<Vec<f32>, String> {
    let src_half: Vec<f16> = src.iter().map(|v| f16::from_f32(*v)).collect();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        ..Default::default()
    }))
    .map_err(|e| format!("no wgpu adapter: {e}"))?;
    eprintln!("wgpu adapter: {}", adapter.get_info().name);

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("stage-13"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| format!("no device: {e}"))?;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("encode.wgsl"),
        source: wgpu::ShaderSource::Wgsl(ENCODE_WGSL.into()),
    });

    let src_tex = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("src"),
            size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytemuck::cast_slice(&src_half),
    );
    let dst_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("dst"),
        size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let ubo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("encode"),
        contents: bytemuck::bytes_of(&uniforms(policy)),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    // Nearest: the shader samples texel centres, and any filtering difference between
    // the two sides would be attributed to the policy.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("encode"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: ubo.as_entire_binding() },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(
                    &src_tex.create_view(&Default::default()),
                ),
            },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    });

    let bytes_per_row = SIZE * 8; // RGBA16F
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bytes_per_row * SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut enc = device.create_command_encoder(&Default::default());
    {
        let view = dst_tex.create_view(&Default::default());
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &dst_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
    );
    queue.submit(Some(enc.finish()));

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("poll: {e:?}"))?;
    let data = slice.get_mapped_range().map_err(|e| format!("map: {e:?}"))?;
    let out: Vec<f32> = bytemuck::cast_slice::<u8, f16>(&data)
        .iter()
        .map(|h| h.to_f32())
        .collect();
    drop(data);
    readback.unmap();
    Ok(out)
}
