import { readFileSync, writeFileSync } from "node:fs";
import { initSync, compress_image, get_dimensions } from "./pkg/wasm_image_compressor.js";

const wasmBytes = readFileSync(new URL("./pkg/wasm_image_compressor_bg.wasm", import.meta.url));
initSync({ module: wasmBytes });

// Build a tiny PNG at runtime is fiddly without a library, so instead we
// synthesize one using a minimal raw pixel buffer written via zlib -- but
// simplest: just read the PNG test fixture in test_assets/sample.png if it
// exists, otherwise bail with instructions.
const fixturePath = new URL("./test_assets/sample.png", import.meta.url);
let bytes;
try {
  bytes = readFileSync(fixturePath);
} catch {
  console.error("Missing test_assets/sample.png fixture");
  process.exit(1);
}

console.log("Input bytes:", bytes.length);
const dims = get_dimensions(bytes);
console.log("Decoded dimensions:", dims[0], "x", dims[1]);

const out = compress_image(bytes, 100, 100, 70, "jpeg");
console.log("Output bytes:", out.length);
const outDims = get_dimensions(out);
console.log("Output dimensions:", outDims[0], "x", outDims[1]);

writeFileSync(new URL("./test_assets/sample-out.jpg", import.meta.url), out);
console.log("Wrote test_assets/sample-out.jpg");
console.log("SMOKE TEST OK");
