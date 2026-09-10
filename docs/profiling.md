# CPU profiling

`flux profile` provides the bootstrap Linux CPU profiler workflow. It compiles the selected Flux program with the optimized `profile` build settings plus native `gprof` instrumentation, runs that exact binary, and prints the resulting CPU profile when the process exits.

```sh
flux profile benchmarks/perf/compute.flux
```

The dedicated command is intentionally separate from `flux build --mode profile`: profile build mode keeps optimization, debug information, and frame pointers suitable for external tools, but does not silently add instrumentation overhead to ordinary profile builds.

The profiler stores `gmon` data in an isolated temporary directory and removes both the instrumented binary and profiling data after reporting. Program stdin/stdout/stderr remain attached to the terminal, so interactive native programs can be profiled as well.

Profiler output removes the compiler's `flux__fn_` prefix from Flux function symbols and resolves sampled native addresses through debug metadata so locations are reported as original `.flux` files and lines whenever an instruction has a source location. Compiler/runtime helpers remain visibly native so profiles do not misrepresent implementation work as application functions.

Allocation/memory analysis, render timelines, and low-overhead production profiling remain roadmap work. `flux doctor` reports whether `gprof` is available on the development host; address-to-source enrichment gracefully falls back to the profiler's native location when `addr2line` is unavailable or an instruction has no Flux source line.