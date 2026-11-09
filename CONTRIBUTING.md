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

To test translation end to end you need your own OpenAI API key and a virtual audio cable such as [VB-CABLE](https://vb-audio.com/Cable/). See the requirements in the [README](README.md#requirements).

## Branches

Create a branch from `main` for each change:

- Maintainer tasks: `task/<id>-<short-slug>` (for example `task/1.2-repo-base-files`).
- Outside contributors: `feat/<short-slug>` for new features, `fix/<short-slug>` for bug fixes (for example `fix/loopback-device-crash`).

Never push directly to `main`.

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
- PRs are merged by the maintainer using **squash merge** after review, so the PR title becomes the commit message on `main`.

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
