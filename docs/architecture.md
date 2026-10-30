# Architecture

## Pipeline

```
hotkey / bar click
  -> Controller (state machine, session ids)
  -> AudioSource      cpal/WASAPI capture on its own thread -> downmix -> resample to 16 kHz mono (rubato FFT)
  -> level gate       rejects empty / too short / near-silent audio
  -> VoiceDetector    Silero VAD (bundled, ~0.9 MB); no speech => no Whisper call; crops to speech +-300 ms
  -> SpeechProvider   whisper.cpp via whisper-rs; model cached after first load
  -> Intent::Dictate  RefinementProvider: optional Gemini polish; falls back to raw text on any failure
     Intent::Compose  ContentGenerator: Gemini writes content from the spoken instruction (30 s timeout,
                      plain-text/paste rules, Markdown stripped); failure = retryable error, nothing pasted
  -> TextInserter     paste at the cursor (clipboard + SendInput Ctrl+V) into the window that was
                      focused when dictation started; refuses if focus moved, falls back to clipboard
  -> TextDelivery     clipboard (fallback, or "keep on clipboard" setting)
  -> StateSink        emits `dictation://state` to both webviews
```

Every arrow is a trait, defined in `src-tauri/src/{audio,transcription,refinement,delivery}/mod.rs` and
`application/controller.rs`. The controller depends only on those traits, and its tests in
`application/controller.rs` run the full lifecycle with fakes.

## State machine

`application/state.rs` defines `DictationState`: `Idle | Recording | Transcribing | Refining | Completed | Error`.
Allowed transitions are a pure function, `can_transition`, and are unit-tested.

- **No overlap.** `start` is rejected unless the state is resting. While processing, the hotkey is ignored.
- **Stale results.** Each recording and retry gets a fresh session id. A result is applied only if the
  state is still `Transcribing{that session}`, so cancel cannot be overwritten by a late result.
- **Retry.** Audio is kept in memory only after a retryable failure, and is cleared on success, cancel,
  dismiss and new recordings. Audio is never written to disk.
- **Recording cap.** Recordings are capped at 2 minutes. Capture also has a separate 130 s memory cap.

The UI is a pure renderer of the state. It fetches it once (`get_state`) and then follows events.

## Windows / OS integration (`src-tauri/src/platform`)

- **`bar` window.** Transparent, undecorated, always on top, skip-taskbar, non-focusable.
  `focusable(false)` was **not enough**: Windows still activated the bar on `show()`, so measured focus
  moved to the bar. `show_no_activate` adds `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` and shows the window
  with `SW_SHOWNOACTIVATE`. After that fix, focus stayed in Notepad across hotkey presses and real mouse
  clicks on the bar (verified by script).
- **Placement.** The bar sits bottom-centre of the monitor's **work area**, which excludes the taskbar
  wherever it is docked. The pure maths are in `bar_origin` and unit-tested.
- **Insertion** (`delivery/paste.rs`, `platform/input.rs`).
  - `GetForegroundWindow` is captured when dictation starts.
  - When the text is ready, VoiceFlow pastes only if that same window is still in the foreground and isn't a
    VoiceFlow window. It then waits for Ctrl/Shift/Alt/Win to be released, so Ctrl+V can't turn into
    another shortcut.
  - It writes the clipboard, re-checks the foreground, sends Ctrl+V, and after 600 ms restores the
    previous clipboard text, unless the user copied something else in the meantime.
  - Any refusal falls back to leaving the text on the clipboard (`Completed.pasteError`).
  - Known limits: elevated (admin) windows and some secure fields ignore synthetic input, which Windows
    doesn't report. Only plain-text clipboard contents can be restored.
- **End-to-end test build.** The `e2e-audio` cargo feature plus `VOICEFLOW_E2E_WAV=<wav>` replaces the
  microphone with a WAV file, so hotkey → Whisper → Gemini → paste can be tested without a voice. It is
  not compiled into normal builds.
- **Two shortcuts, two intents.** The dictation shortcut (default Caps Lock) and the content-creation
  shortcut (default Ctrl+Space) are registered together by `hotkey::register_all`.
  - Plugin shortcut ids map to an `Intent`.
  - The session's intent is stored, so Retry repeats the same kind of request.
  - In push-to-talk mode, releasing one shortcut only stops a recording started by that same shortcut.
- **Hotkey.** The default is **hold Caps Lock** (push-to-talk), implemented in `platform/caps_lock.rs`
  with a `WH_KEYBOARD_LL` hook.
  - `RegisterHotKey` can't use Caps Lock cleanly because each press would still toggle capitals.
  - The hook swallows plain Caps Lock and reports one press and one release edge, ignoring auto-repeat.
  - Shift, Ctrl or Alt + Caps Lock passes through, so capitals can still be toggled.
  - The hook only compares the virtual-key code. No other keystroke is read, stored or logged.
  - Edges are handled on a separate thread, because Windows drops hooks that respond slowly.
  - Other shortcuts (modifier combos) use `tauri-plugin-global-shortcut`, in toggle or push-to-talk mode.
    Bare keys other than Caps Lock are rejected.
  - A registration failure is shown in the main window, and dictation still works from the bar's button.
- **Startup focus.** Every window is created unfocused unless it is shown. Tauri's default
  `focused: true` let the *hidden* main window take the foreground at launch. That stole focus from the
  user's app, and the Caps Lock hook received no events for about 2-6 s while the webview loaded.

## Shared vs OS-specific

| Shared (all platforms) | OS-specific |
|---|---|
| state machine, controller, traits, settings store, model registry/download, whisper provider, VAD, resampling, React UI | `platform/` (hotkey, overlay window, focus), cpal host backend, `delivery/` insertion (Phase 2), mobile native modules (Phase 4+) |

## Data locations (Windows)

- Settings: `%APPDATA%\com.voiceflow.desktop\settings.json`. Saves are atomic. Missing fields fall back to
  defaults; a corrupt file is moved to `.bak` and defaults are used.
- Models: `%APPDATA%\com.voiceflow.desktop\models\`.
- No history, audio or transcripts are stored. Logs contain only state transitions, timings and audio
  levels, never text or audio.

## Network

The only network call is the user-initiated model download. It uses pinned Hugging Face URLs for the
official `ggerganov/whisper.cpp` conversions, and checks SHA-256 against hashes taken from the Hugging Face
LFS pointers.

Optional refinement (`refinement/gemini.rs`) sends transcript text, never audio, to
`generativelanguage.googleapis.com` (`models/{model}:generateContent`):
- The key goes in the `x-goog-api-key` header, so it cannot appear in URLs or error text. It is read
  from Windows Credential Manager (`platform/secrets.rs`) on each call.
- Requests time out after 10 s, with temperature 0.2.
- The prompt treats the dictation as text to edit, not instructions. A length check rejects replies that
  answer the dictation instead of editing it.
- Every failure falls back to the raw transcript (`Completed.refinementNote`). The controller shows
  `Refining` only while a call is in flight, and cancelling discards the result.
