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
  const [sourceBytes, encodeBytes, referenceBytes] = await Promise.all([
    buffer("plan-source.f16"),
    buffer("plan-encode.bin"),
    buffer("plan-reference.rgb8"),
  ]);
  const graph = (await fetch("generated/plan-graph.json").then((r) => r.json())) as Graph;

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

  const reference = new Uint8Array(referenceBytes);

  // GL reads a framebuffer bottom-up and the shaders are authored for wgpu's top-left
  // origin, so the two can differ by a vertical flip that has nothing to do with the
  // maths. Score both, report which matched, and let the caller check it against the
  // parity `preview.ts` predicts — rather than picking one and calling a convention
  // mismatch an error.
  const score = (flip: boolean): Score => {
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

  const direct = score(false);
  const flipped = score(true);
  const best = flipped.max < direct.max ? "flipped" : "direct";

  return {
    ok: true,
    passes,
    missingUniforms: missing,
    orientation: best,
    // What `preview.ts` will do at the blit, so the two can be checked against each
    // other. The relationship is not the obvious one and is worth stating in full:
    //
    //   "direct" here means `readPixels` from the final target matched a top-down
    //   reference, i.e. after `passes` flips the image's **top row sits at v = 0**.
    //   The default framebuffer's y = 0 is the canvas's **bottom**, so presenting that
    //   texture upright requires the blit to read it backwards.
    //
    // So a needed blit flip corresponds to a *direct* readback, not to a flipped one.
    // Getting that backwards is exactly the mistake this field exists to catch, and it
    // was made once already while writing this file.
    blitFlip: passes % 2 === 0,
    direct: { max: direct.max, mean: +direct.mean.toPrecision(4), over1: direct.over1 },
    flipped: { max: flipped.max, mean: +flipped.mean.toPrecision(4), over1: flipped.over1 },
  };
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
