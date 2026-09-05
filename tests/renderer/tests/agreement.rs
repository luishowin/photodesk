//! Spike C, part 2 — do the two paths agree on pixels?
//!
//! Part 1 established that the WGSL lowers to GLSL ES 3.00 and that WebKitGTK compiles
//! it. Neither fact is correctness. §0 freezes "one shader source, preview and export"
//! on the argument that separate paths guarantee undiscoverable WYSIWYG drift — and a
//! transpiled shader that compiles but computes something slightly different is exactly
//! that drift, wearing a build step as a disguise.
//!
//! So this renders the WGSL natively through wgpu, writes the result where the WebGL2
//! harness can fetch it, and lets the browser compare its own output against it. The
//! §12.2 proxy/full-res test will inherit this comparison later; here it is answering
//! whether one shader source is possible at all.

use half::f16;
use std::path::PathBuf;
use wgpu::util::DeviceExt;

const SIZE: u32 = 256;

fn out_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/web/generated"))
}

/// The adjustment block, laid out to match the WGSL `Adjustments` struct.
///
/// Written by hand rather than derived, because that is what the real bridge will do
/// and a layout mistake here is the same mistake it would make. The GLSL side does not
/// reuse these bytes — it queries std140 offsets from the linked program and writes
/// named fields — so agreement is evidence the two layouts actually match rather than
/// evidence they share a buffer.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Adjustments {
    temperature: f32,
    tint: f32,
    exposure: f32,
    highlights: f32,
    shadows: f32,
    blacks: f32,
    contrast: f32,
    vibrance: f32,
    saturation: f32,
    _pad0: [f32; 3],

    luma_curve: [[f32; 4]; 8],
    red_curve: [[f32; 4]; 8],
    green_curve: [[f32; 4]; 8],
    blue_curve: [[f32; 4]; 8],
    curve_counts: [u32; 4],

    hsl: [[f32; 4]; 8],

    grade_shadows: [f32; 4],
    grade_midtones: [f32; 4],
    grade_highlights: [f32; 4],
    grade_global: [f32; 4],
    grade_blend: f32,
    grade_balance: f32,
    _pad1: [f32; 2],
}

/// A parameter set that lights up every stage, including the tone curve — whose
/// dynamically-indexed loop is the construct most likely to lower differently.
fn test_params() -> Adjustments {
    let mut a: Adjustments = bytemuck::Zeroable::zeroed();
    a.temperature = 18.0;
    a.tint = -9.0;
    a.exposure = 0.62;
    a.highlights = -35.0;
    a.shadows = 42.0;
    a.blacks = -12.0;
    a.contrast = 22.0;
    a.vibrance = 30.0;
    a.saturation = -8.0;

    // Two curves with four points each, packed two points per vec4.
    let pts = [[0.0f32, 0.02, 0.25, 0.20], [0.75, 0.82, 1.0, 0.98]];
    a.luma_curve[0] = pts[0];
    a.luma_curve[1] = pts[1];
    a.red_curve[0] = pts[0];
    a.red_curve[1] = [0.75, 0.78, 1.0, 1.0];
    a.curve_counts = [4, 4, 0, 0];

    for (i, band) in a.hsl.iter_mut().enumerate() {
        *band = [(i as f32 - 4.0) * 2.0, (i as f32) * 3.0 - 8.0, 4.0 - i as f32, 0.0];
    }
    a.grade_shadows = [0.02, -0.01, 0.04, 0.0];
    a.grade_midtones = [-0.01, 0.015, 0.005, 0.0];
    a.grade_highlights = [0.03, 0.01, -0.02, 0.0];
    a.grade_global = [0.005, 0.0, -0.005, 0.0];
    a.grade_blend = 0.8;
    a.grade_balance = 12.0;
    a
}

