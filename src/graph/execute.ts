/**
 * Executes a compiled plan. It does not build one.
 *
 * §0's register: *the graph is compiled once, in Rust; both renderers execute the same
 * plan.* Two compilers would render two topologies and drift exactly as two shader
 * sources would, with §12.2 then comparing two **compilations** rather than two
 * executions of one plan — the one-shader-source invariant enforced over the shader
 * while the graph above it went unchecked. So this file walks `Graph.nodes` and issues
 * draws, and there is nothing in it that decides what the graph should contain.
 *
 * §6.2: "The graph has **no UI**. Ever." Nothing here touches the DOM.
 */

import type { Graph, Node, NodeId } from "../document/generated/graph";
import { type Program, type Target } from "../canvas/gl";

/** The nine scalars `adjust.wgsl`'s uniform block holds, in the document's own names. */
const ADJUST_FIELDS = [
  "temperature",
  "tint",
  "exposure",
  "highlights",
  "shadows",
  "blacks",
  "contrast",
  "vibrance",
  "saturation",
] as const;

export interface Programs {
  adjust: Program;
  encode: Program;
}

export interface Resources {
  gl: WebGL2RenderingContext;
  programs: Programs;
  /** The decoded photograph, linear P3 — `NodeKind::source`. */
  source: WebGLTexture;
  /** Two ping-pong targets at proxy size, RGBA16F — the working space (§2.2). */
  targets: [Target, Target];
  /**
   * Where the plan's **output** node draws: 8-bit, because stage 13 has encoded for the
   * display by then. The caller supplies it rather than this file allocating one,
   * because the preview keeps two — the edited frame and the unedited one it is
   * compared against — and they outlive any single execution.
   */
  display: Target;
  /** Stage 13's derived constants, from Rust (`encode_uniform`). */
  encodeUniform: Float32Array<ArrayBufferLike>;
}

export class UnsupportedNode extends Error {
  constructor(op: string) {
    // Named rather than skipped. A node this cannot draw is a picture that is silently
    // wrong, and §14 defers masks and crop by version rather than by accident — so the
    // ones that arrive at v0.2 and v0.4 say so here until they are implemented.
    super(
      `the preview cannot execute a \`${op}\` node yet. §14 puts crop at v0.2 and masks ` +
        `at v0.4; until then a document containing one renders nothing rather than ` +
        `something that looks plausible.`,
    );
  }
}

/**
 * Draw the plan into a target, and say how many passes it took.
 *
 * The pass count is the caller's business because of the orientation problem: the
 * shaders are authored for wgpu's top-left framebuffer origin and GL's is bottom-left,
 * so every pass inverts the image. Rather than have this file know about screens, it
 * reports the parity and `preview.ts` resolves it at the blit.
 */
export function execute(res: Resources, graph: Graph): { output: Target; passes: number } {
  const { gl } = res;
  let passes = 0;
  // Which target holds each node's result. `source` is not a target, so it is tracked
  // separately — the plan's first node is always the photograph itself.
  const produced = new Map<NodeId, WebGLTexture>();

  for (let id = 0; id < graph.nodes.length; id++) {
    const node = graph.nodes[id] as Node;
    const kind = node.kind;

    if (kind.op === "source") {
      produced.set(id, res.source);
      continue;
    }

    const inputId = node.inputs[0];
    if (inputId === undefined) {
      throw new Error(`node ${id} (${kind.op}) has no input, which a compiled graph cannot`);
    }
    const input = produced.get(inputId);
    if (input === undefined) {
      throw new Error(
        `node ${id} reads node ${inputId}, which has not run — the plan is not in ` +
          `topological order, and §6.2 says it always is`,
      );
    }

    const program = res.programs[programFor(kind.op)];
    program.clear();

    if (kind.op === "adjust") {
      // Straight from the node, with an omitted key meaning identity — which for every
      // one of these is zero, and is why `?? 0` is the whole of the mapping. The same
      // sentence is in `render.rs`, because it is the same mapping and it is trivial;
      // anything less trivial would have to cross the boundary rather than be retyped.
      const params = kind as unknown as Record<string, number | null | undefined>;
      for (const field of ADJUST_FIELDS) program.set(field, params[field] ?? 0);
    } else if (kind.op === "encode") {
      program.setBlock(res.encodeUniform);
    } else {
      throw new UnsupportedNode(kind.op);
    }

    // The last node's result is presented rather than sampled again, so it goes to the
    // 8-bit target. Everything before it stays in the f16 working space.
    const dst = id === graph.output ? res.display : (res.targets[passes % 2] as Target);
    program.use();
    gl.bindFramebuffer(gl.FRAMEBUFFER, dst.framebuffer);
    gl.viewport(0, 0, dst.width, dst.height);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, input);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    produced.set(id, dst.texture);
    passes++;
  }

  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  if (passes === 0) {
    throw new Error("the plan drew nothing, so there is no frame to present");
  }
  return { output: res.display, passes };
}

function programFor(op: string): keyof Programs {
  return op === "encode" ? "encode" : "adjust";
}
