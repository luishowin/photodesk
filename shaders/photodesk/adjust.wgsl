// §5 stages 2–9 for one layer, fused into a single pass.
//
// §7.3 requires the fusion and says why: "fifteen discrete render passes at 2 MP f16
// means ~15 × 32 MB of texture round-trips per frame — roughly half a gigabyte of
// traffic, which no amount of ALU saves you from". So this is one shader doing what
// §5 lists as eight stages, and the stage order inside it *is* §5's order, which is a
// FROZEN register item rather than a convenience.
//
// **Stages 6, 7 and 8 are absent, and that is the document's doing rather than an
// omission.** `adjust` at `op_version: 1` carries the scalar parameters of stages 2–5
// and 9; a tone curve, an HSL band set and a grading wheel are structured rather than
// scalar, they arrive at v0.3, and they come with the version bump they justify. A
// version that does not carry a stage runs identity there, which is what §6.1 already
// says about an omitted key.
//
// Fragment-first per the frozen register item: `@fragment`, `var<uniform>`, a sampled
// texture, `@location(0)` return. No compute, no storage buffer, no storage texture —
// naga refuses all three when targeting GLSL ES 3.00, so one of them here would break
// the preview path for the whole project.
//
// **On the formulations.** Each stage below is the simplest thing that does what the
// control is named for, and several of them are choices rather than facts. §16 #4
// leaves v1's pipeline ordering to golden-image validation, and §12.1 is where these
// get re-litigated with evidence rather than by preference — the same posture §3 takes
// on demosaic ("the library default is correct until a golden-image test says
// otherwise"). What is *not* a matter of taste is the order, and the fact that a
// slider at its default does exactly nothing.

// Linear Display P3's luminance weights: the Y row of its RGB→XYZ matrix.
//
// Written here rather than passed in a uniform because it is a constant of the frozen
// working space, not a parameter — and because a uniform would have to be filled by
// both the exporter and the preview, which is two places to write one number. Tied to
// the derived value by a test (`render.rs`), so this cannot drift from `colour.rs`.
const LUMA = vec3<f32>(0.2289745, 0.6917385, 0.0792869);

// Linear Display P3 ← CIE XYZ, likewise a constant of the working space. Columns are
// the XYZ basis. Used only by stage 2, to say where a white point lands in the space
// the pixels are already in.
const XYZ_TO_P3 = mat3x3<f32>(
    vec3<f32>( 2.4934969, -0.8294890,  0.0358458),
    vec3<f32>(-0.9313836,  1.7626641, -0.0761724),
    vec3<f32>(-0.4027108,  0.0236247,  0.9568845),
);

// The correlated colour temperature of the working space's own white point.
//
// D65 is 6504 K, and stage 2 measures its slider *from here*: a temperature of zero
// has to be exactly the identity, which it is by construction below rather than by
// arithmetic that lands near one.
const REFERENCE_KELVIN: f32 = 6504.0;

struct Adjust {
    // —— §5 stage 2, white balance ——
    // Kelvin, as a delta. Forced by §5's execution model rather than chosen: the stack
    // is the loop and stage 2 runs once per layer, so two layers each declaring an
    // absolute 5200 K would describe nothing.
    temperature: f32,
    // Green–magenta, −100…100, perpendicular to the daylight locus.
    tint: f32,
    // —— stage 3 ——
    // Stops. The one control in the set whose formulation is not a choice.
    exposure: f32,
    // —— stage 4 ——
    highlights: f32,
    shadows: f32,
    blacks: f32,
    // —— stage 5 ——
    contrast: f32,
    // —— stage 9 ——
    vibrance: f32,
    saturation: f32,
    _pad: f32,
}

@group(0) @binding(0) var<uniform> adj: Adjust;
@group(0) @binding(1) var src_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;

fn luma(c: vec3<f32>) -> f32 {
    return dot(max(c, vec3<f32>(0.0)), LUMA);
}

// The sRGB transfer curve, used here only as a *perceptual yardstick* — stage 4 weights
// its three controls by where a pixel sits for the eye, and linear luminance is a poor
// answer to that: 0.5 linear is 73% encoded, which is far brighter than "midtone".
fn encode(v: f32) -> f32 {
    let x = max(v, 0.0);
    if (x <= 0.0031308) {
        return x * 12.92;
    }
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}

// —— stage 2 support ————————————————————————————————————————————————————————

