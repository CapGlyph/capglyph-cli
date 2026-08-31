# Security rules

1. The normative security overview and threat model override non-security suggestions.
2. Treat raster images, SVG, WOFF2 fonts, ZIP archives, and reference repositories as untrusted.
3. P0 requires bounded parsing; explicit gates for file, network, and IPC; least-privilege scopes; safe mode; and supply-chain controls.
4. Forbid bare `unwrap()`/`expect()` on untrusted input paths; `vectomancy/text/src/parser.rs` is the exemplar.
5. Forbid unbounded input, work, or allocation in raster/svg parsing and export encoding.
6. Security-sensitive changes require negative, malformed, limit, timeout, fuzz, and safe-mode evidence.
7. Keep risks open until both mitigation and independent reviewer evidence are complete.
8. Record P0 blockers and actionable findings in CarryCtx before handoff.
