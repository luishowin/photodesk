//! Executing a compiled graph (§6.2) through wgpu — §7.2's export path, and the
//! reference the WebGL2 preview has to agree with.
//!
//! ## There is no CPU renderer, and there will not be one
//!
//! The obvious way to test a GPU renderer is to write the same maths in Rust and
//! compare. That is precisely the thing §0 freezes against: two implementations of one
//! pipeline drift, and the drift is undiscoverable because both look right in
//! isolation. So the shaders in `shaders/photodesk/` are the only description of what
//! a pixel goes through, this module binds a graph node to a draw, and correctness is
//! established the way §12.2 says — by rendering *the same document* two ways and
//! requiring the answers to match.
//!
//! ## Memory, and why nodes are released
//!
//! §7.3 caps proxy and graph together at 512 MB. A six-layer masked graph is around
//! fifteen nodes, and holding every intermediate at full resolution would be 12 MP ×
//! 15 × 8 bytes — 1.4 GB, nearly three times the budget. So a node's texture is
//! dropped as soon as its last consumer has run, which is a reference count over the
//! edges the graph already carries.
//!
//! Full-resolution export additionally wants **tiling** (§7.1), which this does not do
//! yet. At proxy — where §12.2's comparison and the whole preview path live — the
//! whole-image path is what is wanted anyway.

use std::collections::HashMap;

use half::f16;

use crate::photodesk::document::{ColorSpace, Params};
use crate::photodesk::graph::{Graph, NodeId, NodeKind};

use super::colour::{DISPLAY_P3, LINEAR_P3, SRGB, Space};
use super::gamut::{EXPORT_GAMUT_POLICY, GamutPolicy, luma_weights};
use super::image::Image;

/// §5 stages 2–9, fused (§7.3). The only description of what those stages do.
pub const ADJUST_WGSL: &str = include_str!("../../../shaders/photodesk/adjust.wgsl");

/// §5 stage 13, with §16 #11's gamut policy.
pub const ENCODE_WGSL: &str = include_str!("../../../shaders/photodesk/encode.wgsl");

#[derive(Debug)]
pub enum RenderError {
    /// No GPU. An environment fact rather than a finding about the graph — §12.1 and
    /// §12.2 skip on it rather than failing, because a machine without an adapter has
    /// not told us anything about the renderer.
    NoAdapter(String),
    Device(String),
    /// A node kind the renderer does not implement yet.
    ///
    /// Named rather than skipped. A graph containing a mask is a v0.4 document, and
    /// rendering it without the mask would produce a picture that looks plausible and
    /// is wrong — the same reason §6.3 rejects an unknown `op` instead of ignoring it.
    Unimplemented { kind: &'static str, since: &'static str },
    Readback(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::NoAdapter(e) => write!(f, "no GPU adapter: {e}"),
            RenderError::Device(e) => write!(f, "could not open a GPU device: {e}"),
            RenderError::Unimplemented { kind, since } => write!(
                f,
                "this document needs `{kind}`, which arrives at {since}. It is refused \
                 rather than skipped: rendering it without would produce a picture that \
                 looks plausible and is wrong"
            ),
            RenderError::Readback(e) => write!(f, "could not read the result back: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// The output of a render: encoded values in the document's output space.
///
/// Not an [`Image`], which is linear working-space by definition. Stage 13 has already
/// run by this point, so these values have been through the gamut map and the transfer
/// curve and are ready to be quantised into a file.
#[derive(Clone, PartialEq)]
pub struct Rendered {
    width: u32,
    height: u32,
    pixels: Vec<f32>,
    pub colorspace: ColorSpace,
}

impl std::fmt::Debug for Rendered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rendered {}×{} {:?}", self.width, self.height, self.colorspace)
    }
}

impl Rendered {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixel(&self, x: u32, y: u32) -> [f32; 3] {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2]]
    }

    /// The space these values are encoded in, as a [`Space`].
    pub fn space(&self) -> &'static Space {
        match self.colorspace {
            ColorSpace::Srgb => &SRGB,
            ColorSpace::DisplayP3 => &DISPLAY_P3,
        }
    }

    /// Quantise to 8 bits, which is what a JPEG or PNG holds.
    ///
    /// Round to nearest and clamp. Dither is a separate concern and would only help,
    /// so leaving it out keeps any measurement of this pessimistic — the same
    /// reasoning `working.rs` uses for its own quantiser.
    pub fn to_u8(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
            .collect()
    }
}

