# LiveDub Voice

Open source Windows desktop app for **real-time voice translation**.

- **Product name:** LiveDub Voice
- **Repository / package name:** `livedub-voice`
- **Binary:** `livedub-voice.exe`

This file holds the project requirements, decisions and conventions so that Claude and any contributor work from the same context.

## Vision

A simultaneous interpreter for calls and games. When other people talk (Google Meet, Microsoft Teams, Discord, in-game voice chat), the user hears them in their own language. When the user talks, the others hear the user in their language — without installing anything on their side.

## Scope — v1

- **Platform:** Windows 10 (2004+) and Windows 11 only.
- **Languages:** any source language (auto-detected by the model) → any target language the model supports: Chinese (`zh`), English (`en`), French (`fr`), German (`de`), Hindi (`hi`), Indonesian (`id`), Italian (`it`), Japanese (`ja`), Korean (`ko`), Portuguese (`pt`), Russian (`ru`), Spanish (`es`), Vietnamese (`vi`). Each direction picks its own target. List from OpenAI's [realtime translation guide](https://developers.openai.com/cookbook/examples/voice_solutions/realtime_translation_guide) (codes from its official sample apps).
- **Panel (UI) language:** English and Spanish only (separate from the translation languages).
- **Translation engine:** OpenAI `gpt-realtime-translate` only.
- **Bring your own key:** each user configures their own OpenAI API key.

Out of scope for v1: macOS/Linux, panel translations beyond English/Spanish, voice selection, other AI providers.

## Requirements

### Functional

