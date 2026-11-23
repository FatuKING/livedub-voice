# LiveDub Voice

[![CI](https://github.com/FatuKING/livedub-voice/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/FatuKING/livedub-voice/actions/workflows/ci.yml)

**Real-time voice translation for calls and games on Windows.** Hear everyone in your language, and let them hear you in theirs — without them installing anything.

> [!WARNING]
> 🚧 **Early development.** There is no release yet: the first installer (v0.1.0) is on its way. Until then you can build it from source (see [Development](#development)). Watch or star the repository to follow progress.

## What it does

LiveDub Voice works like a simultaneous interpreter that sits between you and your call or game app (Google Meet, Microsoft Teams, Discord, in-game voice chat):

- **Incoming translation** — captures the audio of the other participants (a specific app, or the whole system minus LiveDub Voice) and plays the translation in your headphones, while the original audio keeps playing at a lower volume.
- **Outgoing translation** — captures your microphone, translates what you say, and sends it to a virtual microphone that your call or game app uses as its input.

```
Other people ─► LiveDub Voice ─► translated into your language ─► your headphones
Your mic     ─► LiveDub Voice ─► translated into their language ─► virtual cable ─► call/game app
```

Translation is done by OpenAI's `gpt-realtime-translate` model, using your own API key. Each direction is an independent stream, and the expected end-to-end delay is about 1–2 seconds.

## Planned features (v1)

- Speak and listen in any language: the source language is detected automatically, and you choose what each direction translates into — Chinese, English, French, German, Hindi, Indonesian, Italian, Japanese, Korean, Portuguese, Russian, Spanish or Vietnamese (every output language the model supports). Both directions can run at the same time.
- Panel available in English and Spanish.
- Choose which app to listen to (Discord, Teams, a browser, a game…) or capture the whole system.
- Works with any virtual audio cable for the outgoing microphone.
- Adjustable volume of the original audio (ducking).
- Live captions of the translations.
- Usage panel: minutes translated and estimated cost, per session and historical, with an optional spending cap.
- Voice detection: silence is not sent to OpenAI, and a direction that hears nobody for a while (5 minutes by default) turns itself off.
- Runs in the system tray like Discord: closing the window keeps the translations going, and the tray menu starts, stops or quits them.
- Your API key is stored in Windows Credential Manager, never in plain text.
- No telemetry: audio goes only to OpenAI. No game process injection, so it is safe with anti-cheat systems.

## Requirements

- **Windows 10 (version 2004 or later) or Windows 11.**
- **An OpenAI API key** with access to `gpt-realtime-translate`.
- **A virtual audio cable** for outgoing translation, such as [VB-CABLE](https://vb-audio.com/Cable/). LiveDub Voice does not install any driver; it detects common cables and guides you if none is found.

### Cost

You pay OpenAI directly for what you use. `gpt-realtime-translate` costs **$0.034 per audio minute, per direction**. With both directions active and audio streaming all the time that is roughly **$4 per hour**. By default LiveDub Voice only sends audio while someone speaks (plus about a second after each phrase), so a quiet call or game costs much less. It estimates your usage locally in the usage panel; the real bill is on platform.openai.com.

## Installation

1. Download `LiveDub Voice_<version>_x64-setup.exe` from the [Releases page](https://github.com/FatuKING/livedub-voice/releases).
2. Run it. The installer is not code-signed yet, so Windows SmartScreen may say **"Windows protected your PC"**. Click **More info**, check that the app is _LiveDub Voice_, then click **Run anyway**.
3. Pick the installer language (English or Spanish) and follow the steps. LiveDub Voice installs for your Windows user only, so it does not ask for administrator rights. It adds a Start menu shortcut and, unless you untick it on the last page, a desktop shortcut.
4. If WebView2 (the component that draws the panel) is missing, the installer downloads it from Microsoft, so keep an internet connection. It is already on Windows 11 and up-to-date Windows 10.
5. For outgoing translation, also install a virtual audio cable such as [VB-CABLE](https://vb-audio.com/Cable/) (free). LiveDub Voice never installs drivers.

Your settings and usage history live in `%APPDATA%\io.github.fatuking.livedubvoice`, your OpenAI API key in Windows Credential Manager and the logs in `%LOCALAPPDATA%\io.github.fatuking.livedubvoice\logs`.

### Updating and uninstalling

- **Update:** run the installer of the new version; it replaces the old one and keeps your settings.
- **Uninstall:** _Settings → Apps → Installed apps → LiveDub Voice → Uninstall_. If LiveDub Voice is running (also in the tray), the uninstaller offers to close it first. Your settings, usage history and API key are **kept** by default, so a reinstall picks up where you left off. Tick **"Delete the application data"** in the uninstaller to remove them too (settings, usage history, logs and the API key stored in Credential Manager).

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

To build the installer, run `pnpm tauri build`; it is written to `src-tauri/target/release/bundle/nsis/`.

The app is built with [Tauri 2](https://v2.tauri.app/): a Rust core handles audio and the OpenAI connection, and a React + TypeScript panel handles configuration, captions and usage.

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening an issue or pull request.

## License

[MIT](LICENSE) © 2026 Facundo Cozzani
