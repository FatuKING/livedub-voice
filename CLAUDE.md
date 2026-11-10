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
4. **Configuration panel:** app (panel) language — System, English or Spanish; API key, target language of each direction, input/output devices, source app to capture, original-audio volume (ducking), start/stop each direction.
5. **Usage panel:** minutes translated and estimated cost, per session and historical.
6. **Live captions:** show the transcript of the translations in the panel.
7. **README:** how to install and use it (Meet, Teams, Discord, games), license, and how to contribute.

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
  - Output: base64 PCM16 24 kHz mono via `session.output_audio.delta` (chunk length varies), translated text via `session.output_transcript.delta`.
  - Source-language captions (`session.input_transcript.delta`) only arrive when `session.audio.input.transcription` is set (model `gpt-realtime-whisper`).
  - Graceful end: send `session.close`, keep reading until `session.closed`, then close the socket.
  - Docs: [guide](https://developers.openai.com/api/docs/guides/realtime-translation), [client events](https://developers.openai.com/api/reference/resources/realtime/translation-client-events), [server events](https://developers.openai.com/api/reference/resources/realtime/translation-server-events).
  - No voice selection (voice adapts to the speaker), no custom prompts/glossaries, no turn lifecycle.
  - Speech already in the target language may produce silence → keep original audio mixed in (ducked).
  - Price: **$0.034 per audio minute** (both directions active ≈ $4/hour).
- **Incoming capture:** WASAPI _process loopback_ (`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`), either including a target process tree or excluding our own process tree (avoids feedback loops). No driver needed.
- **Outgoing output:** Windows cannot create a virtual microphone without a driver. The app writes translated audio to a virtual cable's playback endpoint; the user selects the cable's recording endpoint as mic in the call/game app.
  - **Driver strategy (v1): driver-agnostic.** LiveDub Voice does not ship or install any driver. The outgoing pipeline writes to whatever output device the user picks. On startup the panel detects known virtual cables (VB-CABLE, Virtual-Audio-Driver, VoiceMeeter, …) and, if none is found, guides the user to install one. Bundling Virtual-Audio-Driver (MIT) as an optional installer component may be revisited later.
- **Usage data:** the OpenAI Usage/Costs API requires an _Admin key_, so usage is metered locally (minutes per session × price). Optional Admin key support may come later.

## Tech stack

- **App shell:** Tauri 2.
- **Core (audio + OpenAI client):** Rust — `tokio`, `tokio-tungstenite`, `wasapi` (WASAPI bindings), `rubato` (resampling), `keyring` 3 with the `windows-native` backend (Credential Manager), `reqwest` 0.12 with rustls and the Windows root store (`rustls-tls-native-roots`, no OpenSSL) as the HTTP client, `serde`/`serde_json` (settings and usage history). _Crates not yet in `Cargo.toml` are proposals; confirm when implementing._
- **Persistence:** no database. Settings and usage history are plain JSON files in Tauri's app config dir (`%APPDATA%\io.github.fatuking.livedubvoice`, e.g. `settings.json`); the API key lives only in Credential Manager.
- **Panel (frontend):** React + TypeScript + Vite. Plain CSS with design tokens (no UI framework, Tailwind or CSS-in-JS); Plus Jakarta Sans bundled via `@fontsource-variable`, icons from `lucide-react`. Nothing loaded from a CDN.
- **Testing:** `cargo test` for the core, Vitest for the panel. `LIVEDUB_FAKE_TRANSLATOR=1` (replay) or `=echo` swaps OpenAI for a free local fake translator.
- **CI/CD:** GitHub Actions on `windows-latest` (fmt, clippy, tests, build, release installer).

## Architecture

```
            ┌──────────────────────── Rust core ────────────────────────┐
 Other app ─► process loopback ─► resample 24k ─► Translator (→ user lang) ─► headphones (+ ducked original)
 Microphone ─► WASAPI capture  ─► resample 24k ─► Translator (→ their lang) ─► virtual cable ─► call/game app
            └──────────── events (captions, usage, status) ─► Tauri IPC ─► React panel ┘
```

- Each direction is an independent pipeline with its own OpenAI WebSocket session.
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
