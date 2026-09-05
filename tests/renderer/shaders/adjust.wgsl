// §5 stages 2–9, fused into one pass.
//
// This shader is the Spike C experiment, not a draft of the real one. It exists to
// answer whether §7.2's "author once in WGSL, transpile to GLSL ES 3.0 with naga"
// survives contact with a *realistic* per-pixel chain — so it deliberately includes
// the constructs most likely to break a GLSL ES 300 backend rather than the ones
// most likely to work: a large uniform block, fixed-size arrays inside it, dynamic
// indexing, and a loop with a data-dependent bound.
//
// Fragment-first by construction, per §2.3:
//   @fragment, not @compute          — GLSL ES 3.0 has no compute stage
//   var<uniform>, not var<storage>   — GLSL ES 3.0 has no storage buffers
//   texture_2d + sampler             — GLSL ES 3.0 has no storage textures
//   @location(0) return              — a colour attachment, not textureStore
//
// §7.3 requires stages 2–9 fused into a single pass, and §5 makes the stack the loop,
// so one dispatch carries exactly one layer's parameters. That is what keeps this
// block small enough for a UBO where RapidRAW's 32-slot mask array needed a storage
// buffer (see FORK-AUDIT.md).

const CURVE_POINTS: u32 = 16u;
const HSL_BANDS: u32 = 8u;

struct GradeWheel {
    // xyz = lift/gamma/gain style offset, w = luminance weight for the band.
    offset: vec4<f32>,
}

struct Adjustments {
    // stage 2 — white balance
    temperature: f32,
    tint: f32,
    // stage 3 — exposure
    exposure: f32,
    // stage 4 — highlights / shadows / blacks
    highlights: f32,
    shadows: f32,
    blacks: f32,
    // stage 5 — contrast
    contrast: f32,
    // stage 9 — vibrance / saturation
    vibrance: f32,
    saturation: f32,

    // stage 6 — tone curve: luma, then R, G, B. Two points packed per vec4 so the
    // std140 array stride is not four times what the data needs.
    luma_curve: array<vec4<f32>, 8>,
    red_curve: array<vec4<f32>, 8>,
    green_curve: array<vec4<f32>, 8>,
    blue_curve: array<vec4<f32>, 8>,
    curve_counts: vec4<u32>,

    // stage 7 — HSL, eight bands of (hue, saturation, luminance)
    hsl: array<vec4<f32>, 8>,

    // stage 8 — colour grading
    grade_shadows: GradeWheel,
    grade_midtones: GradeWheel,
    grade_highlights: GradeWheel,
    grade_global: GradeWheel,
    grade_blend: f32,
    grade_balance: f32,
}

@group(0) @binding(0) var<uniform> adj: Adjustments;
@group(0) @binding(1) var src_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;

const LUMA_COEFF = vec3<f32>(0.2126, 0.7152, 0.0722);

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, LUMA_COEFF);
}

// --- stage 6 support: cubic hermite through the control points ---------------

fn curve_point(curve: array<vec4<f32>, 8>, i: u32) -> vec2<f32> {
    let packed = curve[i / 2u];
    if (i % 2u == 0u) {
        return packed.xy;
    }
    return packed.zw;
}

fn apply_curve(v: f32, curve: array<vec4<f32>, 8>, count: u32) -> f32 {
    if (count < 2u) {
        return v;
    }
    var lo = curve_point(curve, 0u);
    if (v <= lo.x) {
        return lo.y;
    }
    // Data-dependent loop bound over a dynamically indexed array — the pair of
    // constructs a GLSL ES 300 backend is most likely to refuse.
    for (var i = 1u; i < count; i = i + 1u) {
        let hi = curve_point(curve, i);
        if (v <= hi.x) {
            let span = max(hi.x - lo.x, 1e-5);
            let t = clamp((v - lo.x) / span, 0.0, 1.0);
            let t2 = t * t;
            let t3 = t2 * t;
            // Catmull-Rom-ish tangents, finite-differenced from the neighbours.
            let m0 = (hi.y - lo.y) / span;
            let m1 = m0;
            return (2.0 * t3 - 3.0 * t2 + 1.0) * lo.y
                 + (t3 - 2.0 * t2 + t) * span * m0
                 + (-2.0 * t3 + 3.0 * t2) * hi.y
                 + (t3 - t2) * span * m1;
        }
        lo = hi;
    }
    return lo.y;
}