/// The uniform block for `adjust.wgsl`. Field order and padding match the WGSL.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
struct AdjustUniform {
    temperature: f32,
    tint: f32,
    exposure: f32,
    highlights: f32,
    shadows: f32,
    blacks: f32,
    contrast: f32,
    vibrance: f32,
    saturation: f32,
    _pad: f32,
    _tail: [f32; 2],
}

impl AdjustUniform {
    /// Straight from the document's parameters, with an omitted key meaning identity —
    /// which for every one of these is zero, and is why `unwrap_or(0.0)` is the whole
    /// of the mapping rather than a table of per-control defaults.
    fn from(params: &Params) -> Self {
        let Params::AdjustV1(p) = params;
        Self {
            temperature: p.temperature.unwrap_or(0.0),
            tint: p.tint.unwrap_or(0.0),
            exposure: p.exposure.unwrap_or(0.0),
            highlights: p.highlights.unwrap_or(0.0),
            shadows: p.shadows.unwrap_or(0.0),
            blacks: p.blacks.unwrap_or(0.0),
            contrast: p.contrast.unwrap_or(0.0),
            vibrance: p.vibrance.unwrap_or(0.0),
            saturation: p.saturation.unwrap_or(0.0),
            ..Default::default()
        }
    }

    fn bytes(&self) -> &[u8] {
        // Safe: `#[repr(C)]`, all fields `f32`, no padding bytes left uninitialised
        // because `_pad` and `_tail` are real fields.
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self) as *const u8,
                std::mem::size_of::<Self>(),
            )
        }
    }
}

/// The uniform block for `encode.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct EncodeUniform {
    to_dst: [[f32; 4]; 3],
    luma: [f32; 4],
    params: [f32; 4],
}

impl EncodeUniform {
    fn new(dst: &Space, policy: GamutPolicy) -> Self {
        let m = LINEAR_P3.linear_to(dst).0;
        let w = luma_weights(dst);
        // WGSL matrices are column-major; ours is row-major, so this transposes.
        let mut to_dst = [[0.0f32; 4]; 3];
        for (col, cell) in to_dst.iter_mut().enumerate() {
            for (row, slot) in cell.iter_mut().take(3).enumerate() {
                *slot = m[row][col] as f32;
            }
        }
        let params = match policy {
            GamutPolicy::CompressLuma { knee } => [1.0, knee, 0.0, 0.0],
            _ => [0.0, 0.0, 0.0, 0.0],
        };
        Self {
            to_dst,
            luma: [w[0] as f32, w[1] as f32, w[2] as f32, 0.0],
            params,
        }
    }

    fn bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self) as *const u8,
                std::mem::size_of::<Self>(),
            )
        }
    }
}

/// A GPU device and the pipelines the graph's node kinds need.
///
/// Built once and reused: creating a device per render would put adapter enumeration
/// on the slider-drag path, and §7.3 budgets 16 ms for the whole frame.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adjust: wgpu::RenderPipeline,
    encode: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    pub adapter: String,
}

