// The modules the in-tab engine (`engine.rs`) runs a browser variant with, and onnxruntime-web's part: a session over
// the variant's graph and weights, and one decode per prompt row. The files and the stages are the playground's Rust
// code; the plan and the readout are the engine module's (`crates/ardana-engine`, ardana-core's tokenizer, planner and
// readout, which trunk builds into `/engine/` beside this file). Both modules are imported on first use, so a page that
// only runs server models loads neither: onnxruntime-web 1.30.0 is vendored beside the page
// (`crates/ardana-playground/ort/`), and each module fetches its WASM from beside itself. Every path is relative to
// this file, so the page runs wherever it is served (`/` by `ardana serve`, `/playground/` on the landing).

/** The build for each execution provider. The WebGPU build's WASM module has no CPU kernel for the quantized
 * embedding (`GatherBlockQuantized`) the int4 graphs read, so a session on the CPU runs on the plain WASM build. */
const BUNDLES = {
  webgpu: new URL('../ort/1.30.0/ort.webgpu.bundle.min.mjs', import.meta.url).href,
  wasm: new URL('../ort/1.30.0/ort.wasm.bundle.min.mjs', import.meta.url).href,
};

/** The engine module trunk builds from `crates/ardana-engine` beside this file (`index.html`). */
const ENGINE = './ardana-engine.js';

const modules = {};
/** The failed imports of each module so far. */
const failures = {};
/** The row each session is decoding. onnxruntime-web frees a session at once, even while a row of it waits on the GPU
 * (the row then reads freed memory), so a release waits for that row to be over. */
const decoding = new WeakMap();

/** Forgets the module imported as `key`, which did not load: the next import asks for it again under a new query,
 * since a browser may keep a module's failed fetch for its URL. */
function forget(key) {
  delete modules[key];
  failures[key] = (failures[key] ?? 0) + 1;
}

/** The module at `url`, imported once as `key` and started by `start`; a failed import or start is forgotten. */
function load(key, url, start) {
  const retry = failures[key] ? `?retry=${failures[key]}` : '';
  modules[key] ??= import(`${url}${retry}`)
    .then(start)
    .catch((err) => {
      forget(key);
      throw err;
    });
  return modules[key];
}

/** The failure of a session whose build did not load: its WASM module could not be fetched. */
class NotLoaded extends Error {
  name = 'NotLoaded';
}

/** The onnxruntime-web build for `device`; it logs errors only (placing shape operators on the CPU is expected, not
 * worth a warning). */
export function runtime(device) {
  return load(device, BUNDLES[device], (ort) => {
    ort.env.logLevel = 'error';
    return ort;
  });
}

/** The engine module, started: its `init` fetches the module's WASM. */
export function engine() {
  return load('engine', ENGINE, async (module) => {
    await module.default();
    return module;
  });
}

/** Whether the browser offers WebGPU; without `navigator.gpu` a session runs on WASM. */
export function hasWebGpu() {
  return Boolean(globalThis.navigator && navigator.gpu);
}

/** Starts importing the engine module and the build this browser's first session runs on, so Run does not wait for
 * them. A failed import fails the run that needs the module, which says why. */
export function prepare() {
  engine().catch(() => {});
  runtime(hasWebGpu() ? 'webgpu' : 'wasm').catch(() => {});
}

/** A session of `ort` (a build from `runtime`) over `model` (the bytes of `model.onnx`) and `data`
 * (`model.onnx.data`) on `device`, `webgpu` or `wasm`, with the build it runs on. A run stopped by `signal` while the
 * session starts keeps nothing: the session is released as soon as it exists. A build whose WASM module could not be
 * fetched fails with `NotLoaded` and is forgotten: onnxruntime-web keeps a backend's failed start for the life of its
 * module, so the next run imports the build afresh. Any other failure (a model the device cannot run) stays. */
