# Web build files

This directory holds the static page that boots the WebAssembly build of the
game. It is copied into `dist_wasm/` by `build_wasm.sh` together with the
compiled `hexfront.wasm` module, `maps/*.map` and `sounds/*.ogg`.

## Contents

- `index.html` — full-page canvas and the call that loads `hexfront.wasm`.
- `mq_js_bundle.js` — the JavaScript runtime shipped with
  [macroquad](https://github.com/not-fl3/macroquad) 0.4.16, vendored from the
  crate's `js/mq_js_bundle.js`. Keeping a local copy means the game does not
  depend on a CDN and the runtime always matches the linked macroquad version.
  Update this file together with the `macroquad` dependency. One local fix is
  applied: the unused `quad_net` plugin assigned `register_plugin` without
  declaring it, which throws a `ReferenceError` in the bundle's strict mode.
  The assignment now uses `var`.

`mq_js_bundle.js` is part of macroquad, licensed under MIT OR Apache-2.0; that
licence applies to this bundled file. The game itself is distributed under the
terms in the repository's [`LICENSE`](../../LICENSE), which is GPL-3.0.

## Building and running

Run the build script from the repository root:

```sh
./build_wasm.sh
```

It creates `dist_wasm/` (ignored by git). The directory must be served over
HTTP because the browser fetches the wasm module, maps and sounds with
`XMLHttpRequest`; opening `index.html` through `file://` does not work. For a
local check:

```sh
python3 -m http.server 8080 --directory dist_wasm
```

Then open <http://localhost:8080/>. A newly added map is picked up only after
`build_wasm.sh` runs again: a browser cannot list `maps/`, so the level list is
embedded in the wasm module at compile time by `rust/build.rs`.