impl Renderer {
    pub fn new() -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            ..Default::default()
        }))
        .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let name = adapter.get_info().name;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("photodesk"),
            required_features: wgpu::Features::empty(),
            // The limits WebGL2 guarantees, so a graph that renders here renders in the
            // preview too. Asking for more would let the exporter succeed on something
            // the webview refuses, which is the WYSIWYG drift §0 freezes against
            // arriving through the back door.
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;

        let pipeline = |label: &str, source: &str| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        Ok(Self {
            adjust: pipeline("adjust", ADJUST_WGSL),
            encode: pipeline("encode", ENCODE_WGSL),
            // Nearest: every pass here is a one-to-one map from a texel to a fragment,
            // so any filtering would be interpolation nobody asked for.
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            }),
            device,
            queue,
            adapter: name,
        })
    }

    /// Render `graph` over `source`.
    pub fn render(&self, graph: &Graph, source: &Image) -> Result<Rendered, RenderError> {
        let (w, h) = (source.width(), source.height());

        // How many nodes still need each node's result. A texture is dropped the
        // moment this reaches zero, which is what keeps §7.3's 512 MB plausible.
        let mut consumers: HashMap<NodeId, usize> = HashMap::new();
        for node in graph.nodes() {
            for input in &node.inputs {
                *consumers.entry(*input).or_insert(0) += 1;
            }
        }
        // The output is consumed by the reader rather than by a node.
        *consumers.entry(graph.output()).or_insert(0) += 1;

        let mut live: HashMap<NodeId, wgpu::Texture> = HashMap::new();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("render") });

        for (index, node) in graph.nodes().iter().enumerate() {
            let id = NodeId(index);
            let texture = match &node.kind {
                NodeKind::Source => self.upload(source),
                NodeKind::Adjust(params) => {
                    let input = &live[&node.inputs[0]];
                    self.pass(&mut encoder, &self.adjust, input, AdjustUniform::from(params).bytes(), w, h)
                }
                NodeKind::Encode { colorspace } => {
                    let dst = match colorspace {
                        ColorSpace::Srgb => &SRGB,
                        ColorSpace::DisplayP3 => &DISPLAY_P3,
                    };
                    let input = &live[&node.inputs[0]];
                    let uniform = EncodeUniform::new(dst, EXPORT_GAMUT_POLICY);
                    self.pass(&mut encoder, &self.encode, input, uniform.bytes(), w, h)
                }
                // Everything below is a real node kind the compiler can emit and the
                // renderer cannot yet run. Refused by name rather than skipped.
                NodeKind::Geometry(_) => {
                    return Err(RenderError::Unimplemented { kind: "geometry", since: "v0.2" });
                }
                NodeKind::Composite { .. } => {
                    return Err(RenderError::Unimplemented {
                        kind: "layer opacity and masks",
                        since: "v0.4",
                    });
                }
                NodeKind::MaskShape(_)
                | NodeKind::MaskFeather { .. }
                | NodeKind::MaskInvert
                | NodeKind::MaskCompose { .. } => {
                    return Err(RenderError::Unimplemented { kind: "masks", since: "v0.4" });
                }
            };
            live.insert(id, texture);

            // Release what nothing else will read.
            for input in &node.inputs {
                let remaining = consumers.get_mut(input).expect("a consumed node was counted");
                *remaining -= 1;
                if *remaining == 0 {
                    live.remove(input);
                }
            }
        }

        let output = live.remove(&graph.output()).expect("the output node ran");
        self.read_back(encoder, &output, w, h, graph)
    }

    fn upload(&self, source: &Image) -> wgpu::Texture {
        // RGBA because a three-channel float texture is not a format WebGL2
        // guarantees, and §12.2 compares against a path that has to run there.
        let mut rgba = vec![f16::ZERO; (source.width() * source.height() * 4) as usize];
        for (i, chunk) in source.pixels().chunks_exact(3).enumerate() {
            rgba[i * 4] = chunk[0];
            rgba[i * 4 + 1] = chunk[1];
            rgba[i * 4 + 2] = chunk[2];
            rgba[i * 4 + 3] = f16::ONE;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("source"),
            size: wgpu::Extent3d {
                width: source.width(),
                height: source.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(rgba.as_ptr() as *const u8, rgba.len() * 2)
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(source.width() * 8),
                rows_per_image: Some(source.height()),
            },
            wgpu::Extent3d {
                width: source.width(),
                height: source.height(),
                depth_or_array_layers: 1,
            },
        );
        texture
    }

    fn pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        input: &wgpu::Texture,
        uniform: &[u8],
        width: u32,
        height: u32,
    ) -> wgpu::Texture {
        use wgpu::util::DeviceExt;
        let ubo = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("params"),
                contents: uniform,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pass"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubo.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &input.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let view = target.create_view(&Default::default());
        let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        render.set_pipeline(pipeline);
        render.set_bind_group(0, &bind_group, &[]);
        render.draw(0..3, 0..1);
        drop(render);
        target
    }

    fn read_back(
        &self,
        mut encoder: wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
        graph: &Graph,
    ) -> Result<Rendered, RenderError> {
        // Copies out of a texture need rows aligned to 256 bytes, so the buffer is
        // padded and the padding is dropped on the way into the result.
        let unpadded = width * 8;
        let padded = unpadded.div_ceil(256) * 256;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::Readback(format!("{e:?}")))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| RenderError::Readback(format!("{e:?}")))?;

        let mut pixels = vec![0.0f32; (width * height * 3) as usize];
        for y in 0..height as usize {
            let row = y * padded as usize;
            for x in 0..width as usize {
                let at = row + x * 8;
                for c in 0..3 {
                    let raw = u16::from_le_bytes([data[at + c * 2], data[at + c * 2 + 1]]);
                    pixels[(y * width as usize + x) * 3 + c] = f16::from_bits(raw).to_f32();
                }
            }
        }
        drop(data);
        readback.unmap();

        let colorspace = match &graph.node(graph.output()).kind {
            NodeKind::Encode { colorspace } => *colorspace,
            // The compiler always ends a graph with an encode; this is the assertion
            // rather than an assumption about it.
            other => {
                return Err(RenderError::Readback(format!(
                    "the graph's output is `{}`, not an encode",
                    other.tag()
                )));
            }
        };
        Ok(Rendered { width, height, pixels, colorspace })
    }
}
