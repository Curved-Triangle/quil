# Security and privacy

## Security boundary

Quil's dictation runtime is designed to operate without network access. HTTP support is isolated to the separate `quil-model-fetch` executable, which runs only after an explicit install/update action. There is no telemetry, account system, crash upload, advertisement, or automatic update channel.

The macOS app milestones must ship with App Sandbox enabled and without incoming or outgoing network entitlements. Microphone and Accessibility are the only requested permissions, each with a clear purpose explanation. A sandboxed deny-network test and dependency-graph checks belong to CI.

## Audio handling

- Captured microphone samples stay in memory and are never written to disk.
- Audio buffers are bounded and zeroed after use.
- Logs must contain timings, errors, and counters—not audio or dictated text.
- Benchmark audio comes from external documented corpora or synthetic generators, never from Quil microphone capture.
- Clipboard restoration is required when paste fallback is used in the later UI milestone.

## Model supply chain

Model downloads require a deliberate command/action. Every artifact has a checked-in expected SHA-256 and byte length, is downloaded to a temporary file, and is installed atomically only after verification. Model files are data; Quil does not execute downloaded scripts or install downloaded native code. The ONNX Runtime static library is a separate build-time artifact, fetched only by `scripts/fetch_onnxruntime.py` with independently pinned archive and library hashes; it is never fetched by the dictation runtime.

## Dependency policy

The core excludes GPL/AGPL dependencies. Dependencies and the Rust toolchain are pinned; CI runs `cargo audit`, `cargo deny`, Clippy with warnings denied, rustfmt, tests, and benchmark checks. Unsafe Rust is restricted to narrow platform/FFI boundaries and every block must document its safety invariants.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub Security Advisories for `permitzip/quil`. Do not include private dictated text, microphone recordings, tokens, or other secrets in a report. While the repository is private, contact an existing repository maintainer for access to the private reporting channel.

## Scope notes

Checksum verification detects corrupted or unexpected artifacts only when the checked-in expected hash is trustworthy; it is not a substitute for upstream review. macOS Accessibility permission grants broad input-control capability, so Quil must request it only for cursor insertion and must not collect keystrokes. Code signing and notarization are release-milestone requirements, not claims about unsigned development builds.
