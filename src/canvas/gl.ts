/**
 * The WebGL2 primitives the preview needs, and nothing else.
 *
 * Two things here are less obvious than they look, and both are written down because
 * getting either wrong produces a picture rather than an error.
 *
 * **Uniform offsets are read back from the linked program**, never written down. The
 * shaders are lowered from WGSL by naga (§7.2), which renames `@group`/`@binding` into
 * whatever GLSL ES 3.00 can express — `_group_0_binding_0_fs.exposure` today. A table of
 * offsets in TypeScript would be a second description of a layout naga owns, and it
 * would go stale silently: the slider would move and nothing would happen.
 *
 * **A pass does not flip the image, and it is worth knowing why not.** The shaders are
 * authored for wgpu, whose framebuffer origin is top-left, and GL's is bottom-left — so
 * the same `uv` expression would address the opposite end, except that `engine::glsl`
 * lowers with naga's `ADJUST_COORDINATE_SPACE` and that negates `gl_Position.y`. A GL
 * pass therefore writes the row indices wgpu's writes, however many passes there are.
 * Spike C scored both orientations rather than assuming one and reports "direct"
 * (`SPIKE-C.md`); `tests/renderer/` now scores both an odd and an even pass count,
 * because for one long release the even case was the only one measured and the odd one
 * was upside down on screen. What remains is a single flip at the blit — `preview.ts`.
 */

export class GlError extends Error {}

export function context(canvas: HTMLCanvasElement): WebGL2RenderingContext {
  const gl = canvas.getContext("webgl2", {
    antialias: false,
    // The canvas holds display-encoded values by the time it is presented, and asking
    // the compositor to treat them as premultiplied would darken every edge.
    premultipliedAlpha: false,
    preserveDrawingBuffer: false,
  });
  if (!gl) throw new GlError("this webview has no WebGL2 context, so there is nothing to preview with");
  if (!gl.getExtension("EXT_color_buffer_float")) {
    throw new GlError(
      "EXT_color_buffer_float is missing, so RGBA16F cannot be rendered to. The working " +
        "space is linear f16 (§2.2) and an 8-bit intermediate would quantise every stage.",
    );
  }
  return gl;
}

/** A linked program plus the reflection needed to feed it. */
export class Program {
  readonly handle: WebGLProgram;
  private readonly gl: WebGL2RenderingContext;
  private readonly ubo: WebGLBuffer | null;
  private readonly block: ArrayBuffer;
  private readonly offsets = new Map<string, number>();
  private readonly f32: Float32Array;

  constructor(gl: WebGL2RenderingContext, vert: string, frag: string, label: string) {
    this.gl = gl;
    this.handle = link(gl, vert, frag, label);
    gl.useProgram(this.handle);

    const names: string[] = [];
    const active = gl.getProgramParameter(this.handle, gl.ACTIVE_UNIFORMS) as number;
    for (let i = 0; i < active; i++) {
      const info = gl.getActiveUniform(this.handle, i);
      if (info) names.push(info.name);
    }

    const blocks = gl.getProgramParameter(this.handle, gl.ACTIVE_UNIFORM_BLOCKS) as number;
    if (blocks > 0) {
      const size = gl.getActiveUniformBlockParameter(
        this.handle,
        0,
        gl.UNIFORM_BLOCK_DATA_SIZE,
      ) as number;
      gl.uniformBlockBinding(this.handle, 0, 0);
      this.block = new ArrayBuffer(size);
      this.ubo = gl.createBuffer();
      gl.bindBuffer(gl.UNIFORM_BUFFER, this.ubo);
      gl.bufferData(gl.UNIFORM_BUFFER, this.block, gl.DYNAMIC_DRAW);

      // Offsets by field name, from the driver's own reflection of the shader that is
      // about to run. `endsWith` because naga's block member is nested one level.
      for (const name of names) {
        const indices = gl.getUniformIndices(this.handle, [name]);
        if (!indices || indices[0] === undefined || indices[0] === gl.INVALID_INDEX) continue;
        const offset = gl.getActiveUniforms(this.handle, [indices[0]], gl.UNIFORM_OFFSET) as number[];
        if (offset[0] === undefined || offset[0] < 0) continue;
        const field = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1) : name;
        this.offsets.set(field.replace(/\[0\]$/, ""), offset[0]);
      }
    } else {
      this.block = new ArrayBuffer(0);
      this.ubo = null;
    }
    this.f32 = new Float32Array(this.block);