1. **Incoming translation:** capture the audio of other participants (a specific app such as Discord/Teams/browser/game, or the whole system minus LiveDub Voice) and play the translation through the user's headphones.
2. **Outgoing translation:** capture the user's microphone, translate it, and send it to a virtual microphone that the call/game app uses as its input.
3. **OpenAI configuration:** the user enters and validates their OpenAI API key from the panel.
4. **Configuration panel:** app (panel) language — System, English or Spanish; API key, target language of each direction, input/output devices, source app to capture, original-audio volume (ducking), voice detection (don't send silence, auto-off), whether closing the window quits, start/stop each direction.
5. **Usage panel:** minutes translated and estimated cost, per session and historical, plus an optional daily/monthly spending cap that stops the translations.
6. **Live captions:** show the transcript of the translations in the panel, plus a "speaking / listening" indicator per direction.
7. **Cost saving:** local voice detection stops sending audio while nobody speaks; a direction that hears no speech for N minutes (default 5, can be off) turns itself off.
8. **System tray:** closing the window hides the app to the tray (like Discord) with the translations still running; the tray menu opens the window, starts/stops each direction and quits.
9. **README:** how to install and use it (Meet, Teams, Discord, games), license, and how to contribute.

### Non-functional

- Low latency: the audio pipeline must not add noticeable delay on top of the model (~1–2 s expected end to end).
- The API key is stored in Windows Credential Manager, never in plain text or logs. The panel never reads it back: it only gets whether a key is set and its last four characters. The key is checked with a free `GET /v1/models/gpt-realtime-translate` (no tokens spent), so a key on an account without credits still passes until audio is streamed.
- No telemetry. Audio goes only to OpenAI.
- No game process injection (must be safe with anti-cheat systems).
- Low CPU/RAM footprint while gaming.

## Key technical facts

- **Model:** `gpt-realtime-translate` — WebSocket `wss://api.openai.com/v1/realtime/translations?model=gpt-realtime-translate`.
  - Target language set via `session.audio.output.language` (one of the 13 codes above, e.g. `"es"`); source language is auto-detected. Rust `settings::Language` and panel `src/lib/languages.ts` hold the list; an unknown code in `settings.json` falls back to that direction's default.
  - Auth: `Authorization: Bearer <key>` on the WebSocket handshake. A wrong key does not fail the handshake: the server sends an `error` event with code `invalid_api_key`.
  - Input: PCM16 mono 24 kHz little-endian, base64, via `session.input_audio_buffer.append` (send silence too). Append in 200 ms chunks (4800 samples); shorter chunks are buffered, longer ones split.
  - Output: base64 PCM16 24 kHz mono via `session.output_audio.delta` (chunk length varies), translated text via `session.output_transcript.delta`. Optional delta fields: `sample_rate`, `channels`, `format` (`pcm16`) and `elapsed_ms` (alignment metadata, not an id); they are parsed leniently and recorded, never needed to play the audio.
  - Observed output pacing (real sessions): the server streams one 400 ms delta (9600 samples) every ~400 ms while audio is appended, **digital silence included**, with ~4 s from start of speech to translated speech. After `session.close` it flushes the translated tail and then many seconds of zeros (~25 s after a 16 s session) as fast as it can. The jitter buffer therefore skips digital silence queued behind speech (keeping a 300 ms pause), the stop ends after 5 s of post-close silence without waiting for `session.closed`, and latency is measured on audible deltas only.
  - Source-language captions (`session.input_transcript.delta`) only arrive when `session.audio.input.transcription` is set (model `gpt-realtime-whisper`).
  - Graceful end: send `session.close`, keep reading until `session.closed`, then close the socket.
  - Input noise reduction: `session.audio.input.noise_reduction` (`near_field` / `far_field`); the app sends `near_field`.
  - On stop the app sends ~1.5 s of trailing silence before `session.close` and plays the translated tail before releasing the cable (state `stopping`, capped at 10 s).
  - Docs: [guide](https://developers.openai.com/api/docs/guides/realtime-translation), [client events](https://developers.openai.com/api/reference/resources/realtime/translation-client-events), [server events](https://developers.openai.com/api/reference/resources/realtime/translation-server-events).
  - No voice selection (voice adapts to the speaker), no custom prompts/glossaries, no turn lifecycle.
  - Speech already in the target language may produce silence → keep original audio mixed in (ducked).
  - Price: **$0.034 per audio minute** (both directions active and always streaming ≈ $4/hour).
  - **Billing (researched Nov 2026):** the [pricing page](https://developers.openai.com/api/docs/pricing) lists `gpt-realtime-translate` under Transcription as "$0.034 / minute" (column "Estimated cost", no input/output token prices), and no page says what the minute measures (appended input audio, session wall-clock or output), nor anything about silence, rounding or minimums. The [client events reference](https://developers.openai.com/api/reference/resources/realtime/translation-client-events) does say that "if a client stops sending audio and later resumes, model time treats the resumed audio as contiguous" — model time is driven by the audio appended (200 ms engine frames). The design assumes the minute is **audio appended** (so silence that is never appended is not billed) and meters exactly that; if it turns out to be session time, auto-off is what caps the cost. To check: the maintainer can compare the OpenAI usage dashboard with the Usage screen after a long, mostly silent session. The guide and cookbook still say to keep appending silence between phrases; the gate keeps ~1.2 s of it after each phrase, which is what the model needs to finish it.
  - Errors: the docs only document the `error` event's shape (`type`, `code`, `message`, `param`; "most errors are recoverable and the session will stay open") and no codes, so `TranslatorError` maps codes leniently (OpenAI's usual API codes) and splits them into **fatal** — `invalid_api_key`/401 (`openai_auth`), 403/404 (`openai_no_model_access`), `insufficient_quota`/billing (`openai_quota`), an invalid request about `language` (`openai_unsupported_language`), unexpected handshake statuses (`openai_protocol`) — and **retryable** — 429 `rate_limit_exceeded` (`openai_rate_limited`, with the `retry-after-ms`/`Retry-After` header or a "try again in 1.5s" message as the minimum wait), 5xx/`server_error` (`openai_server`), network errors and timeouts (`network`), other `error` codes (`openai_api`) and a session closed by the server. A fatal `error` event mid-session stops the direction; a retryable one is only remembered as the cause if the session then drops.
  - **Voice detection** (`pipeline/voice.rs`): per direction, on the 24 kHz mono input in 10 ms frames. Energy against a noise floor that is the quietest frame of the last 3 s (minimum statistics: speech has gaps, steady noise does not); loud = ≥ 9 dB above the floor and ≥ −50 dBFS; voice starts after 3 loud frames out of 5 and ends 300 ms after the last loud one. With `pauseWhenSilent` (default on) the gate starts paused, appends voice plus ~1.2 s after it (ending on a 200 ms chunk boundary), then stops appending and keeps a 300 ms pre-roll that is appended first when voice starts again. Off = everything is appended as before (the detector still drives the indicator and auto-off). A stop while paused skips the 1.5 s trailing silence. Steady loud sound (music in a game) counts as voice: it costs what streaming everything did. The session stays open while paused, kept alive by the pings. **Unverified:** whether OpenAI keeps a session that receives no appends for minutes (the stored key had expired when this was built). If it drops it, the drop is retried like any other (`reconnecting`, new session). `translator::openai::tests::probe_idle_session` (ignored) measures it with a real key and two speech WAVs.
  - **Auto-off:** the gate counts samples since the last voice (the audio clock, exact and testable; capture keeps flowing while paused or reconnecting) and reports `NoSpeech` once after `autoOffMinutes`; the commands layer stops the direction through `PipelineManager::stop` (graceful) and emits `{direction}-auto-off` (`{ minutes }`), shown on Home until the next start.
  - Connection liveness: a ping every 5 s; no message from the server (deltas or pongs) for 10 s, a send that takes 10 s, a socket error or a close the client did not ask for ends the session with a `network` error. Sessions also carry `expires_at`; an expired session drops and is reconnected like any other.
  - **Reconnect** (`pipeline/reconnect.rs`): after a retryable drop mid-session the pipeline keeps the devices open and the apps ducked, discards the captured audio (never queued), plays out what already arrived, and opens a new session after 1, 2, 4, 8, 16, 16 s (6 attempts, longer if OpenAI asked) — state `reconnecting` with `{ attempt, maxAttempts }`. The count only starts over after a session stays up 30 s, so a server that keeps dropping sessions runs out of attempts (`reconnect_failed`). A start that fails is not retried: the user sees the error at once.
- **Incoming capture:** WASAPI _process loopback_ (`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`), either including a target process tree or excluding our own process tree (avoids feedback loops). No driver needed. `wasapi` 0.27 wraps it (`AudioClient::new_application_loopback_client`); the `windows` crate is only used for the process snapshot, executable descriptions, exit checks and per-app session volumes (ducking).
  - The source picker lists apps from the audio sessions (`IAudioSessionManager2`) of every active playback device, grouped by app: a session belongs to the top of its tree, climbing through parents with the same executable (Chrome, Discord) or through WebView2/CEF helpers (new Teams). Settings store the root's executable name (`SourceApp::Process { name }`, compared without case); it is resolved to a PID when capture starts (`source_app_not_running` if no process of it runs).
  - The loopback device has no mix format: the app asks for 48 kHz stereo `f32` (autoconvert) and resamples to 24 kHz mono PCM16 with `rubato` like the mic. Silence keeps flowing while the app is quiet. Capturing one app stops with an error when its root process exits.
  - Process loopback captures each app **after** its session volume (measured: an exact linear gain on the `f32` samples). LiveDub Voice's own process tree is never captured, so a source app must not be a child of LiveDub Voice.
- **Ducking (incoming):** while incoming runs, the captured sessions' `ISimpleAudioVolume` (the Windows volume mixer) is set to `original × level` (`originalVolume`, floored at 0.5 % because of the compensation), and the pipeline multiplies the captured audio by `1 / level` so the translator still hears the app at full volume. Apps are ducked after the translator connects and before their capture starts, and restored after the translated tail played (also on errors and on app exit). Sessions are rescanned every 250 ms; in whole-system mode every session except LiveDub Voice's is ducked (system sounds included). Each ducked volume is written to `ducking.json` before it changes; whatever could not be restored (crash, app closed) is restored at the next launch. A volume the user changes by hand while ducked is left alone. A running translation applies a new `originalVolume` when settings are saved.
- **Outgoing output:** Windows cannot create a virtual microphone without a driver. The app writes translated audio to a virtual cable's playback endpoint; the user selects the cable's recording endpoint as mic in the call/game app.
  - **Driver strategy (v1): driver-agnostic.** LiveDub Voice does not ship or install any driver. The outgoing pipeline writes to whatever output device the user picks. On startup the panel detects known virtual cables (VB-CABLE, Virtual-Audio-Driver, VoiceMeeter, …) and, if none is found, guides the user to install one. Bundling Virtual-Audio-Driver (MIT) as an optional installer component may be revisited later.
- **Usage data:** the OpenAI Usage/Costs API requires an _Admin key_, so usage is metered locally (`usage/`). Optional Admin key support may come later.
  - **What counts as a billed minute:** the app counts the seconds of audio appended to the session (the pipeline's `AudioSent` reports; see "Billing" above for why). With voice detection that is speech, its pre-roll and the ~1.2 s after each phrase (plus the 1.5 s trailing silence when a stop interrupts speech); silence that is not appended is not counted, and neither are reconnect gaps. With pausing off it equals the wall-clock time the session is open and streaming. Cost = seconds / 60 × `PRICE_PER_MINUTE_USD` (0.034, the one constant in the core; `src/lib/pricing.ts` mirrors it and a test checks they match). The panel always says it is an estimate, not the OpenAI bill.
  - **History:** `usage.json` (versioned, `USAGE_VERSION`) holds seconds and sessions per local calendar day and direction (the user's time zone when the audio was reported; a session across midnight is split), plus `capOverrideDate`. Written atomically every 30 s while it changes, when a direction stops and on exit, so a crash loses at most ~30 s. A corrupt or newer-version file is moved to `usage.json.bak`. Dates use `chrono` behind a mockable `Clock`.
  - Events: `usage-updated` (`UsageSnapshot`: current session per direction, today, this month, days newest first, cap status) about once a second per running direction; command `get_usage`.
- **System tray and closing** (`tray.rs`): a tray icon (the app's window icon, Tauri `tray-icon` feature). Left click opens or focuses the window; the menu has Open, one check item per direction showing its state ("Incoming (others → you): Live", click to start/stop through the same path as the panel's commands), and Quit. Closing the window: `closeQuitsApp` (default off) quits; otherwise the window hides and the translations keep running. The first close keeps the window open and the panel explains the tray (`close-to-tray-notice` event; "hide" sets `trayNoticeShown`, which the core owns: a panel save never resets it). Quit (tray, Settings, or the notice) is `app.exit(0)`, which goes through the usual exit shutdown. The menu's strings live in `src/i18n` like every string: the panel sends them in its language (`set_tray_labels`) at startup and when the language changes; until then the menu is English. Menu updates are queued on the main thread, never awaited (pipeline events can arrive while the main thread waits for them at exit).
- **Spending cap:** `spendingCap: { dailyUsd, monthlyUsd }` in `settings.json` (since version 3; `null` = off, the default; 0 < cap ≤ 10 000). Version 4 adds `voiceActivity: { pauseWhenSilent, autoOffMinutes }` (on, 5; 1–240 or `null`), `closeQuitsApp` (false) and `trayNoticeShown` (false). From 80 % Home and Usage warn; at 100 % the meter emits `spending-cap-reached` and the running directions stop gracefully through `PipelineManager::stop_all` (tail played, ducking restored). Starting while the cap is reached fails with `spending_cap_reached` (no status event) and the panel asks first: "Keep going today" (`override_spending_cap`, valid until local midnight) or "Raise the cap" (Usage screen). The real safety net is a budget in OpenAI's dashboard, linked from the Usage screen. The usage counted while a stop finishes can slightly exceed the cap.

- **App icon** (`src-tauri/icons/`): two speech bubbles with voice bars, teal (others) behind coral (you), on a navy tile (Ruta dark surface). Masters: `icon.svg` (32 px and up) and `icon-small.svg` (no bars, for the 16/20/24 px entries of `icon.ico`); `pnpm icons` (`scripts/icons.mjs`, Node + `tauri icon` only) regenerates every PNG/ICO/ICNS and the installer bitmaps. The tray loads the embedded app icon at `SM_CXSMICON` so it gets the small drawing; the panel's `LogoMark` is the same mark in theme tokens.
- **Installer:** NSIS only, built by the Tauri bundler (`pnpm tauri build` → `src-tauri/target/release/bundle/nsis/LiveDub Voice_<version>_x64-setup.exe`). Per-user install (`installMode: currentUser`, no admin) into `%LOCALAPPDATA%\LiveDub Voice`, with a language selector (English, Spanish), a Start menu shortcut and a desktop shortcut (a checkbox on the finish page). WebView2: `downloadBootstrapper` (fetched only when missing). **Unsigned** for now: SmartScreen warns on first run ("More info → Run anyway", documented in the README). Installing over a running app and uninstalling both offer to close it first (Tauri's template). Uninstall **keeps** `%APPDATA%\io.github.fatuking.livedubvoice` (settings, usage) and the API key by default; the uninstaller's "Delete the application data" checkbox removes that folder, `%LOCALAPPDATA%\io.github.fatuking.livedubvoice` (logs) and the Credential Manager entry (`installer/hooks.nsh`). Updates never delete data. No release workflow yet.

## Tech stack

- **App shell:** Tauri 2.
- **Core (audio + OpenAI client):** Rust — `tokio`, `tokio-tungstenite`, `wasapi` (WASAPI bindings), `rubato` (resampling), `keyring` 3 with the `windows-native` backend (Credential Manager), `reqwest` 0.12 with rustls and the Windows root store (`rustls-tls-native-roots`, no OpenSSL) as the HTTP client, `serde`/`serde_json` (settings and usage history), `chrono` (local days and months for usage and the spending cap). _Crates not yet in `Cargo.toml` are proposals; confirm when implementing._
- **Persistence:** no database. Settings and usage history are plain JSON files in Tauri's app config dir (`%APPDATA%\io.github.fatuking.livedubvoice`: `settings.json`, `usage.json`, plus `ducking.json` while app volumes are ducked); the API key lives only in Credential Manager.
- **Panel (frontend):** React + TypeScript + Vite. Plain CSS with design tokens (no UI framework, Tailwind or CSS-in-JS); Plus Jakarta Sans bundled via `@fontsource-variable`, icons from `lucide-react`. Nothing loaded from a CDN.
- **Testing:** `cargo test` for the core, Vitest for the panel. `LIVEDUB_FAKE_TRANSLATOR=1` (replay) or `=echo` swaps OpenAI for a free local fake translator. `LIVEDUB_RECORD_DIR=<folder>` records each session of either direction (WAVs + events) locally for diagnosing audio quality.
- **CI/CD:** GitHub Actions on `windows-latest` (fmt, clippy, tests, build, release installer).

## Architecture

```
            ┌──────────────────────── Rust core ────────────────────────┐
 Other app ─► process loopback ─► resample 24k ─► Translator (→ user lang) ─► headphones (+ ducked original)
 Microphone ─► WASAPI capture  ─► resample 24k ─► Translator (→ their lang) ─► virtual cable ─► call/game app
            └──────────── events (captions, usage, status) ─► Tauri IPC ─► React panel ┘
```

- Each direction is an independent pipeline with its own OpenAI WebSocket session; both run on the same engine (`pipeline/translation.rs`), with the direction-specific config and device checks in `pipeline/incoming.rs` / `pipeline/outgoing.rs`.
- **Lifecycle** (`pipeline/manager.rs`): `PipelineManager` is the one owner of both directions (the panel's commands, the tray menu, the spending cap and auto-off all go through it). States: `idle`, `connecting` (starting), `live` (running), `reconnecting`, `stopping`, `error` (with a code). Start and stop are idempotent per direction (the slot stays locked for the whole start or stop, so double clicks never open two sessions). Settings changes apply on the next start (only the original-audio volume applies live; the Settings screen says so while a direction runs). On exit (Quit, the window closed with `closeQuitsApp`, Ctrl+C in dev) both directions stop fast — `session.close`, at most 1 s for the session, WASAPI threads joined, cable released, ducked apps restored — within 5 s, then `ducking.json` is replayed as a last resort. Panics are written to the log file.
- The panel never touches audio or the API key directly; it talks to the core via Tauri commands/events.

## UI design

- The panel uses the **Ruta** design system, documented in [`docs/design.md`](docs/design.md). Read it before touching panel UI.
- All panel UI uses the Ruta tokens (`src/styles/tokens.css`) and components (`src/components/`); no hard-coded colors or one-off styles in screens.
- Color meanings are fixed: **incoming (others → you) = teal**, **outgoing (you → others) = coral**, sun/amber = warnings and highlights, red = errors.
- Dark and light themes follow the OS; text must meet WCAG AA in both.
- The panel UI is available in **English and Spanish** (`uiLanguage` setting: `system` follows the Windows display language, `es*` → Spanish, anything else → English). Every new user-visible string (labels, hints, aria-labels, empty states, notices, error texts) goes through the dictionaries in `src/i18n/` (`en.ts` is the source of truth, `es.ts` must have the same keys) — in **both** languages, never hard-coded in a component. Core error codes get a translated message in `errors`. Code, comments, commits and docs stay in English.

## Conventions

- **Everything in English:** code, comments, commits, README, issues. (`TASK.md` is personal and may be in Spanish.)
- Commits follow [Conventional Commits](https://www.conventionalcommits.org/).
- License: **MIT**.
- Rust: `cargo fmt` + `cargo clippy -D warnings`. TS: ESLint + Prettier.

## Workflow

- Product tasks live in `TASK.md` at the repo root. **That file is local and never pushed to GitHub** (it is in `.gitignore`). Never stage or commit it.
- **One task = one Claude session = one pull request.** A planning session decides scope; each task from `TASK.md` is implemented in its own session.
- **Every change goes through a PR.** Never push directly to `main`. (Only exception: the initial scaffold commit that created `main`.)
  - Branch: `<type>/<short-slug>` using the Conventional Commits type of the PR (e.g. `feat/process-loopback-capture`, `chore/lint-tooling`). Never reference `TASK.md` ids in branches, commits or PRs: contributors can't see that file.
  - PR title follows Conventional Commits (e.g. `chore: add license and base repo files`); the body says what changed, why, and how it was tested.
  - Merge strategy: **squash merge** only (the repo allows nothing else; the PR title becomes the commit, branches are deleted on merge), done by the maintainer after review.
- When a task is done, its session reports back; `TASK.md` is updated in the planning session.
- When a decision changes the requirements, update this `CLAUDE.md` (in the task's PR).
