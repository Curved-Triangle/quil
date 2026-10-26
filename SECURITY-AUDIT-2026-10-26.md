# Quil Security Audit — 2026-10-26

**Repository:** `permitzip/quil` · `main` @ `10d3c11` ("Close security audit findings: verify model digests at runtime") · clean tree
**Method:** read-only — five parallel deep-dive audits (network egress, audio privacy, model/download supply chain, unsafe/FFI/keystroke-injection, CI/packaging/hygiene) plus tooling (`cargo audit 0.22.2`, `cargo deny 0.20.2` via repo-pinned `.tools/cargo`, git secret scans, tracked-file review) and manual spot-verification of the critical paths and every `unsafe` claim against vendored dependency sources. Nothing was built, downloaded, or mutated.

---

## Executive summary

**No Critical or High findings. 7 Medium, 12 Low, several informational notes.** Every non-negotiable policy in `AGENTS.md`/`SECURITY.md` is actually implemented in code and test-enforced — including the headline "runtime audio and transcription never use the network" guarantee, which holds structurally, empirically, and procedurally. The previous audit's closure (commit `10d3c11`, runtime digest verification) is verified in place.

The Medium findings are: a WAV-input memory-amplification DoS, three privacy-hygiene gaps where zeroization cannot reach all copies of audio (reallocated buffers, native resampler/whisper.cpp internals), wrong-window text insertion (focus hop), an accidental-only defense against enigo Tab/newline synthesis, stuck-hotkey failure modes, and a sandbox-vs-Accessibility design conflict that must be resolved before the bundled-app milestone.

| Audit domain | Verdict |
|---|---|
| Runtime network isolation & telemetry | ✅ PASS — no HTTP/TLS/DNS/socket code reachable from any runtime crate; no telemetry anywhere |
| Model/download supply chain | ✅ PASS — pinned independent SHA-256 + length; verify → atomic install → re-verify → re-verify at every load |
| Audio privacy & real-time safety | ✅ PASS with noted hardening gaps (M2, M3, L9) |
| unsafe Rust / FFI discipline | ✅ PASS — 4/5 crates `forbid(unsafe_code)`; every block commented; ABI contracts verified |
| Keystroke injection / keylogging | ✅ No keylogging; injection limited to dictated text; one boundary-hardening gap (M5) |
| CI/CD | ✅ PASS — read-only perms, SHA-pinned actions, no secrets, no untrusted triggers |
| Entitlements/packaging | ✅ Compliant today; design conflict pending (M7) |
| Dependency & advisory tooling | ✅ cargo audit clean (336 deps vs 1,352 RustSec advisories); cargo deny clean |
| Secrets & repo hygiene | ✅ No secrets; no model weights or recorded audio tracked |

---

## Threat model

**Assets:** microphone audio (highly sensitive), transcript text, user's focused-app context, model artifacts, build toolchain.

**Adversaries considered:** a malicious/compromised download endpoint (Hugging Face, `cdn.pyke.io`, OpenSLR), a malicious input WAV file, post-install file tampering by same-user local code, poisoned CI caches/actions, malicious crates, focus-grabbing apps during the inference window, and accidental privacy regressions.

**Trust roots:** the checked-in `models/catalog.json` (embedded via `include_str!` at compile time, `crates/quil-core/src/catalog.rs:6`), `Cargo.lock` + pinned toolchain (Rust 1.94.0), and vendored binding sources — all accurately scoped in `SECURITY.md:30-32`.

---

## Findings

### Medium

