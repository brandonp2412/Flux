# Profiling

## CPU profiling

`flux profile` provides the bootstrap Linux CPU profiler workflow. It compiles the selected Flux program with the optimized `profile` build settings plus native `gprof` instrumentation, runs that exact binary, and prints the resulting CPU profile when the process exits.

```sh
flux profile benchmarks/perf/compute.flux
```

The dedicated command is intentionally separate from `flux build --mode profile`: profile build mode keeps optimization, debug information, and frame pointers suitable for external tools, but does not silently add instrumentation overhead to ordinary profile builds.

The CPU profiler stores `gmon` data in an isolated temporary directory and removes both the instrumented binary and profiling data after reporting. Program stdin/stdout/stderr remain attached to the terminal, so interactive native programs can be profiled as well.

CPU profiler output removes the compiler's `flux__fn_` prefix from Flux function symbols and resolves sampled native addresses through debug metadata so locations are reported as original `.flux` files and lines whenever an instruction has a source location. Compiler/runtime helpers remain visibly native so profiles do not misrepresent implementation work as application functions.

## Allocation profiling

`flux profile <target> --alloc` builds the same optimized profile-mode native binary, then runs it under glibc `memusage`. After the program exits, `memusage` reports heap total and peak usage, stack peak usage, allocation call counts/bytes/failures, and a block-size histogram. Flux removes the temporary native binary afterward.

```sh
flux profile benchmarks/perf/compute.flux --alloc
```

Allocation profiling is deliberately opt-in and external to normal binaries: ordinary debug/profile/release builds gain no allocator wrapper or profiling runtime. The development Linux host needs `memusage`; `flux doctor` reports its availability. This bootstrap profiler is intended for allocation-pressure investigation rather than leak diagnosis or production telemetry.

## Leak diagnostics

`flux profile <target> --leaks` builds an isolated optimized profile binary with Clang AddressSanitizer instrumentation and runs it with leak detection enabled. Leaked allocations are reported with native allocation stacks and debug locations; Flux's generated `#line` metadata lets compiler-generated application frames resolve back to original `.flux` source where Clang can preserve that mapping.

```sh
flux profile benchmarks/perf/compute.flux --leaks
```

The leak profiler is intentionally separate from normal debug/profile/release builds, so sanitizer instrumentation adds no overhead unless `--leaks` is requested. A sanitizer finding uses a dedicated failing exit status and Flux removes the temporary binary afterward. Full retained-object graph visualization is not meaningful for most current Flux values because the bootstrap language still lacks general owned heap collections; richer heap graphs can extend this tooling as those value models land.

Task/render timelines and low-overhead production profiling remain roadmap work. CPU address-to-source enrichment gracefully falls back to the profiler's native location when `addr2line` is unavailable or an instruction has no Flux source line.
