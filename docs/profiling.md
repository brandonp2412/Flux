# CPU profiling

`flux profile` provides the bootstrap Linux CPU profiler workflow. It compiles the selected Flux program with the optimized `profile` build settings plus native `gprof` instrumentation, runs that exact binary, and prints the resulting CPU profile when the process exits.

```sh
flux profile benchmarks/perf/compute.flux
```

The dedicated command is intentionally separate from `flux build --mode profile`: profile build mode keeps optimization, debug information, and frame pointers suitable for external tools, but does not silently add instrumentation overhead to ordinary profile builds.

The profiler stores `gmon` data in an isolated temporary directory and removes both the instrumented binary and profiling data after reporting. Program stdin/stdout/stderr remain attached to the terminal, so interactive native programs can be profiled as well.

This first slice exposes native function-level CPU hotspots through `gprof`. Flux-aware symbol demangling, richer source mapping in profiler reports, allocation/memory analysis, render timelines, and low-overhead production profiling remain roadmap work.

`flux doctor` reports whether `gprof` is available on the development host.