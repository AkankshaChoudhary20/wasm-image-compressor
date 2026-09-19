# wasm-image-compressor

A drag-and-drop image resizer/compressor that runs entirely inside your
browser tab. The Rust core is compiled to WebAssembly; there is no server,
no upload endpoint, and no network request involved in processing an image.

## The problem this solves

Most "compress my image online" tools work by uploading your file to
someone else's server, processing it there, and sending back a result --
which means a photo you'd rather not hand over (a screenshot with personal
info in it, a scan, whatever) leaves your machine regardless of how much
you trust the site's privacy policy. This tool decodes, resizes, and
re-encodes the image with a WebAssembly module running in the page itself.
Open devtools' Network tab while using it: no request fires when you
process a file, because none needs to.

## Why WebAssembly

- **Privacy by construction, not by policy.** The image bytes never leave
  `window` -- there's no server code in this project at all, so there's
  nothing to trust or audit on that front.
- **Native-speed decode/resize/encode in the browser.** PNG/JPEG decoding
  and image resampling are exactly the kind of tight numeric loop that's
  slow in JS and fast in compiled code. Rust's `image` crate does the
  decode/encode work; wasm-bindgen exposes it to JS with a plain function
  call.
- **The same core logic is testable outside the browser.** Because the
  resize/compress logic is plain Rust functions (not wasm-bindgen-specific
  code), `cargo test` runs it natively on the host with no wasm runtime
  involved -- see "Tests" below.

## How it works

- `src/lib.rs` decodes PNG/JPEG bytes via the `image` crate
  (`image::load_from_memory`), resizes to fit within a max width/height
  (aspect-ratio preserved, using the `Triangle`/bilinear-family filter --
  never upscales), then re-encodes as JPEG (with a quality setting) or PNG.
- Three `#[wasm_bindgen]` functions are exposed to JS: `compress_image`,
  `get_dimensions`, and `init_panic_hook`. Everything else is plain Rust
  called by those three.
- `index.html` is a single static page: file input -> read bytes into a
  `Uint8Array` -> call `compress_image()` from the wasm module -> show
  before/after size, dimensions, and a preview `<img>` -> a download link
  built from a `Blob` of the returned bytes. No build tooling (webpack/
  vite/etc.) -- it's a plain `<script type="module">` importing the
  wasm-bindgen-generated glue directly.

## Build

```bash
# one-time setup
cargo install wasm-pack        # or: rustup target add wasm32-unknown-unknown
                                #     + cargo install wasm-bindgen-cli

# build the wasm module + JS glue into ./pkg
wasm-pack build --target web --out-dir pkg

# serve the static files (wasm needs to be fetched over http://, not file://)
python3 -m http.server 8080
# then open http://localhost:8080/
```

The compiled `pkg/` output (the `.wasm` binary and its JS/`.d.ts` glue) is
committed to this repo, so cloning + serving with any static file server
is enough to run it without rebuilding. Rebuild only if you change
`src/lib.rs`.

## Tests

```bash
cargo test --release
```

7 native unit tests, run on the host target (no wasm involved) against the
same `resize_to_fit` / `encode_jpeg` / `process_image` functions the wasm
build calls:

```
test tests::process_image_rejects_garbage_bytes ... ok
test tests::resize_does_not_upscale_smaller_images ... ok
test tests::get_dimensions_matches_source_image ... ok
test tests::resize_preserves_aspect_ratio ... ok
test tests::lower_jpeg_quality_produces_smaller_or_equal_output ... ok
test tests::resize_shrinks_oversized_image ... ok
test tests::process_image_end_to_end_png_to_jpeg ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Coverage: aspect-ratio-preserving resize math, the no-upscale guard, JPEG
quality actually affecting output size, a full decode-resize-encode
round trip that re-decodes its own output to check dimensions, and
rejection of garbage (non-image) input.

## What was actually verified here vs. what a user does themselves

Honesty section, since "it compiles" and "it works" are different claims:

- **`cargo test --release` was run and passed** (7/7) -- this exercises
  the real resize/encode logic natively, no wasm involved.
- **The wasm build was actually run**: `wasm-pack build --target web`
  completed and produced a real `pkg/wasm_image_compressor_bg.wasm`
  (~420 KB) plus JS glue. This was not just assumed to work from the
  source compiling -- the `.wasm` binary in `pkg/` is the literal output
  of that build.
- **The compiled wasm module was exercised outside a browser** as an
  extra check: a small Node.js script (`smoke_test.mjs`) loads
  `pkg/wasm_image_compressor_bg.wasm` via wasm-bindgen's `initSync`,
  decodes `test_assets/sample.png` (a synthetic 300x200 PNG), calls
  `compress_image(..., 100, 100, 70, "jpeg")`, and writes the result to
  `test_assets/sample-out.jpg`. That output file is committed and is a
  real, valid JPEG (147,313 bytes in -> 1,467 bytes out, 100x67px) --
  confirming the actual compiled `.wasm` artifact decodes, resizes, and
  re-encodes correctly, using the exact glue code the browser would load.
  Run it yourself with `node smoke_test.mjs`.
- **What was *not* verified here**: the `index.html` page was not opened
  in an actual browser during this build (no browser automation was run
  in this environment). The JS in `index.html` calls the same three
  exported functions (`compress_image`, `get_dimensions`,
  `init_panic_hook`) that the Node smoke test already exercises
  end-to-end against the real wasm binary, so the remaining risk is
  narrow -- mostly DOM/UI wiring (file input handling, `Blob`/object-URL
  creation, button state) rather than the wasm boundary itself. If you
  clone this, serve `index.html` with any static file server (it must be
  served over `http(s)://`, not opened via `file://`, or `fetch()`-ing the
  `.wasm` file will fail) and try it with a real PNG/JPEG.
- **Not tested**: very large images (multi-thousand-pixel photos) --
  resizing those is more CPU/memory work per call and might be worth a
  loading spinner or a web worker if you extend this; this build only
  used a small synthetic test image.

## Project layout

```
wasm-image-compressor/
├── src/
│   └── lib.rs          # decode/resize/encode core + wasm-bindgen bindings + unit tests
├── pkg/                # wasm-pack build output (committed): .wasm binary + JS/TS glue
├── index.html           # the entire front end: file input, preview, download
├── smoke_test.mjs        # Node script that exercises the compiled .wasm outside a browser
├── test_assets/
│   ├── sample.png        # synthetic input used by the smoke test
│   └── sample-out.jpg    # its real, committed output (147 KB -> 1.4 KB)
├── Cargo.toml
└── Cargo.lock
```

## What's intentionally not here

No webpack/vite/rollup -- a plain ES module import of the wasm-bindgen
glue is enough for a project this size. No server component of any kind
(that's the whole point). No WebP/AVIF/HEIC support -- the `image` crate
supports more formats than PNG/JPEG, but those two cover the common case
and keeping the feature list in `Cargo.toml` narrow keeps the wasm binary
smaller. No web worker -- resizing runs on the main thread, which is fine
for the image sizes this was tested with but would stutter the UI on a
very large image.
