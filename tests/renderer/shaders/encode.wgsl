// §5 stage 13 — display or export encode, with §16 #11's gamut policy inside it.
//
// Stage 13 is the one stage that runs on *every* pixel of *both* paths: the preview
// writes it to the canvas and the exporter writes it to a file, and §0 freezes those
// to one shader source. That is why the gamut policy had to be chosen as something a
// fragment shader can do — see `tests/color/src/gamut.rs`, which is this file's
// reference implementation and the thing `encode_stage.rs` checks it against.
//
// Fragment-first per the frozen register item:
//   @fragment · var<uniform> · texture_2d + sampler · @location(0) return.
// No compute, no storage buffer, no storage texture — naga refuses all three when
// targeting GLSL ES 3.00, so one of them here would break the preview path.

struct Encode {
    // Linear working space (P3) -> linear destination RGB. Supplied rather than
    // derived: `tests/color/` already validates matrix construction against lcms2,
    // and this shader is being tested on the policy, not on the matrix.
    to_dst: mat3x3<f32>,
    // xyz = the destination's luminance weights (the Y row of its RGB->XYZ matrix).
    // They sum to one, which is what makes `vec3(grey)` have luminance `grey`.
    luma: vec4<f32>,
    // x: 0 = preserve-luma (the frozen policy), 1 = the compression variant.
    // y: the compression knee.
    //
    // A shipped shader has one policy, selected when the pipeline is built. The
    // uniform is here because the harness has to check both — the compression form
    // is the register's named exit path, and "it would have lowered" is not something
    // to find out on the day it is needed.
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> enc: Encode;
@group(0) @binding(1) var src_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;

// Stands in for "this channel imposes no bound". Not an infinity: GLSL ES 3.00 has no
// literal for one, and at 1e9 the reciprocal is 1e-9, which every branch below already
// treats as deep inside the gamut.
const UNCONSTRAINED: f32 = 1.0e9;
const FLAT: f32 = 1.0e-9;

// The largest multiple of `v` from `vec3(grey)` that stays inside the unit cube —
// which, in linear destination RGB, is exactly the destination gamut.
//
// Branchless because every channel has to be considered and only the smallest bound
// matters. The divisor is forced away from zero first: a channel that is not moving
// imposes no bound, and 0/0 would otherwise put a NaN into a `select` that is about
// to discard it anyway.
fn ray_t_max(grey: f32, v: vec3<f32>) -> f32 {
    let flat = abs(v) < vec3<f32>(FLAT);
    let safe = select(v, vec3<f32>(1.0), flat);
    let hi = vec3<f32>(1.0 - grey) / safe;   // where the channel meets white
    let lo = vec3<f32>(-grey) / safe;        // where it meets black
    let bound = select(
        select(vec3<f32>(UNCONSTRAINED), lo, v < vec3<f32>(-FLAT)),
        hi,
        v > vec3<f32>(FLAT),
    );
    return max(min(bound.x, min(bound.y, bound.z)), 0.0);
}

// §16 #11, frozen: slide along the constant-luminance ray until the colour is exactly
// on the gamut boundary, and no further.
//
// The vector being scaled carries zero luminance — its components are `c - grey` and
// the weights sum to one — so scaling it is a pure chroma operation and L* comes
// through untouched. That is the property the policy was chosen for.
fn preserve_luma(c: vec3<f32>) -> vec3<f32> {
    let grey = clamp(dot(c, enc.luma.xyz), 0.0, 1.0);
    let v = c - vec3<f32>(grey);
    let t_max = ray_t_max(grey, v);
    // The sample sits at t = 1, so this is the in-gamut test. Returned untouched
    // rather than recomputed, because "in-gamut content does not move" is exact.
    if (t_max >= 1.0) {
        return c;
    }
    return clamp(vec3<f32>(grey) + v * t_max, vec3<f32>(0.0), vec3<f32>(1.0));
}

// The variant the register names as this policy's exit path: the same ray, with a
// rational rolloff so out-of-gamut colours land *inside* the boundary and stay
// distinguishable from one another. Measured and not chosen — it buys gradient
// survival by moving in-gamut colours that were already correct.
fn compress_luma(c: vec3<f32>, knee: f32) -> vec3<f32> {
    let grey = clamp(dot(c, enc.luma.xyz), 0.0, 1.0);
    let v = c - vec3<f32>(grey);
    let t_max = ray_t_max(grey, v);
    // Distance to the sample in units of the boundary. `t_max` is floored because a
    // colour at the destination's white luminance has a zero-length ray.
    let d = 1.0 / max(t_max, FLAT);
    if (d <= knee) {
        return c;
    }
    let span = 1.0 - knee;
    let u = d - knee;
    let mapped = knee + u * span / (span + u);
    return clamp(vec3<f32>(grey) + v * (t_max * mapped), vec3<f32>(0.0), vec3<f32>(1.0));
}

// The sRGB transfer curve. No odd extension for negative inputs, unlike the harness's
// `Transfer::from_linear`: that exists so a *measurement* can carry an unmapped colour
// through, and by this point the gamut map has already put the value in [0,1].
fn encode_srgb(v: vec3<f32>) -> vec3<f32> {
    let lo = v * 12.92;
    let hi = 1.055 * pow(max(v, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, v <= vec3<f32>(0.0031308));
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// The same full-screen triangle `adjust.wgsl` uses: positions from the vertex index,
// so there is no vertex buffer to bind and none to get wrong.
@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOut {
    var out: VertexOut;
    let x = f32(i32(idx) / 2) * 4.0 - 1.0;
    let y = f32(i32(idx) % 2) * 4.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, 1.0 - (y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(src_texture, src_sampler, in.uv);
    // Working space -> linear destination. §4's order: matrix, then gamut, then curve.
    let dst_linear = enc.to_dst * texel.rgb;
    var mapped = preserve_luma(dst_linear);
    if (enc.params.x > 0.5) {
        mapped = compress_luma(dst_linear, enc.params.y);
    }
    return vec4<f32>(encode_srgb(mapped), texel.a);
}