/// A deterministic source image. Written to disk rather than reproduced by a formula on
/// each side: two implementations of the same generator is one more thing that can
/// silently disagree, and it would disagree inside the very measurement meant to detect
/// disagreement.
fn source_image() -> Vec<f32> {
    let mut v = vec![0.0f32; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = ((y * SIZE + x) * 4) as usize;
            let u = x as f32 / SIZE as f32;
            let w = y as f32 / SIZE as f32;
            let ring = ((u - 0.5).powi(2) + (w - 0.5).powi(2)).sqrt();
            let d = (u * 37.0).sin() * (w * 29.0).cos() * 0.06;
            v[i] = (0.05 + u * 0.9 + d).clamp(0.0, 1.0);
            v[i + 1] = (0.03 + w * 0.85 + d * 0.7).clamp(0.0, 1.0);
            v[i + 2] = (0.08 + (1.0 - ring) * 0.7 + d).clamp(0.0, 1.0);
            v[i + 3] = 1.0;
        }
    }
    v
}

fn render_with_wgpu(src: &[f32], params: &Adjustments) -> Result<Vec<f32>, String> {
    // Both paths store RGBA16F, because that is §4's working space. Matching formats
    // is what makes the comparison about shader maths rather than about one side
    // carrying more precision than the other.
    let src_half: Vec<f16> = src.iter().map(|v| f16::from_f32(*v)).collect();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        ..Default::default()
    }))
    .map_err(|e| format!("no wgpu adapter: {e}"))?;

    let info = adapter.get_info();
    eprintln!(
        "wgpu adapter: {} ({:?}, {:?})",
        info.name, info.backend, info.device_type
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("spike-c"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| format!("no device: {e}"))?;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("adjust.wgsl"),
        source: wgpu::ShaderSource::Wgsl(photodesk_renderer_spike::ADJUST_WGSL.into()),
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
        label: Some("adjustments"),
        contents: bytemuck::bytes_of(params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    // Nearest, because the shader samples texel centres and any filtering difference
    // between the two paths would be attributed to the maths.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("adjust"),
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

#[test]
fn emit_wgpu_reference_for_the_web_harness() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).expect("create web/generated");

    let src = source_image();
    std::fs::write(dir.join("source.f32"), bytemuck::cast_slice(&src)).expect("write source");

    let params = test_params();
    std::fs::write(dir.join("params.bin"), bytemuck::bytes_of(&params)).expect("write params");
    // Named fields, so the GLSL side can place them at std140 offsets it queries itself
    // rather than trusting that the two layouts happen to match.
    let p = &params;
    let json = format!(
        r#"{{"size":{SIZE},"temperature":{},"tint":{},"exposure":{},"highlights":{},"shadows":{},"blacks":{},"contrast":{},"vibrance":{},"saturation":{},"curve_counts":[{},{},{},{}],"luma_curve":{:?},"red_curve":{:?},"hsl":{:?},"grade_shadows":{:?},"grade_midtones":{:?},"grade_highlights":{:?},"grade_global":{:?},"grade_blend":{},"grade_balance":{}}}"#,
        p.temperature, p.tint, p.exposure, p.highlights, p.shadows, p.blacks, p.contrast,
        p.vibrance, p.saturation,
        p.curve_counts[0], p.curve_counts[1], p.curve_counts[2], p.curve_counts[3],
        p.luma_curve, p.red_curve, p.hsl,
        p.grade_shadows, p.grade_midtones, p.grade_highlights, p.grade_global,
        p.grade_blend, p.grade_balance
    );
    std::fs::write(dir.join("params.json"), json).expect("write params.json");

    match render_with_wgpu(&src, &params) {
        Ok(out) => {
            std::fs::write(dir.join("reference.f32"), bytemuck::cast_slice(&out))
                .expect("write reference");
            let finite = out.iter().filter(|v| v.is_finite()).count();
            println!(
                "wgpu reference: {}x{}, {} floats, {} finite, first pixel {:?}",
                SIZE, SIZE, out.len(), finite, &out[..4]
            );
            assert_eq!(finite, out.len(), "wgpu render produced non-finite values");
            assert!(
                out[..out.len().min(4096)].iter().any(|v| *v != 0.0),
                "wgpu render is entirely zero — the pass did not run"
            );
        }
        Err(e) => {
            // A missing GPU is an environment fact, not a Spike C finding. The WebGL2
            // side still runs; it just has nothing to be compared against, and the
            // harness says so rather than reporting agreement it did not measure.
            eprintln!("SKIP: wgpu unavailable in this environment: {e}");
            let _ = std::fs::remove_file(dir.join("reference.f32"));
        }
    }
}
