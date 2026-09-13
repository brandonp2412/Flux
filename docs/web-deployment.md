# Web production deployment

The current Flux web backend emits a self-contained static DOM application. Create a deployment artifact with `flux package web <file.flux|package-dir|flux.toml>`. Manifest-backed packages default to `dist/<package>-<version>-web`; `-o <directory>` selects another destination.

The deployment root contains one `index.html` with the production CSS and JavaScript embedded directly. It contains no development-server version endpoint or save-triggered reload code, so the directory can be uploaded unchanged to a static file host, CDN, object-storage website, or an ordinary web server. Flux does not require a deployment daemon or hosted Flux service.

Web builds use the browser platform directly by default. `flux build web ... --wasm never` is the explicit form of that default and emits compiler-generated JavaScript for supported application functions. `--wasm auto` lets the compiler lower functions from the currently supported pure `i64` slice into an embedded WebAssembly module while keeping DOM/CSS/browser APIs as the application and UI substrate. `--wasm always` requires every reachable application function to fit that WASM slice and fails at compile time instead of silently falling back. The generated WASM bytes stay embedded in the same self-contained `index.html`; WASM is not a framework runtime, UI renderer, plugin channel, or user-authored JavaScript bridge.

Package output is intentionally immutable: an existing destination is rejected rather than updated in place. Build to a fresh path and let the hosting system perform its normal atomic release or directory swap.

The bootstrap web backend currently covers the declarative application surface documented by the web-target milestone. Production deployment does not expand that source/runtime surface; unsupported web application code remains a compile-time error rather than being replaced by a framework or browser VM fallback.