// The CIE daylight locus (CIE 15:2004), as its two published cubics.
//
// A definition rather than a derived value, like the sRGB curve's coefficients and the
// Bradford matrix — which is why it is written down here and why `colour.rs`'s
// no-tabulated-constants rule does not reach it. Valid from 4000 K to 25000 K; the
// clamp below keeps the slider inside that rather than extrapolating a cubic.
fn daylight_xy(kelvin: f32) -> vec2<f32> {
    let t = clamp(kelvin, 4000.0, 25000.0);
    let inv = 1000.0 / t;
    var x: f32;
    if (t <= 7000.0) {
        x = 0.244063 + 0.09911 * inv + 2.9678 * inv * inv - 4.6070 * inv * inv * inv;
    } else {
        x = 0.237040 + 0.24748 * inv + 1.9018 * inv * inv - 2.0064 * inv * inv * inv;
    }
    let y = -3.000 * x * x + 2.870 * x - 0.275;
    return vec2<f32>(x, y);
}

/// A chromaticity as linear working-space RGB, normalised to Y = 1.
fn white_as_rgb(xy: vec2<f32>) -> vec3<f32> {
    let xyz = vec3<f32>(xy.x / xy.y, 1.0, (1.0 - xy.x - xy.y) / xy.y);
    return XYZ_TO_P3 * xyz;
}

// —— the pass ——————————————————————————————————————————————————————————————

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// A full-screen triangle. No vertex buffer: the positions come from the index, which
// is one fewer resource to bind and one fewer thing to get wrong.
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
    var c = texel.rgb;

    // —— stage 2: white balance ————————————————————————————————————————————
    //
    // A von Kries scaling between two daylight white points. The gain is the ratio of
    // the target white to the *locus evaluated at the reference*, not to D65's defined
    // chromaticity — so a temperature of zero is exactly 1.0 per channel rather than
    // within a thousandth of it, and the slider's zero really is the identity.
    //
    // Normalised to preserve luminance, so that warming a photograph does not also
    // brighten it. §11 gives each control one job, and "exposure" is the other one.
    var reference_xy = daylight_xy(REFERENCE_KELVIN);
    var target_xy = daylight_xy(REFERENCE_KELVIN + adj.temperature);
    // Tint moves perpendicular to the locus, which in xy is very nearly the y axis.
    // Positive is green, matching the direction the control is labelled.
    target_xy.y = target_xy.y + adj.tint * 0.0005;
    let reference_rgb = white_as_rgb(reference_xy);
    let target_rgb = white_as_rgb(target_xy);
    var gain = target_rgb / reference_rgb;
    gain = gain / dot(gain, LUMA);
    c = c * gain;

    // —— stage 3: exposure ————————————————————————————————————————————————
    c = c * exp2(adj.exposure);

    // —— stage 4: highlights, shadows, blacks ——————————————————————————————
    //
    // Weighted by where the pixel sits *perceptually* rather than in linear light:
    // 0.5 linear is 73% encoded, so thresholds in linear would put "midtone" up near
    // the highlights and the three controls would fight over the same pixels.
    let tone = encode(luma(c));
    let hi = smoothstep(0.5, 1.0, tone);
    let lo = 1.0 - smoothstep(0.0, 0.5, tone);
    let blk = 1.0 - smoothstep(0.0, 0.2, tone);
    c = c * (1.0
        + adj.highlights * 0.01 * hi
        + adj.shadows * 0.01 * lo
        + adj.blacks * 0.01 * blk);

    // —— stage 5: contrast ————————————————————————————————————————————————
    //
    // About 18% grey, which is the reflectance a meter is calibrated to and therefore
    // the point a photographer expects to stay put. In linear light, so that raising
    // contrast is a gain about a pivot rather than a curve nobody specified.
    let pivot = 0.18;
    c = (c - pivot) * (1.0 + adj.contrast * 0.01) + pivot;

    // —— stages 6, 7, 8: absent at op_version 1 ————————————————————————————
    // Tone curve, HSL and colour grading. Identity here, and the gap is deliberate —
    // §5's numbering is the contract, so the stages keep their positions.

    // —— stage 9: vibrance, then saturation ————————————————————————————————
    //
    // §5's order, and it is not commutative: vibrance backs off where a pixel is
    // already saturated, so applying it after a saturation boost would find a
    // different picture than applying it before.
    let grey = luma(c);
    let neutral = vec3<f32>(grey);
    // How far this pixel already is from neutral, relative to its own brightness — so
    // a saturated shadow counts as saturated rather than as nearly-grey.
    let chroma = length(c - neutral) / max(grey, 0.02);
    let vib = adj.vibrance * 0.01 * (1.0 - clamp(chroma, 0.0, 1.0));
    c = mix(neutral, c, 1.0 + vib);

    let grey2 = luma(c);
    c = mix(vec3<f32>(grey2), c, 1.0 + adj.saturation * 0.01);

    return vec4<f32>(c, texel.a);
}
