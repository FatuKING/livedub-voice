# Contributing to LiveDub Voice

Thanks for your interest in LiveDub Voice! The project is in early development, so things move fast and some areas are still taking shape. This guide explains how to set up the project and how changes get in.

Everything in the project — code, comments, commits, issues and pull requests — is written in **English**.

## Development setup

1. Install the prerequisites:
   - [Node.js](https://nodejs.org/) 20 or later
   - [pnpm](https://pnpm.io/)
   - [Rust](https://rustup.rs/) stable with the MSVC toolchain (`stable-x86_64-pc-windows-msvc`)
   - [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload
   - WebView2 runtime (preinstalled on Windows 11 and recent Windows 10)
2. Clone the repository and install dependencies:

   ```bash
   git clone https://github.com/FatuKING/livedub-voice.git
   cd livedub-voice
   pnpm install
   ```

3. Run the app in development mode:

   ```bash
   pnpm tauri dev
   ```

Useful commands:

| Command                         | What it does                   |
| ------------------------------- | ------------------------------ |
| `pnpm build`                    | Type-check and build the panel |
| `cargo build` (in `src-tauri/`) | Build the Rust core            |
| `cargo test` (in `src-tauri/`)  | Run the Rust tests             |
| `pnpm tauri build`              | Build the Windows installer    |
| `pnpm icons`                    | Regenerate the app icons       |
| `pnpm version:bump X.Y.Z`       | Set the version everywhere     |

### Icons and installer

The app icon is drawn in two SVG masters in `src-tauri/icons/`: `icon.svg` (32 px and up) and `icon-small.svg`, the same drawing without the voice bars for the 16, 20 and 24 px sizes (system tray, title bar). Every other file in that folder — the PNGs, `icon.ico`, `icon.icns` and the installer's `nsis-header.bmp` / `nsis-sidebar.bmp` — is generated: edit a master, run `pnpm icons` and commit the results. The panel's `LogoMark` component draws the same mark, so update it too.

`pnpm tauri build` builds the NSIS installer (`src-tauri/target/release/bundle/nsis/LiveDub Voice_<version>_x64-setup.exe`). Its options live under `bundle` in `src-tauri/tauri.conf.json`, and `src-tauri/installer/hooks.nsh` holds the custom install/uninstall steps. Before a release, install it on a clean user, check the icon in the Start menu, taskbar and tray, then uninstall it with and without "Delete the application data". How releases are cut and published is in [docs/releasing.md](docs/releasing.md).

### Running without an OpenAI key

Set `LIVEDUB_FAKE_TRANSLATOR` before starting the app to replace OpenAI with a local fake translator that costs nothing:

| Value           | What the fake does                                                                       |
| --------------- | ---------------------------------------------------------------------------------------- |
| `1` or `replay` | Replays a recorded Spanish → English session (captions and short audio deltas) in a loop |
| `echo`          | Plays your own input back as the "translation" after 1.5 s                               |
| `echo:<ms>`     | Same, with another delay (for example `echo:500`)                                        |

```bash
LIVEDUB_FAKE_TRANSLATOR=echo pnpm tauri dev
```

In PowerShell: `$env:LIVEDUB_FAKE_TRANSLATOR = "echo"; pnpm tauri dev`. The recorded session lives in `src-tauri/src/translator/fixtures/fake_session.jsonl`.

To test translation end to end you need your own OpenAI API key and a virtual audio cable such as [VB-CABLE](https://vb-audio.com/Cable/). See the requirements in the [README](README.md#requirements).

### Troubleshooting audio quality

Every session of either direction logs a summary when it stops (`session summary`, `latency`, `input summary`, each with `direction` = `incoming` or `outgoing`): jitter underruns and dropped samples, the largest backlog of translated audio, how many output deltas arrived, the audio sent and received, the latency per utterance (min / median / max) and the input's peak, RMS and clipping. Outgoing sessions also warn about a clipping or too quiet microphone, a silent channel or channels that cancel out. Incoming sessions log when the original audio is ducked and restored (`ducking started`, `ducking stopped`). Logs live in `%LOCALAPPDATA%\io.github.fatuking.livedubvoice\logs`. Use `RUST_LOG=livedub_voice_lib=debug` for one line per utterance.

For a closer look, set `LIVEDUB_RECORD_DIR` to a folder before starting the app. Each session then writes a subfolder `outgoing-<unix seconds>` or `incoming-<unix seconds>` with:

| File           | Contents                                                                                                                                                       |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sent.wav`     | The audio actually sent to the translator: the microphone, or the captured apps after the ducking compensation (PCM16 mono 24 kHz, silence included)           |
| `received.wav` | The translated audio as received, on a wall-clock timeline: gaps between deltas are silence, so late deltas are audible                                        |
| `output.wav`   | What was played on the virtual cable (outgoing) or the headphones (incoming), in that device's format                                                          |
| `events.jsonl` | Timestamped events: each 200 ms block sent (RMS/peak), each output delta, transcript deltas, speech start/end, latency, jitter stats every second, the summary |

```powershell
$env:LIVEDUB_RECORD_DIR = "$HOME\livedub-recordings"; pnpm tauri dev
```

This is a **local-only debugging tool**, off by default: nothing is uploaded, but the files contain your voice (or the other participants') and the transcript, so only share them with people you trust and delete them when you are done.

Hardware checks that are skipped by default (they play a quiet tone on the default output):

```powershell
cargo test --manifest-path src-tauri/Cargo.toml session_volume_scales_the_loopback_capture -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml ducker_ducks_and_restores_a_real_app -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml incoming_from_a_tone_player -- --ignored --nocapture
```

## Branches

Create a branch from `main` for each change, named `<type>/<short-slug>`, where `<type>` is the same Conventional Commits type the PR will use:

| Type        | Use it for                              | Example                         |
| ----------- | --------------------------------------- | ------------------------------- |
| `feat/`     | New features                            | `feat/process-loopback-capture` |
| `fix/`      | Bug fixes                               | `fix/loopback-device-crash`     |
| `docs/`     | Documentation only                      | `docs/usage-discord`            |
| `refactor/` | Code changes that don't change behavior | `refactor/audio-buffers`        |
| `test/`     | Adding or fixing tests                  | `test/resampler-roundtrip`      |
| `ci/`       | GitHub Actions and build pipeline       | `ci/windows-build`              |
| `chore/`    | Tooling, dependencies, repo config      | `chore/upgrade-eslint`          |

Use lowercase and hyphens. Never push directly to `main`.

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<optional scope>): <short summary>
```

Common types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `ci`, `perf`. For example:

```
feat(audio): add process loopback capture
fix(panel): keep the API key field masked after validation
```

## Pull requests

- **One focused change per pull request.** Smaller PRs are easier to review and merge.
- Use a Conventional Commits title for the PR (for example `fix: handle device disconnects`).
- In the description, explain **what** changed and **why**, link the related issue if there is one, and describe **how you tested it** (commands you ran, what you checked manually in the app).
- Make sure `pnpm build` and `cargo build` pass before asking for review.
- Pull requests must pass the `CI / checks` workflow (GitHub Actions on Windows: lint, tests and build) before they can be merged.
- PRs are merged by the maintainer using **squash merge** after review (it is the only merge method enabled), so the PR title becomes the commit message on `main`. The branch is deleted automatically after merging.

If a change affects the project requirements or decisions described in [CLAUDE.md](CLAUDE.md), update that file in the same PR.

## Code style

- **Rust:** formatted with `rustfmt` (`src-tauri/rustfmt.toml`) and linted with Clippy (lints in `src-tauri/Cargo.toml`, warnings are errors). Avoid `unwrap()`/`expect()` outside tests.
- **TypeScript:** linted with ESLint (`eslint.config.js`) and formatted with Prettier (`.prettierrc`).
- Line endings, indentation and charset are defined in [`.editorconfig`](.editorconfig) and [`.gitattributes`](.gitattributes); most editors pick them up automatically.

Run these from the repository root before opening a pull request:

| Command          | What it does                                                                                  |
| ---------------- | --------------------------------------------------------------------------------------------- |
| `pnpm format`    | Formats everything (`prettier --write .` and `cargo fmt`).                                    |
| `pnpm lint`      | Type-checks (`tsc`), runs ESLint, `prettier --check`, `cargo fmt --check` and `cargo clippy`. |
| `pnpm test`      | Runs the panel tests (Vitest) and the Rust tests (`cargo test`).                              |
| `pnpm typecheck` | Type-checks the panel only.                                                                   |

All four must pass for a pull request to be merged. The recommended VS Code extensions are listed in [`.vscode/extensions.json`](.vscode/extensions.json).

## Reporting bugs

Open an [issue](https://github.com/FatuKING/livedub-voice/issues) and include:

- What you expected to happen and what happened instead.
- Steps to reproduce it.
- Your Windows version, the LiveDub Voice version (or commit), and the apps involved (for example Discord, Teams, a specific game).
- The audio devices you use, including your virtual audio cable.
- Relevant log output, if any.

**Never include your OpenAI API key** (or any other secret) in issues, logs or screenshots.

For security issues, please do not open a public issue; contact the maintainer privately through GitHub instead.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