export async function createSession(ort, model, data, device, signal) {
  let session;
  try {
    session = await ort.InferenceSession.create(model, {
      executionProviders: [device],
      externalData: [{ path: 'model.onnx.data', data }],
      logSeverityLevel: 3,
    });
  } catch (err) {
    const message = String(err?.message ?? err);
    // The vendored builds' WASM loader (Emscripten's) says so when the module beside the bundle cannot be fetched.
    if (message.includes('fetching of the wasm failed')) {
      forget(device);
      throw new NotLoaded(message, { cause: err });
    }
    throw err;
  }
  if (signal.aborted) {
    await session.release();
    throw signal.reason;
  }
  return { ort, session };
}

/** The size of a symbolic input dimension for one row decoded from position 0: one row, nothing cached yet, and the
 * attention head width the exported caches are declared with (docs/guidelines/onnx.md, "The graph"). */
function dimension(name) {
  switch (name) {
    case 'batch_size':
      return 1;
    case 'past_sequence_length':
      return 0;
    case 'kv_cache_dim':
      return 256;
    default:
      throw new Error(`the graph has an input dimension the engine does not know: ${name}`);
  }
}

/** An fp16 value from its bits. */
function half(bits) {
  const sign = bits & 0x8000 ? -1 : 1;
  const exponent = (bits >> 10) & 0x1f;
  const fraction = bits & 0x3ff;
  if (exponent === 0) return sign * 2 ** -14 * (fraction / 1024);
  if (exponent === 31) return fraction ? NaN : sign * Infinity;
  return sign * 2 ** (exponent - 15) * (1 + fraction / 1024);
}

/** Decodes one prompt row, `ids` (a Uint32Array), with empty caches, and returns the logits of its last position (the
 * only one the graph outputs) at `labels` (a Uint32Array), as a Float32Array. */
export async function decode({ ort, session }, ids, labels) {
  const n = ids.length;
  const metadata = new Map(session.inputMetadata.map((input) => [input.name, input]));
  const feeds = {};
  for (const name of session.inputNames) {
    if (name === 'input_ids') {
      feeds[name] = new ort.Tensor('int64', BigInt64Array.from(ids, BigInt), [1, n]);
    } else if (name === 'attention_mask') {
      feeds[name] = new ort.Tensor('int64', new BigInt64Array(n).fill(1n), [1, n]);
    } else if (name === 'position_ids') {
      // Interleaved MRoPE: text positions repeat on each of the three axes.
      const positions = new BigInt64Array(3 * n);
      for (let axis = 0; axis < 3; axis++) {
        for (let i = 0; i < n; i++) positions[axis * n + i] = BigInt(i);
      }
      feeds[name] = new ort.Tensor('int64', positions, [3, 1, n]);
    } else {
      const input = metadata.get(name);
      const dims = input.shape.map((d) => (typeof d === 'number' ? d : dimension(d)));
      const size = dims.reduce((a, b) => a * b, 1);
      const zeros = input.type === 'float16' ? new Uint16Array(size) : new Float32Array(size);
      feeds[name] = new ort.Tensor(input.type, zeros, dims);
    }
  }
  const row = session.run(feeds, ['logits']);
  decoding.set(session, row);
  const outputs = await row.finally(() => {
    if (decoding.get(session) === row) decoding.delete(session);
  });
  const logits = outputs.logits;
  try {
    const vocab = logits.dims[logits.dims.length - 1];
    const last = (logits.dims[logits.dims.length - 2] - 1) * vocab;
    const read = logits.data instanceof Uint16Array ? half : Number;
    return Float32Array.from(labels, (id) => read(logits.data[last + id]));
  } finally {
    logits.dispose();
  }
}

/** Frees a session's weights, once the row it is decoding is over: a run stopped mid-row (picking a server row, say)
 * no longer wants that row, but onnxruntime-web is still running it. */
export async function release({ session }) {
  // The row's own outcome is its run's, which is over; here it only has to end.
  await decoding.get(session)?.catch(() => {});
  return session.release();
}
