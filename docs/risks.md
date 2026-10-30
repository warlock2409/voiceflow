# Architectural risks and status

| # | Risk | Status after Phase 1 |
|---|---|---|
| 1 | The floating bar steals focus, so Phase 2 insertion would go to the wrong window | **Hit and fixed.** `focusable(false)` alone still activated the bar. Fixed with `WS_EX_NOACTIVATE` + `SW_SHOWNOACTIVATE`, and verified with Notepad + hotkey + real mouse clicks. The macOS equivalent (non-activating `NSPanel`) is still unverified. |
| 2 | whisper.cpp native build on Windows | **Resolved.** Builds with MSVC 2019 + CMake 4.4 + LLVM 23 (`LIBCLANG_PATH`). First full build takes about 8 min. |
| 3 | Whisper hallucinates on silence/noise ("you") | **Hit and fixed.** Seen live on this laptop's mic. Neither Whisper's own `no_speech_probability` (~0 on noise) nor whisper.cpp's in-decoder VAD prevented it. Fixed with an explicit Silero VAD stage before Whisper. Covered by `audio::vad` tests and a controller test. |
| 4 | Model distribution (the definition of done forbids manual downloads) | Phase 1 does an in-app download with SHA-256 verification. The VAD model is bundled. **Open:** bundle base.en in the installer (+148 MB), or keep first-run download, decided in Phase 2. |
| 5 | Latency: Whisper is not streaming, and the 30 s encoder window gives a floor of ~1.5 s (base.en) | Meets target (RTF < 1). Options for later: tiny.en for speed, CUDA feature, clang-cl ggml build, streaming in a later phase. |
| 6 | Language goals (Indian English, Tamil-English) | `.en` models are English-only. The model and language settings already exist. Multilingual `base`/`small` and a Tamil code-switching evaluation are future work. |
| 7 | Mobile parity | Not started, and nothing is claimed. See `mobile-feasibility.md`. |
| 8 | Hotkey collisions with other dictation tools | Wispr Flow was running on the dev machine. `RegisterHotKey` conflicts are detected and shown, but tools that use low-level keyboard hooks can still see the same keys. The user can change the shortcut. |
| 9 | whisper-rs leaks a small `CString` per call when VAD params are set on `FullParams` | Avoided: VAD runs through the standalone `WhisperVadContext` instead. |
| 11 | Caps Lock push-to-talk uses a global low-level keyboard hook | Some antivirus or enterprise policies flag keyboard hooks, so the fallback is any modifier shortcut via `RegisterHotKey`. The hook ignores everything except Caps Lock. On macOS this needs a different mechanism (CGEventTap plus Input Monitoring permission), and that is not implemented. |
| 12 | The hidden main window stole focus at launch | **Hit and fixed.** It also blocked the Caps Lock hook for about 2-6 s after startup. Verified with a hold test at 1, 2, 3, 4, 5, 6 and 8 s after launch: 7/7 passed, and focus stayed in the user's app. |
| 10 | Echo cancellation on laptop mics (Intel Smart Sound) | Scrambles speaker-to-mic loopback testing. Automated accuracy tests use the WAV fixture, and live accuracy needs a human voice test. |
