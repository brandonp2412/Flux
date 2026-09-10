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

Flux also installs a small debugger support layer into that GDB session. It keeps compiler-generated local names out of the normal inspection workflow:

```text
flux-locals
flux-print value
flux-eval value + 3 * 2
flux-eval value < 0 && true
flux-watch value
next
flux-unwatch value
```

`flux-locals` shows visible parameters and locals with their Flux names. `flux-print name` inspects one visible local. `flux-eval expression` evaluates a side-effect-free Flux expression over visible primitive locals using Flux semantics rather than handing source text to GDB's C evaluator. The bootstrap evaluator supports `i64`, `bool`, and `str` literals/locals, parentheses, `!`/unary `-`, checked integer `+ - * /`, comparisons/equality, and short-circuit `&&` / `||`. `flux-watch name` reports a local each time execution stops until `flux-unwatch name` removes it. Watches report an explicit `<out of scope>` state instead of accidentally resolving another native symbol.

You can install one or more breakpoints before the debugger opens:

```sh
flux debug examples/branches.flux --break examples/branches.flux:3
flux debug examples/branches.flux -b examples/branches.flux:3 -b classify
```

Add `--run` to start the program immediately after the requested breakpoints are installed:

```sh
flux debug examples/branches.flux --break examples/branches.flux:3 --run
```

The bootstrap debugger intentionally delegates process control to GDB while Flux supplies source-aware inspection and primitive expression evaluation on top. `flux-eval` does not execute function calls, assignments, platform operations, or aggregate/member access, so debugger evaluation cannot mutate the debuggee accidentally; richer aggregate/ownership-aware inspection and async/task debugging remain roadmap work.

`flux doctor` reports whether GDB is available on the development host.
