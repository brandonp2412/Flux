# Debugging Flux

Flux debug builds preserve `.flux` source paths and line tables through native lowering. On Linux, `flux debug` uses GDB as the bootstrap source debugger instead of exposing generated C as the developer workflow.

## Start a debugger

```sh
flux debug examples/branches.flux
```

Flux compiles the target in debug mode, opens GDB on a temporary native binary, and keeps the original Flux source locations in the debug information. Standard GDB source commands work against `.flux` files:

```text
break examples/branches.flux:3
run
next
step
bt
```

You can install one or more breakpoints before the debugger opens:

```sh
flux debug examples/branches.flux --break examples/branches.flux:3
flux debug examples/branches.flux -b examples/branches.flux:3 -b classify
```

Add `--run` to start the program immediately after the requested breakpoints are installed:

```sh
flux debug examples/branches.flux --break examples/branches.flux:3 --run
```

The bootstrap debugger intentionally delegates process control to GDB. Breakpoints, source stepping, and stack frames already resolve to Flux files and lines. Locals and function symbols can still expose compiler-generated native names, so Flux-aware local/watch presentation remains roadmap work rather than being claimed as finished.

`flux doctor` reports whether GDB is available on the development host.