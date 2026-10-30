# VoiceFlow Desktop

Local, private voice dictation. Press a global shortcut, speak, and the transcript is on your clipboard.
Speech recognition runs entirely on your computer (whisper.cpp).

**Status: Phase 1, a Windows proof of concept.** It has only been built and tested on Windows 11.
macOS, Android and iOS have **not** been built or tested.

## Use

1. Launch VoiceFlow. A small bar appears above the taskbar.
2. On first run, the main window opens. Download a speech model there (Base, 148 MB, is recommended).
3. Click where you want to type in any app, **hold Caps Lock** while you speak, and release it.
   The text is **pasted at your cursor**, and your previous clipboard text is put back.
   - If you switch windows before it finishes, VoiceFlow won't paste into the wrong place. It leaves the
     text on the clipboard so you can press Ctrl+V yourself.
   - Pasting doesn't work into apps running as administrator (Windows blocks it) or into some password
     fields. The text is then on the clipboard instead.
   - While VoiceFlow runs, Caps Lock no longer toggles capitals. Use **Shift+Caps Lock** for that.
   - You can switch to a key combination such as Ctrl+Shift+Space in Settings, in toggle mode
     (press to start, press again to stop) or hold-to-talk mode.
4. Close the main window to keep VoiceFlow running in the bar. Use **Quit VoiceFlow** to exit.

### Content creation (Ctrl+Space)

Hold **Ctrl+Space** and say what you want written, for example *"create a 50% off discount email"*.
Gemini writes the content and it is pasted at your cursor. Emails start with a `Subject:` line. Details
you didn't give become placeholders such as `[Date]` or `[Promo Code]`; nothing is invented.

- This needs a Gemini API key (see AI refinement below).
- If generation fails, **nothing is pasted**. Pasting your instruction into an email would be wrong. The
  bar offers **Retry**, which reuses the same recording.
- Ctrl+Space is also code completion in most IDEs and an input-language switch in Windows. While
  VoiceFlow runs it takes over that shortcut. Change it in Settings (for example to Ctrl+Alt+Space), or
  leave it empty to turn content creation off.

Settings in the main window cover both shortcuts, toggle vs push-to-talk, model, paste/clipboard behaviour, and bar visibility.

## Develop

Prerequisites (Windows): Node 22+, Rust (stable MSVC), Visual Studio C++ Build Tools, CMake, and LLVM.
LLVM is needed because whisper-rs uses bindgen.

```powershell
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
npm install
npm run dev            # tauri dev (Vite on :1420)
npm test               # UI tests (vitest)
npm run test:rust      # Rust unit + integration tests
npm run build          # release build + NSIS installer
```

The whisper integration tests need a model in `%APPDATA%\com.voiceflow.desktop\models`. Download it from the
app, or set `VOICEFLOW_MODELS_DIR`; the tests are skipped if no model is present. Select the model with
`VOICEFLOW_TEST_MODEL=base.en`. The network download test runs only on request:
`cargo test --lib -- --ignored downloads_and_verifies`.

## Layout

```
apps/desktop-ui/        React + TS: bar.html (floating pill) and index.html (main window)
packages/shared-types/  TS mirror of the Rust state/settings types
src-tauri/src/
  application/          state machine + controller (trait-based, fully unit-tested)
  audio/                cpal capture, resampling, level gate, Silero VAD
  transcription/        SpeechProvider trait, whisper.cpp provider, model registry/download
  refinement/           RefinementProvider trait (Verbatim only in Phase 1)
  delivery/             TextDelivery trait, clipboard
  storage/              settings (atomic JSON)
  platform/             overlay window, hotkey, focus (Windows-specific code lives here)
src-tauri/resources/    bundled Silero VAD model (MIT)
tests/fixtures/         jfk.wav (from whisper.cpp samples)
docs/                   architecture, risks, benchmarks, mobile feasibility
```

## AI refinement (optional, Google Gemini)

Paste a Gemini API key in the main window under **AI text refinement**, then click **Save & test**.
After that, each transcript is sent to Gemini to fix grammar and make it read clearly before it is
copied. Choose **Clean**, **Polished** (the default), **Developer**, or **Off**.

- The key is stored in Windows Credential Manager under `VoiceFlow/gemini-api-key`. It is never written
  to settings files or logs, and never shown in the UI again.
- If Gemini is slow (more than 10 s), offline, rejects the key, or answers your text instead of editing it,
  you get the original transcript and a note explaining why.

## Privacy

- Audio is held in memory only and discarded after transcription. It is never written to disk or sent anywhere.
- With AI refinement on, the transcript **text** is sent to Google Gemini. With it off or with no key
  set, nothing leaves the machine.
- There is no transcript history in Phase 1.
- Other network access: the model download you start. Its SHA-256 is verified against pinned hashes.
- Logs record state changes, timings and audio levels, never audio or text.

## Licences

- whisper.cpp and the Whisper models: MIT.
- Silero VAD: MIT.
- Tauri: MIT/Apache-2.0.
