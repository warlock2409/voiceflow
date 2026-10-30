# Benchmarks (Phase 1)

All numbers are measured, not estimated. Machine: HP laptop, Intel Core i5-11400H (6C/12T, 2.7 GHz base),
16 GB RAM, Windows 11 (build 26200), GTX 1650 **not used** (CPU-only build). whisper.cpp via whisper-rs 0.16,
built with MSVC 2019, CMake Release. whisper.cpp system info: `AVX2 = 1 | FMA = 1 | AVX512 = 1 | F16C = 1`.

## Transcription: fixture (`tests/fixtures/jfk.wav`, 11.0 s, 22 words)

`cargo test --release --lib transcribes_jfk -- --nocapture` with `VOICEFLOW_TEST_MODEL` set.

| Model | Size | Load | Inference | RTF | Output |
|---|---|---|---|---|---|
| tiny.en | 78 MB | 140-200 ms | 3.2-3.7 s | ~0.30 | exact match |
| base.en | 148 MB | ~300 ms | 7.3-7.6 s | ~0.67 | exact match |

Both models transcribed the fixture exactly: "And so my fellow Americans ask not what your country can do
for you, ask what you can do for your country." **Both meet the spec target of processing no slower than
the audio's duration (RTF < 1) on this machine.**

Thread scaling for tiny.en is below. The default is `min(cores, 8)`, which is 8 threads on this machine.

| Threads | Time |
|---|---|
| 1 | 12.3 s |
| 2 | 7.7 s |
| 4 | 4.9 s |
| 6 | 3.8 s |
| **8** | **3.2 s** |
| 12 | 3.5 s |

Notes:
- Whisper always encodes a 30 s window. Short clips therefore cost about 1.5 s minimum with base.en, even for 2-3 s of audio.
- Decoding time grows with the number of words, so speech-dense audio is slower than sparse audio.
- The VAD step (Silero, bundled) costs about 0.1 s. It trims leading and trailing silence before Whisper runs.

## Live app (real microphone, `tauri dev`)

| Measurement | Result | Spec target |
|---|---|---|
| Launch to bar visible (release exe, 3 runs) | 1229 ms (cold), 664 ms, 686 ms | ≤ 2 s |
| Hotkey to `Recording` state, from mic-open start | 152-232 ms (232 ms on the cold first open) | ≤ 300 ms |
| Model warm-up at launch (background) | 240-380 ms (base.en) | n/a |
| 12.3 s utterance, base.en | 1.64 s processing (RTF 0.13)\* | RTF ≤ 1 |
| 2.9 s utterance, base.en | 1.5-1.7 s | n/a |

\*The live-app clips were speaker-to-mic playback, and the laptop mic's echo cancellation removed most of
the speech. Few words were decoded, so these RTFs look better than real dictation will. The fixture numbers
above are the representative ones.

## Not yet measured

- The installed (NSIS) build. It was built but not installed. Launch timing above is from `target/release`.
- Idle CPU and memory.
- CUDA (`--features cuda`), and a clang-cl build of ggml (MSVC builds of ggml are commonly slower).
- `small.en` and the multilingual models.