**M1 — WAV input memory-amplification DoS (untrusted file input).**
`crates/quil-audio/src/wav.rs:8-54` (`read_wav`) imposes no duration/sample-count cap, and `crates/quil-audio/src/pcm.rs` (`resample_to_16k`) allocates output proportional to `input_len × 16000 / claimed_rate`. A small crafted WAV claiming a 1 Hz sample rate multiplies memory ~16,000× → OOM abort or capacity panic. Moonshine's 64-second cap (`moonshine.rs:131-136`) and the benchmark 600-second cap (`benchmark.rs:91-95`) are enforced only after the amplified resample. Affects `transcribe`/`bench` with attacker-supplied files; not a privacy violation (stays in RAM).
**Fix:** cap accepted duration inside `read_wav` (the mic path already caps at 300 s, `capture.rs:147-151`), and pre-check resampler output capacity before allocating.

**M2 — Unwiped reallocation copies of file audio in `read_wav`.**
`wav.rs:18-35` pushes samples into a `Zeroizing<Vec<f32>>` one at a time; each Vec growth frees the previous allocation unwiped (a documented `zeroize` limitation). Applies to the user-file `Transcribe` path. **Fix:** seed with `Vec::with_capacity(reader.len())` (hound reports the header sample count) so growth never happens — same fix also cheapens M1's cap.

**M3 — Native backend copies escape zeroization (mic-derived audio).**
rubato 5.0.1's `Fft` resampler buffers mic-derived PCM internally and frees it unwiped, and whisper.cpp copies PCM into per-call state and log-mel spectrograms inside C++ (`crates/quil-inference/src/whisper.rs:61,79`), also freed unwiped. This is the one place the "buffers are zeroed after use" guarantee cannot be honored from pure Rust today. Mitigations already present: per-call state creation and the Moonshine ONNX path deliberately *borrows* the caller's buffer (`moonshine.rs:143-150`) so the Quil-owned wipe covers it.
**Fix:** document this exception in `SECURITY.md`/`AGENTS.md`; consider a reused-and-wiped whisper state; keep tracking upstream.

**M4 — Focus-hop insertion (wrong target app).**
`crates/quil/src/insert.rs:85-100` sets `AXSelectedText` on whatever is focused *at insertion time*. During the inference window (150 ms–seconds), a focus grab (e.g., a password field) receives the transcript. No session-start focus pinning exists.
**Fix:** record the focused element's pid at session start (available via `objc2-application-services`) and abort/confirm insertion if it differs at commit time; mention the protection in the Accessibility purpose text.

**M5 — enigo Tab/newline synthesis blocked only by distant-committer coincidence.**
enigo 0.6.1's `fast_text` synthesizes a **real Tab keystroke** for chunks beginning with `\t`, and uses a ZWSP workaround for leading newlines (`enigo-0.6.1/src/macos/macos_impl.rs:296-315`). The dictation path is safe today only because `PartialCommitter` splits on whitespace (`streaming.rs:67,82-84`) — a refactor removing `split_whitespace` would silently enable Tab synthesis (e.g., shell completion in a terminal). The defense lives two files away from the FFI boundary it protects.
**Fix:** sanitize/normalize whitespace in `TextInserter::insert` itself plus a regression test asserting the committer cannot emit `\t`/`\n`-leading chunks. (Note: `insert-test` reaching Tab synthesis with explicit `$'a\tb'` input is documented opt-in behavior.)

**M6 — Stuck-hotkey failure modes.**
A missed hotkey `Released` event (fast user switch, lock screen under press, CPU starvation) leaves `hotkey_down` set forever (`dictate.rs:132-193`): in Toggle mode every later press is swallowed (hotkey permanently bricked until restart); in Hold mode `Stop` never arrives and capture runs open-ended (bounded by the 30 s ring, but recording continues silently). No watchdog or gate re-priming exists.
**Fix:** Hold-mode watchdog that stops capture after N seconds without a `Stop`; Toggle-mode re-prime after a timeout window.

**M7 — App Sandbox vs. Accessibility insertion (design conflict, pre-bundle).**
`packaging/Quil.entitlements` enables `com.apple.security.app-sandbox`, which will block the AX/CGEvent insertion path (`insert.rs:69-100`) when a real bundle ships (`SECURITY.md:7` and the entitlements test both bake in the conflict). Nothing is broken today (no bundle step applies the entitlements yet), but the menu-bar milestone must consciously choose: unsandboxed app, or sandboxed app + non-sandboxed insertion helper (XPC), or a different insertion mechanism.

