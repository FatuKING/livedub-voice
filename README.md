# LiveDub Voice

[![CI](https://github.com/FatuKING/livedub-voice/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/FatuKING/livedub-voice/actions/workflows/ci.yml)

**Real-time voice translation for calls and games on Windows.** Hear everyone in your language, and let them hear you in theirs — without them installing anything.

> [!WARNING]
> 🚧 **Early development — not usable yet.** There is no release and the app does not translate anything yet. Watch or star the repository to follow progress.

## What it does

LiveDub Voice works like a simultaneous interpreter that sits between you and your call or game app (Google Meet, Microsoft Teams, Discord, in-game voice chat):

- **Incoming translation** — captures the audio of the other participants (a specific app, or the whole system minus LiveDub Voice) and plays the translation in your headphones, with the original audio mixed in at a lower volume.
- **Outgoing translation** — captures your microphone, translates what you say, and sends it to a virtual microphone that your call or game app uses as its input.

```
Other people ─► LiveDub Voice ─► translated into your language ─► your headphones
Your mic     ─► LiveDub Voice ─► translated into their language ─► virtual cable ─► call/game app
```

Translation is done by OpenAI's `gpt-realtime-translate` model, using your own API key. Each direction is an independent stream, and the expected end-to-end delay is about 1–2 seconds.

## Planned features (v1)

- Spanish ⇄ English translation, in both directions at the same time.
- Choose which app to listen to (Discord, Teams, a browser, a game…) or capture the whole system.
- Works with any virtual audio cable for the outgoing microphone.
- Adjustable volume of the original audio (ducking).
- Live captions of the translations.
- Usage panel: minutes translated and estimated cost, per session and historical.
- Your API key is stored in Windows Credential Manager, never in plain text.
- No telemetry: audio goes only to OpenAI. No game process injection, so it is safe with anti-cheat systems.

## Requirements

- **Windows 10 (version 2004 or later) or Windows 11.**
- **An OpenAI API key** with access to `gpt-realtime-translate`.
- **A virtual audio cable** for outgoing translation, such as [VB-CABLE](https://vb-audio.com/Cable/). LiveDub Voice does not install any driver; it detects common cables and guides you if none is found.

### Cost

You pay OpenAI directly for what you use. `gpt-realtime-translate` costs **$0.034 per audio minute, per direction**. With both directions active that is roughly **$4 per hour**. LiveDub Voice estimates your usage locally in the usage panel.

## Installation

> **TODO** — written once the first installer is released.

## Usage (Meet / Teams / Discord / games)

> **TODO** — step-by-step setup guides for Google Meet, Microsoft Teams, Discord and games.

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) 20 or later
- [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/) stable with the MSVC toolchain (`stable-x86_64-pc-windows-msvc`)
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload
- WebView2 runtime (preinstalled on Windows 11 and recent Windows 10)

See the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/) for details.

### Run

```bash
pnpm install
pnpm tauri dev
```

The app is built with [Tauri 2](https://v2.tauri.app/): a Rust core handles audio and the OpenAI connection, and a React + TypeScript panel handles configuration, captions and usage.

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening an issue or pull request.

## License

[MIT](LICENSE) © 2026 Facundo Cozzani
