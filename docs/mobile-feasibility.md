# Mobile feasibility notes (Phase 4/5 input; nothing here is built or tested)

## What carries over

- The Rust core: controller, traits, settings, model registry, VAD and whisper provider. whisper.cpp
  supports iOS (Metal/CoreML) and Android (NEON).
- The React UI for the recording screen, transcript preview and settings, through Tauri 2 mobile targets.

## What does not

- **Global hotkeys.** These don't exist on mobile, and the global-shortcut plugin is desktop-only.
- **Floating overlay.**
  - iOS: not possible.
  - Android: possible only with `SYSTEM_ALERT_WINDOW`, which has Play policy implications. Treat it as optional.
- **Typing into other apps.** Mobile needs a keyboard:
  - **Android IME (`InputMethodService`).** Native Kotlin. It can record audio (it needs `RECORD_AUDIO`
    granted to the app) and commit text through `InputConnection`. This is the most practical route to
    system-wide dictation on Android. It should call the shared Rust core through JNI.
  - **iOS custom keyboard extension.** Native Swift. Keyboard extensions **cannot access the microphone**.
    The usual workaround is for the keyboard to open the containing app, which records and transcribes,
    then share the result through an App Group, after which the user switches back. Memory limits for
    extensions (~50-70 MB) also rule out running Whisper inside the extension.
- **Clipboard.** Verify `tauri-plugin-clipboard-manager` support on each mobile target. Otherwise, fall back
  to the native share sheet.

## Spike checklist (Phase 4)

1. Build `tauri android init` / `tauri ios init` with the shared core. Check whisper-rs cross-compiles (NDK, Xcode).
2. Measure tiny.en/base.en RTF on a mid-range Android phone and an iPhone.
3. Write an Android IME prototype that commits a fixed string, then real dictation.
4. Write an iOS keyboard + App Group round-trip prototype, and document the UX cost.