### Low

1. **Runtime hash-then-reopen TOCTOU** — `verify_artifacts` hashes by path (`crates/quil-inference/src/lib.rs:75-88`), then backends re-open by name (`whisper.rs:24`, `moonshine.rs:50,57,70`); a sub-second file swap evades verification for that load. Requires same-user local write access; fd-based loading isn't supported by current backends. The runtime's per-load re-verification limits exposure to one session.
2. **Python fetchers: weaker than the Rust downloader** — `scripts/fetch_onnxruntime.py` / `prepare_benchmark_corpus.py` check redirects *after* `urllib` has followed them and downloaded the body, use predictable temp names in `.tools/` (`:55,96-97`), and open the lib-side temp `"wb"` (truncate) rather than `"xb"`. Integrity is still digest-anchored, so not exploitable; align with the Rust side (`https_only`-equivalent per-hop check, unpredictable `xb` temps).
3. **Installed assets unverified** — `install.sh:28-30` copies binaries/chimes without digests, the runtime never re-verifies `~/.local/share/quil/assets/*.wav` before `afplay`, and `QUIL_ASSET_DIR` (`dictate.rs:215-231`) accepts any path. Same-user attack surface; pin/re-verify or restrict.
4. **`install.sh` check→install TOCTOU** — the symlink refusal (`:18-26`) and `install(1)` are separate syscalls; race closes only with same-user privileges.
5. **Runtime-network check is a heuristic deny-list** — `scripts/check-runtime-network-deps.sh:8-17` lists ~30 HTTP/TLS crate names, runs `cargo tree` without `--locked`, and the policy isn't mirrored as `cargo deny` bans. Add `--locked`, consider `--all-features`/bans-duplicate, and duplicate the ban in `deny.toml` so the policy is declarative.
6. **`.tools/venv` CMake is an unpinned, manually-pip-installed tool** silently prepended to `PATH` (`scripts/cargo.sh:10-13`) — anything that can write `.tools/venv/bin` shadows `cmake`/`python` for every developer. Add a pinned bootstrap script.
7. **`.gitignore` is path-scoped only** (`:1-11`) — no defense-in-depth globs (`*.onnx`, `*.gguf`, `.env*`, stray `*.wav`); a stray model/weights file has no second tripwire. Add globs with explicit `!` exceptions for the documented fixtures.
8. **Release profile takes unsafe-by-default options** — `Cargo.toml [profile.release]` has no `overflow-checks`/`panic` settings (defaults: overflow checks off, unwind), and a callback-path panic would cross a `C-unwind` FFI boundary in cpal/coreaudio-rs. Quil's callback is panic-free today and allocation-free by test, but: set `overflow-checks = true`, and add a lint/no-panic discipline for the `quil-audio` ring/callback path so the property is police-checked, not incidental.
9. **Dictated text is not zeroized** — `PartialCommitter` strings, intermediate transcripts (`dictate.rs:540-549`), and tokenizer decode output are freed unwiped. Policy technically covers audio only; `Zeroizing<String>` at `reset()`/`finish()` would close it.
10. **`NSAccessibilityUsageDescription` is not a real macOS key** (`packaging/Info.plist:13`) — macOS never shows it; harmless declarative intent, but the purpose text won't actually surface.
11. **Stale README claim** — `README.md:113`: "The repository stays private until a deliberate open-source release" vs. `AGENTS.md` "the repository is public as an early-development project".
12. **No automated action-update watching** — actions are SHA-pinned with version comments, but no Dependabot/renovate exists, so comment↔SHA drift and stale-action advisories are invisible.

### Informational

