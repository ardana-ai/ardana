# onnxruntime-web

The runtime the playground's in-tab engine runs browser models on (`src/engine.js`), vendored so the page loads
nothing from another origin. trunk copies this directory into `dist/ort/`; `ardana serve` embeds it with the page and
serves it at `/ort/`. Replace the files from the sources below, never edit them; a new version gets a new directory,
since the server lets browsers cache these files for a week.

| File | Source |
| --- | --- |
| `1.30.0/ort.webgpu.bundle.min.mjs` | `package/dist/ort.webgpu.bundle.min.mjs` of the npm package `onnxruntime-web` 1.30.0, `https://registry.npmjs.org/onnxruntime-web/-/onnxruntime-web-1.30.0.tgz`, checked against the registry's `dist.integrity` `sha512-q0y+JrrtukXSzsBWEMccVfqX25LRmosXHF+CaRJmg8pZClzcV7svNc4rKY3jL02Vb7QmRMDs1SigqR4CXAfKYQ==`. The WebGPU build with its WASM glue bundled in. |
| `1.30.0/ort-wasm-simd-threaded.asyncify.wasm` | `package/dist/ort-wasm-simd-threaded.asyncify.wasm` of the same tarball: the WASM module the WebGPU bundle loads from beside itself. |
| `1.30.0/ort.wasm.bundle.min.mjs` | `package/dist/ort.wasm.bundle.min.mjs` of the same tarball: the plain WASM build, for a browser without WebGPU. The WebGPU build's module has no CPU kernel for the quantized embedding (`com.microsoft.GatherBlockQuantized`) the int4 graphs read, so its `wasm` provider cannot run them; this build's can. |
| `1.30.0/ort-wasm-simd-threaded.wasm` | `package/dist/ort-wasm-simd-threaded.wasm` of the same tarball: the WASM module the plain WASM bundle loads from beside itself. |
| `1.30.0/LICENSE.txt` | `LICENSE` of `microsoft/onnxruntime` at the tag `v1.30.0` (commit `f2c39fe2f838cf35ce7da92824f5a5e3ee6e88a7`): MIT, Copyright (c) Microsoft Corporation. The npm package names the licence in `package.json` and its bundle header but ships no licence file. |
| `1.30.0/ThirdPartyNotices.txt` | `ThirdPartyNotices.txt` of `microsoft/onnxruntime` at the same tag: the notices of the components compiled into the WASM modules. |

sha256: `0730816731ce2cdfb06f273da065d4a1c7091eb9d1756a83b2916c8a89d015e2` (`ort.webgpu.bundle.min.mjs`),
`39f9f0894d478800487ed9f7dbe92618498db320cf55c8e3d89adff8dce658da` (`ort-wasm-simd-threaded.asyncify.wasm`),
`11e64bd8ffe11bd1a2a2f0d6275fdfbbba7262f0b76b99b53d228a8a22ef3d90` (`ort.wasm.bundle.min.mjs`),
`3398c10d07d229bd91b364548e130e0e51a8e5704b88c7c083ebbeb78842dee2` (`ort-wasm-simd-threaded.wasm`).
