# Flux manual end-to-end dogfood

This is the canonical Priority-0 acceptance path for the first manually usable Flux application workflow on Linux.

## 1. Create or open an app in a real editor

To dogfood the checked-in acceptance app from the repository root:

```sh
./tools/flux-nvim examples/hello_app.flux
```

To start from your own fresh package instead:

```sh
cargo run -- new /tmp/my-flux-app
./tools/flux-nvim /tmp/my-flux-app/src/main.flux
```

The generated package contains the same minimal native Text/Button/state pattern as the acceptance app and passes `fluxc check` immediately. The launcher builds the bootstrap compiler when needed, adds the checked-in Flux Neovim runtime without changing the user's global configuration, recognizes `.flux` files, enables immediate syntax colouring, and starts `fluxc lsp` for the buffer.

Acceptance in the editor:

- the buffer filetype is `flux`;
- keywords, strings, numbers, types, comments, and operators are coloured immediately;
- the Flux LSP attaches and supplies semantic highlighting;
- diagnostics, completion, hover, go-to-definition, references/rename, formatting, code actions, signature help, and inlay hints use `fluxc lsp` rather than a second editor parser.

Use `:LspInfo` or `:checkhealth vim.lsp` when checking the connection manually.

## 2. Compile and launch the native app from the editor

Inside Neovim run:

```vim
:FluxRun
```

For a manifest-backed package created outside the repository, pass its directory explicitly, for example `:FluxRun /tmp/my-flux-app`.

`FluxRun` opens a terminal split and executes `fluxc run` for the selected Flux target. The development runner builds a native Linux executable and launches a GTK4 window through the compositor. On a Wayland session GTK uses its native Wayland backend.

The same path can be launched directly from a terminal when isolating editor problems:

```sh
cargo run -- run examples/hello_app.flux
```

Acceptance in the native window:

- a window titled `HelloApp` appears;
- the text initially reads `Hello, Flux!`;
- the button initially reads `Click me`;
- clicking the button changes the text to `Clicked!` and the button to `Reset`;
- clicking again restores the original labels.

This interaction comes from Flux `state` plus the typed functional transition `on_press: clicked => !clicked`. The Flux source does not construct GTK objects or a widget/controller hierarchy.

## 3. Exercise save-triggered development

Keep `:FluxRun` running. Edit either state-dependent text literal in `examples/hello_app.flux`, then save the file normally.

Acceptance for the development loop:

- no reload hotkey is required;
- `fluxc run` notices the save and reports that it is compiling;
- a successful replacement build restarts the app automatically;
- the edited text appears in the relaunched native window;
- introducing a compile error leaves the last good app running and reports diagnostics;
- fixing and saving the error causes the next successful replacement build to launch automatically.

State-preserving in-process Fast-Refresh-style hot apply remains a later milestone. The P0 dogfood contract intentionally accepts the existing controlled restart on compatible successful builds while preserving the requirement that normal development is save-triggered and needs no manual reload key.

## 4. Non-interactive preflight

Before a manual run, the compiler/editor pieces can be checked without opening a window:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo run -- check examples/hello_app.flux
cargo run -- build examples/hello_app.flux --mode debug -o /tmp/flux-hello-app
```

The checked-in Neovim integration can also be probed headlessly; CI/tests should verify that `.flux` detection starts a `flux` LSP client and that the server advertises semantic tokens and completion.