- `openssl`/`openssl-sys` in `Cargo.lock` are non-macOS target-conditional deps of `native-tls`; never compiled locally; reachable only from `quil-model-fetch` in any case.
- SIGINT/Ctrl-C runs no Rust teardown (no signal handler); buffers stay unwiped until the OS reclaims pages — the `capture.rs:122-124` comment holds only de facto. Handle graceful shutdown at the menu-bar milestone.
- On ring overflow the current callback buffer and everything until the worker notices (≤25 ms tick) is dropped — documented fail-loud trade-off, wording already accurate in `AudioError::Overflow`.
- `benchmark-data/` contains untracked local WAVs; all gitignored, provenance-pinned, unreachable from runtime code. The only tracked audio is the two synthesized chimes and the CC-BY-4.0 LibriSpeech fixture (352 KB, attributed in `tests/fixtures/NOTICE.md`).
- The catalog is embedded in two separate binaries; an old `quil-model-fetch` + new `quil` skew is possible but fails safe (runtime enforces its own pins).
- Backends hardcode artifact filenames (`model.bin`, `encoder.onnx`) rather than `artifact.path` — they coincide today; future divergence surfaces as a loud missing-artifact error, not a bypass.
- The `.git`-component check in catalog path validation is case-sensitive (`.GIT` on case-insensitive APFS); no exploit path exists since the model dir isn't a git checkout.
- enigo may fire its own Accessibility re-prompt if the permission is revoked mid-session (cosmetic double-prompt).
- Enigo posts keycode-0 key-down-only unicode events; apps reading the virtual keycode instead of the unicode field see an anomaly — functional, not a security issue.
- `SECURITY.md` lacks response-expectation/SLA and supported-versions statements — reasonable to defer pre-release.
- `cargo deny` prints ~20 duplicate-version warnings (winit/enigo/objc2/windows version skew) — cosmetic; consider `bans` `multiple-versions = "deny"` on the runtime subtree once version skew narrows.
- `insert-test` is the only arbitrary-character injection path — explicit, opt-in, documented.

---

## Verified security properties (what the audit confirmed works)

