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
- **Languages:** Spanish ⇄ English only.
- **Translation engine:** OpenAI `gpt-realtime-translate` only.
- **Bring your own key:** each user configures their own OpenAI API key.

Out of scope for v1: macOS/Linux, other languages, voice selection, other AI providers.

## Requirements

### Functional

1. **Incoming translation:** capture the audio of other participants (a specific app such as Discord/Teams/browser/game, or the whole system minus LiveDub Voice) and play the translation through the user's headphones.
2. **Outgoing translation:** capture the user's microphone, translate it, and send it to a virtual microphone that the call/game app uses as its input.
3. **OpenAI configuration:** the user enters and validates their OpenAI API key from the panel.
4. **Configuration panel:** API key, language direction, input/output devices, source app to capture, original-audio volume (ducking), start/stop each direction.
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
  - Target language set via `session.audio.output.language` (`"es"` / `"en"`); source language is auto-detected.
  - Input: PCM16 mono 24 kHz, base64, via `session.input_audio_buffer.append` (send silence too).
  - Output: PCM16 in 200 ms chunks via `session.output_audio.delta`, plus transcript deltas.
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
- **Testing:** `cargo test` for the core, Vitest for the panel.
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