// --- stage 7 support ---------------------------------------------------------

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let cmax = max(c.r, max(c.g, c.b));
    let cmin = min(c.r, min(c.g, c.b));
    let d = cmax - cmin;
    var h = 0.0;
    if (d > 1e-6) {
        if (cmax == c.r) {
            h = 60.0 * (((c.g - c.b) / d) % 6.0);
        } else if (cmax == c.g) {
            h = 60.0 * (((c.b - c.r) / d) + 2.0);
        } else {
            h = 60.0 * (((c.r - c.g) / d) + 4.0);
        }
    }
    if (h < 0.0) { h = h + 360.0; }
    let s = select(0.0, d / cmax, cmax > 1e-6);
    return vec3<f32>(h, s, cmax);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let h = c.x / 60.0;
    let s = c.y;
    let v = c.z;
    let i = floor(h);
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let idx = i32(i) % 6;
    switch idx {
        case 0: { return vec3<f32>(v, t, p); }
        case 1: { return vec3<f32>(q, v, p); }
        case 2: { return vec3<f32>(p, v, t); }
        case 3: { return vec3<f32>(p, q, v); }
        case 4: { return vec3<f32>(t, p, v); }
        default: { return vec3<f32>(v, p, q); }
    }
}

fn band_influence(hue: f32, centre: f32, width: f32) -> f32 {
    var d = abs(hue - centre);
    d = min(d, 360.0 - d);
    return 1.0 - smoothstep(width * 0.5, width, d);
}

// --- the pass ----------------------------------------------------------------

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// A full-screen triangle. No vertex buffer: the positions come from the index,
// which is one fewer resource to bind and one fewer thing to get wrong.
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

    // stage 2 — white balance, as a channel gain in linear light.
    let temp = adj.temperature / 100.0;
    let tint = adj.tint / 100.0;
    c = c * vec3<f32>(1.0 + temp, 1.0 + tint * 0.5, 1.0 - temp);

    // stage 3 — exposure, in stops.
    c = c * exp2(adj.exposure);

    // stage 4 — highlights / shadows / blacks, weighted by luminance.
    let l = luma(max(c, vec3<f32>(0.0)));
    let hi_w = smoothstep(0.5, 1.0, l);
    let lo_w = 1.0 - smoothstep(0.0, 0.5, l);
    let blk_w = 1.0 - smoothstep(0.0, 0.2, l);
    c = c * (1.0 + adj.highlights * 0.01 * hi_w
                + adj.shadows * 0.01 * lo_w
                + adj.blacks * 0.01 * blk_w);

    // stage 5 — contrast about mid grey.
    let pivot = 0.1845;
    c = (c - pivot) * (1.0 + adj.contrast * 0.01) + pivot;

    // stage 6 — tone curve: luma first, then per channel.
    let before = luma(max(c, vec3<f32>(0.0)));
    let after = apply_curve(before, adj.luma_curve, adj.curve_counts.x);
    c = c * select(1.0, after / before, before > 1e-5);
    c = vec3<f32>(
        apply_curve(c.r, adj.red_curve, adj.curve_counts.y),
        apply_curve(c.g, adj.green_curve, adj.curve_counts.z),
        apply_curve(c.b, adj.blue_curve, adj.curve_counts.w),
    );

    // stage 7 — HSL across eight bands.
    var hsv = rgb_to_hsv(max(c, vec3<f32>(0.0)));
    var h_shift = 0.0;
    var s_scale = 1.0;
    var v_scale = 1.0;
    for (var b = 0u; b < HSL_BANDS; b = b + 1u) {
        let centre = f32(b) * 45.0;
        let w = band_influence(hsv.x, centre, 90.0);
        let band = adj.hsl[b];
        h_shift = h_shift + band.x * w;
        s_scale = s_scale + band.y * 0.01 * w;
        v_scale = v_scale + band.z * 0.01 * w;
    }
    hsv.x = (hsv.x + h_shift + 360.0) % 360.0;
    hsv.y = clamp(hsv.y * s_scale, 0.0, 1.0);
    hsv.z = hsv.z * v_scale;
    c = hsv_to_rgb(hsv);

    // stage 8 — colour grading, three tonal ranges plus a global wheel.
    let gl = clamp(luma(max(c, vec3<f32>(0.0))), 0.0, 1.0);
    let bal = adj.grade_balance * 0.01;
    let w_sh = 1.0 - smoothstep(0.0, 0.5 + bal, gl);
    let w_hi = smoothstep(0.5 + bal, 1.0, gl);
    let w_mid = max(0.0, 1.0 - w_sh - w_hi);
    let graded = c
        + adj.grade_shadows.offset.rgb * w_sh
        + adj.grade_midtones.offset.rgb * w_mid
        + adj.grade_highlights.offset.rgb * w_hi
        + adj.grade_global.offset.rgb;
    c = mix(c, graded, adj.grade_blend);

    // stage 9 — vibrance then saturation.
    let g = luma(max(c, vec3<f32>(0.0)));
    let sat_now = length(c - vec3<f32>(g));
    let vib = adj.vibrance * 0.01 * (1.0 - clamp(sat_now, 0.0, 1.0));
    c = mix(vec3<f32>(g), c, 1.0 + vib);
    c = mix(vec3<f32>(luma(max(c, vec3<f32>(0.0)))), c, 1.0 + adj.saturation * 0.01);

    return vec4<f32>(c, texel.a);
}
