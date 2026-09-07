/**
 * §12.2, across the boundary §0's one-shader-source invariant is actually about.
 *
 * This page runs **the front end's own modules** — `src/graph/execute.ts` and
 * `src/canvas/gl.ts`, the same ones `main.ts` imports — over inputs Rust emitted, and
 * compares the result against wgpu's render of the same plan. Not a harness that looks
 * like the front end: the front end, bundled by the same Vite that builds the app.
 *
 * A twin would make the exercise circular, which is the same reason the older probe
 * loads generated GLSL rather than hand-written GLSL.
 *
 * **Two plans, not one.** The preview blits the edited document and — for §11's
 * hold-for-original — the same document with an empty stack, and those are two passes
 * and one. Measuring only the first is how v0.1 shipped presenting every unedited
 * photograph upside down while this file reported PASS.
 */

import { Program, context, displayTarget, target, texture } from "../../../src/canvas/gl";
import { execute } from "../../../src/graph/execute";
import type { Graph } from "../../../src/document/generated/graph";

const N = 64;

interface Score {
  max: number;
  mean: number;
  over1: number;
}

async function run(): Promise<Record<string, unknown>> {
  const canvas = document.createElement("canvas");
  canvas.width = N;
  canvas.height = N;
  document.body.append(canvas);

  const gl = context(canvas);

  const text = (name: string) => fetch(`generated/${name}`).then((r) => r.text());
  const buffer = (name: string) => fetch(`generated/${name}`).then((r) => r.arrayBuffer());

  const [av, af, ev, ef] = await Promise.all([
    text("plan-adjust.vert"),
    text("plan-adjust.frag"),
    text("plan-encode.vert"),
    text("plan-encode.frag"),
  ]);
  const [sourceBytes, encodeBytes, referenceBytes, plainReferenceBytes] = await Promise.all([
    buffer("plan-source.f16"),
    buffer("plan-encode.bin"),
    buffer("plan-reference.rgb8"),
    buffer("plan-plain-reference.rgb8"),
  ]);
  const [graph, plainGraph] = (await Promise.all([
    fetch("generated/plan-graph.json").then((r) => r.json()),
    fetch("generated/plan-plain-graph.json").then((r) => r.json()),
  ])) as [Graph, Graph];

  const programs = {
    adjust: new Program(gl, av, af, "adjust"),
    encode: new Program(gl, ev, ef, "encode"),
  };

  // The check that catches a rename before it catches a wrong picture: the front end
  // addresses these by name, so a field the linked program does not have is a slider
  // that silently does nothing.
  const fields = [
    "temperature", "tint", "exposure", "highlights", "shadows",
    "blacks", "contrast", "vibrance", "saturation",
  ];
  const missing = fields.filter((f) => !programs.adjust.has(f));

  const source = texture(gl, N, N, new Uint16Array(sourceBytes));
  const targets: [ReturnType<typeof target>, ReturnType<typeof target>] = [
    target(gl, N, N),
    target(gl, N, N),
  ];
  const encodeUniform = new Float32Array(encodeBytes);

  const display = displayTarget(gl, N, N);

  // GL reads a framebuffer bottom-up and the shaders are authored for wgpu's top-left
  // origin, so the two could differ by a vertical flip that has nothing to do with the
  // maths. Score both and report which matched, rather than picking one and calling a
  // convention mismatch an error.
  const score = (got: Uint8Array, reference: Uint8Array, flip: boolean): Score => {
    let max = 0;
    let sum = 0;
    let over1 = 0;
    for (let y = 0; y < N; y++) {
      const ry = flip ? N - 1 - y : y;
      for (let x = 0; x < N; x++) {
        for (let c = 0; c < 3; c++) {
          const mine = got[(y * N + x) * 4 + c] as number;
          const theirs = reference[(ry * N + x) * 3 + c] as number;
          const d = Math.abs(mine - theirs);
          if (d > max) max = d;
          if (d > 1) over1++;
          sum += d;
        }
      }
    }
    return { max, mean: sum / (N * N * 3), over1 };
  };

  // Both of the things the preview actually blits, because they do not have the same
  // number of passes and an orientation rule derived from that count is right for one
  // and wrong for the other.
  //
  //   "direct" means `readPixels` from the final target matched a top-down reference:
  //   the image's top row sits at v = 0. The default framebuffer's y = 0 is the
  //   canvas's **bottom**, so presenting that texture upright requires the blit to read
  //   it backwards — a needed blit flip corresponds to a *direct* readback, not to a
  //   flipped one. Getting that backwards was made once already while writing this
  //   file, and once again in `preview.ts`, which is why both cases are now measured
  //   rather than one being assumed to imply the other.
  const cases = [
    { name: "edited", graph, reference: new Uint8Array(referenceBytes) },
    { name: "no edits (§11's Space)", graph: plainGraph, reference: new Uint8Array(plainReferenceBytes) },
  ].map(({ name, graph, reference }) => {
    const { output, passes } = execute(
      { gl, programs, source, targets, display, encodeUniform },
      graph,
    );
    // 8-bit out of an 8-bit target, so this compares two quantised images rather than a
    // float against a rounding of it — the same bytes `Rendered::to_u8` produces.
    const got = new Uint8Array(N * N * 4);
    gl.bindFramebuffer(gl.FRAMEBUFFER, output.framebuffer);
    gl.readPixels(0, 0, N, N, gl.RGBA, gl.UNSIGNED_BYTE, got);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);

    const direct = score(got, reference, false);
    const flipped = score(got, reference, true);
    return {
      name,
      passes,
      orientation: flipped.max < direct.max ? "flipped" : "direct",
      direct: { max: direct.max, mean: +direct.mean.toPrecision(4), over1: direct.over1 },
      flipped: { max: flipped.max, mean: +flipped.mean.toPrecision(4), over1: flipped.over1 },
    };
  });

  return { ok: true, missingUniforms: missing, cases };
}

run()
  .then((result) => post(result))
  .catch((e: unknown) =>
    post({ ok: false, error: e instanceof Error ? `${e.message}` : String(e) }),
  );

function post(result: Record<string, unknown>): void {
  document.title = "done";
  void fetch("/result", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(result),
  });
}