1. **No runtime network — triple-enforced.** (a) *Structural:* the only HTTP client (reqwest 0.13.5 + hyper/tokio/native-tls subtree) is reachable solely from `quil-model-fetch`; `cargo tree -i` shows no runtime crate depends on it. (b) *Empirical:* `crates/quil/tests/offline_runtime.rs` runs the real binary (catalog query + real CPU transcription) under a DYLD `connect`/`sendto`/`getaddrinfo` interposer with a deliberate-connect self-test, and under `sandbox-exec (deny network)` — asserts zero connection attempts. (c) *Procedural:* CI network-dep audit + entitlements test. No telemetry/analytics/auto-update code exists anywhere in the workspace.
2. **Model supply chain is genuinely defense-in-depth.** Independent pinned SHA-256 + exact byte lengths embedded at compile time (`include_str!`, `catalog.rs:6`); `https_only(true)` re-verified at every redirect hop; announced-length pre-check + streaming size cap; unpredictable `create_new` temp in the same directory; `sync_all` → hash-verify → atomic `rename` → **re-verify the re-opened destination, delete on mismatch** (`quil-model-fetch/src/main.rs:141-231`); temp always cleaned on failure; `--force` re-verifies before replacing. The runtime re-verifies length + SHA-256 of every artifact **before every model load** (`quil-inference/src/lib.rs:54-91`) with a hard reject — the previous audit's finding is closed correctly. No path exists for a download-endpoint attacker; the trust root (checked-in catalog) is accurately scoped in `SECURITY.md:32`.
3. **Audio privacy holds at the code level for the mic path.** The only disk writes in the runtime are benchmark metrics JSON and the model-fetch artifacts; no logging framework exists at all; the capture callback does exactly one lock-free, allocation-free ring write (test-enforced via a counting `GlobalAlloc`); ring slots and `AudioBuffer`s are `Zeroizing`-wiped including `truncate`/`drop_prefix` tails; `Debug` output redacts PCM (regression-tested); mic buffers are preallocated to hard bounds so no reallocation leaves stale copies. Whisper's own realtime segment printing is explicitly disabled (`whisper.rs:70-73`).
4. **No keylogging, by construction.** Only Carbon `RegisterEventHotKey` (system-side matching of the registered combo only); hotkeys are F1–F12-only, enforced at parse (`dictate.rs:195-213`, fail-closed) with modifiers structurally `None`; global-hotkey's `CGEventTapCreate` media-key path is unreachable from Quil's type flow; `AXSelectedText` is never *read* — only settable-checked and written (`insert.rs:66-100`); no event taps, no `NSPasteboard`, no AppleScript anywhere in the workspace.
5. **unsafe discipline is real.** `#![forbid(unsafe_code)]` in quil-core, quil-audio, quil-inference, quil-model-fetch; the five unsafe blocks in `insert.rs` + two in `system.rs` all carry invariant-explaining `SAFETY:` comments, and their FFI contracts (Create-rule adoption, `NonNull<Boolean>` out-params, null checks) were verified against the exact vendored binding sources. Teardown order (pause → drop → unit stop → deallocate) is correct on every path including panic unwind.
6. **CI is hardened.** Top-level `permissions: contents: read`; every action SHA-pinned with `persist-credentials: false`; no `pull_request_target`/`workflow_run`; no `secrets.*` anywhere; no `${{ github.* }}` inside `run:` shells; caches keyed on lockfiles with model caches that are re-verified at every load even if poisoned; the only artifact is metrics JSON with no transcripts/audio.
7. **Entitlements honor policy today** — sandbox + `device.audio-input` only, no network keys, enforced by `crates/quil/tests/entitlements.rs`; purpose strings match actual behavior (aside from L10's inert key).
8. **Tooling results (run in this audit):** `cargo audit --deny warnings` — clean, 336 dependencies scanned against 1,352 RustSec advisories; `cargo deny check` — advisories/bans/licenses/sources all ok (duplicate-version warnings only); tracked-file secret scans (private keys, AWS/GitHub/Slack/OpenAI tokens, certs) — zero matches; no `.env`/`.pem`/model weights recorded in git; `MODELS.md` ↔ `deny.toml` ↔ catalog license claims consistent (no GPL anywhere in core).

---

## Prioritized remediation

**Near-term (before further dictation work):**
1. M1 + M2 (one change: `read_wav` capacity/duration cap + `with_capacity`) — small, closes the only input-DoS and a privacy gap together.
2. M5 (sanitize at the `TextInserter::insert` boundary + regression test) — prevents a silent regression from turning dictated text into Tab/newline keystrokes.
3. M6 (hotkey watchdog / gate re-priming) — the only unhandled user-facing failure modes; touches mic-privacy (open-ended capture).
4. L5 (`--locked` on the network-dep script; mirror as `cargo deny` bans) — one-liner, makes the flagship guarantee declarative.

**Milestone-2/3 (streaming + menu bar):**
5. M7 (decide sandbox vs. AX insertion architecture) — must be settled before any bundle/signing work.
6. M4 (session-start pid pinning) and L9 (zeroize transcript strings).
7. L8 (`overflow-checks = true`; panic-discipline lint for the `quil-audio` callback path), L6 (pin the `.tools` CMake bootstrap), L7 (`.gitignore` globs).

**Pre-release (hardening milestone):**
8. M3 documentation/exception policy + whisper state reuse-and-wipe; L1–L4 local-attack-surface tightenings; L10, L11, L12, plus `SECURITY.md` SLA/supported-versions when a release exists.

---

*Prepared 2026-10-26 against worktree `main` @ `10d3c11`. Findings M1, M2, M6 (lost-release watchdog), L5, and L11, plus the M5 regression guard, were fixed in the follow-up commits that added this report; those commits also added model-fetch progress feedback and made `make run` fetch a missing model automatically. The remaining findings are tracked above.*
