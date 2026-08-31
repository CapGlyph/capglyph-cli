# Performance rules

1. Performance is a contract expressed through accepted, measurable budgets; descriptive goals alone are not acceptance criteria.
2. A claim records workload, input corpus, hardware, OS, build profile, toolchain, warmup, sample size, variance, baseline, and exact revision.
3. Measure raster decode, thinning, resampling, FFT, and export emission separately.
4. Bound decoded images, polyline counts, FFT terms, template expansion, and queue depths before optimizing.
5. WASM targets must keep `rayon`/`imageproc` out of `wasm32-unknown-unknown` graph; verify with `cargo tree --target wasm32-unknown-unknown`.
6. Benchmark fixtures and traces must be reproducible and secret-minimizing. Cross-platform claims require evidence on the named platform.
7. A benchmark improvement cannot weaken correctness, security, compatibility, or recovery.
8. Record regressions and residual uncertainty in CarryCtx; update canonical performance requirements when an accepted budget changes.
