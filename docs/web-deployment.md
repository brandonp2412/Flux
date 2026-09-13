# Web production deployment

The current Flux web backend emits a self-contained static DOM application. Create a deployment artifact with `flux package web <file.flux|package-dir|flux.toml>`. Manifest-backed packages default to `dist/<package>-<version>-web`; `-o <directory>` selects another destination.

The deployment root contains one `index.html` with the production CSS and JavaScript embedded directly. It contains no development-server version endpoint or save-triggered reload code, so the directory can be uploaded unchanged to a static file host, CDN, object-storage website, or an ordinary web server. Flux does not require a deployment daemon or hosted Flux service.

Package output is intentionally immutable: an existing destination is rejected rather than updated in place. Build to a fresh path and let the hosting system perform its normal atomic release or directory swap.

The bootstrap web backend currently covers the declarative application surface documented by the web-target milestone. Production deployment does not expand that source/runtime surface; unsupported web application code remains a compile-time error rather than being replaced by a framework or browser VM fallback.