    // The sampler is a plain uniform rather than a block member; texture unit 0 for all
    // of them, because every pass in this pipeline reads exactly one image.
    for (const name of names) {
      if (/sampler|texture/i.test(name) && !this.offsets.has(name)) {
        const location = gl.getUniformLocation(this.handle, name);
        if (location) gl.uniform1i(location, 0);
      }
    }
  }

  /** Whether the shader has this field at all — a rename shows up here, not as a no-op. */
  has(field: string): boolean {
    return this.offsets.has(field);
  }

  set(field: string, value: number): void {
    const at = this.offsets.get(field);
    if (at === undefined) return;
    this.f32[at / 4] = value;
  }

  /** Write a whole block that Rust laid out — stage 13's derived constants. */
  setBlock(values: Float32Array<ArrayBufferLike>): void {
    this.f32.set(values.subarray(0, Math.min(values.length, this.f32.length)));
  }

  /** Bind the program and upload whatever `set` has accumulated. */
  use(): void {
    const gl = this.gl;
    gl.useProgram(this.handle);
    if (this.ubo) {
      gl.bindBuffer(gl.UNIFORM_BUFFER, this.ubo);
      gl.bufferSubData(gl.UNIFORM_BUFFER, 0, this.block);
      gl.bindBufferBase(gl.UNIFORM_BUFFER, 0, this.ubo);
    }
  }

  /** Zero the block, so an omitted parameter is identity rather than the last drag's value. */
  clear(): void {
    this.f32.fill(0);
  }
}

function link(gl: WebGL2RenderingContext, vert: string, frag: string, label: string): WebGLProgram {
  const compile = (type: number, source: string, stage: string) => {
    const shader = gl.createShader(type);
    if (!shader) throw new GlError(`${label}: could not create the ${stage} shader`);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      throw new GlError(`${label} ${stage} did not compile:\n${gl.getShaderInfoLog(shader)}`);
    }
    return shader;
  };
  const program = gl.createProgram();
  if (!program) throw new GlError(`${label}: could not create the program`);
  gl.attachShader(program, compile(gl.VERTEX_SHADER, vert, "vertex"));
  gl.attachShader(program, compile(gl.FRAGMENT_SHADER, frag, "fragment"));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new GlError(`${label} did not link:\n${gl.getProgramInfoLog(program)}`);
  }
  return program;
}

/** A texture and the framebuffer that renders into it. */
export interface Target {
  texture: WebGLTexture;
  framebuffer: WebGLFramebuffer;
  width: number;
  height: number;
}

export function texture(
  gl: WebGL2RenderingContext,
  width: number,
  height: number,
  data: Uint16Array | null = null,
): WebGLTexture {
  const tex = gl.createTexture();
  if (!tex) throw new GlError("out of textures");
  gl.bindTexture(gl.TEXTURE_2D, tex);
  // RGBA16F throughout: §2.2 froze the working space at linear f16, and an 8-bit
  // intermediate would quantise between every stage of a chain built to avoid exactly
  // that. HALF_FLOAT takes the Uint16Array straight from Rust with no conversion.
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16F, width, height, 0, gl.RGBA, gl.HALF_FLOAT, data);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return tex;
}

/**
 * A target for the **end** of the chain: 8-bit, because stage 13 has already encoded for
 * the display and there is nothing left for more bits to carry.
 *
 * Not an optimisation — a requirement. `blitFramebuffer` refuses to copy between a
 * floating-point read buffer and a fixed-point draw buffer (ES 3.0 §4.3.2,
 * `INVALID_OPERATION`), and the canvas is fixed-point. An RGBA16F final target presents
 * as a blank canvas with no error raised anywhere the user can see it, which is how this
 * was found.
 */
export function displayTarget(gl: WebGL2RenderingContext, width: number, height: number): Target {
  const tex = gl.createTexture();
  if (!tex) throw new GlError("out of textures");
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return attach(gl, tex, width, height);
}

export function target(gl: WebGL2RenderingContext, width: number, height: number): Target {
  const tex = texture(gl, width, height);
  return attach(gl, tex, width, height);
}

function attach(
  gl: WebGL2RenderingContext,
  tex: WebGLTexture,
  width: number,
  height: number,
): Target {
  const fbo = gl.createFramebuffer();
  if (!fbo) throw new GlError("out of framebuffers");
  gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
  const status = gl.checkFramebufferStatus(gl.FRAMEBUFFER);
  if (status !== gl.FRAMEBUFFER_COMPLETE) {
    throw new GlError(`a render target is incomplete (0x${status.toString(16)})`);
  }
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  return { texture: tex, framebuffer: fbo, width, height };
}

export function dispose(gl: WebGL2RenderingContext, t: Target): void {
  gl.deleteFramebuffer(t.framebuffer);
  gl.deleteTexture(t.texture);
}
