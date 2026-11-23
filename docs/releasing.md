# Releasing

Releases are built and published by the [Release workflow](../.github/workflows/release.yml) when a `vX.Y.Z` tag is pushed. Nobody uploads installers by hand.

## Cutting a release

1. **Bump the version** on a branch and merge it through a PR:

   ```bash
   git switch main && git pull
   git switch -c chore/release-0.2.0
   pnpm version:bump 0.2.0
   git commit -am "chore: release 0.2.0"
   git push -u origin chore/release-0.2.0
   ```

   `pnpm version:bump` writes the version into the four files that carry it: `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and the app's entry in `src-tauri/Cargo.lock`. To do it by hand, edit the first three and run `cargo check` in `src-tauri/`, which updates the lock file; `pnpm version:check` says whether they agree.

   The PR touches `tauri.conf.json`, so it also runs the Release workflow as a dry run: its `build` job must pass, and its artifact holds the installer, `SHA256SUMS.txt` and the release notes exactly as they will be published.

2. **Squash-merge the PR**, then tag the merge commit on `main` and push the tag:

   ```bash
   git switch main && git pull
   pnpm version:check v0.2.0
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. **Watch the run** in the Actions tab (about 15 minutes). When it finishes, the release is on the [Releases page](https://github.com/FatuKING/livedub-voice/releases) with the installer, a version-less copy and `SHA256SUMS.txt`.

4. **Smoke-test the published installer** on a clean Windows user (see "Icons and installer" in [CONTRIBUTING.md](../CONTRIBUTING.md)).

The tag must match the version in the files (`v0.2.0` ↔ `0.2.0`) and point at a commit of `main`; otherwise the workflow fails before building and publishes nothing. Only the maintainer pushes tags.

## Pre-releases

A tag with a pre-release suffix — `v0.2.0-beta.1`, `v0.2.0-rc.1` — publishes a GitHub **pre-release**. Bump the files to `0.2.0-beta.1` the same way. Pre-releases are never marked "Latest", so `releases/latest` and the landing page keep pointing at the last stable version.

## Release assets and links

| Asset                                   | What it is                                                                       |
| --------------------------------------- | -------------------------------------------------------------------------------- |
| `LiveDub-Voice_<version>_x64-setup.exe` | The NSIS installer                                                               |
| `LiveDub-Voice_x64-setup.exe`           | The same file under a name that never changes                                    |
| `SHA256SUMS.txt`                        | SHA-256 of both installers, in `sha256sum -c` format (also in the release notes) |

The names never contain spaces (the Tauri bundler's own name, `LiveDub Voice_<version>_x64-setup.exe`, is renamed). Links that keep working across releases:

- Latest release page: `https://github.com/FatuKING/livedub-voice/releases/latest`
- Latest installer, straight download: `https://github.com/FatuKING/livedub-voice/releases/latest/download/LiveDub-Voice_x64-setup.exe`
- A specific version: `https://github.com/FatuKING/livedub-voice/releases/download/v0.2.0/LiveDub-Voice_0.2.0_x64-setup.exe`

`releases/latest/download/<name>` only works with a name that is the same in every release, which is why the version-less copy exists. The landing page's "Download" button can use the straight link; the release page is the fallback that also shows the notes.

## Release notes

The notes start with a fixed header from [`.github/release-notes.md`](../.github/release-notes.md) (download link, requirements, the SmartScreen note and the checksums) and continue with GitHub's generated notes: the PRs merged since the previous tag and new contributors. Good PR titles make good release notes. To change the header, edit that file; `{{VERSION}}`, `{{TAG}}` and `{{REPOSITORY}}` are filled in by the workflow.

To fix the notes of a published release, edit them on GitHub. To rebuild the assets of a release, re-run the workflow run of its tag: the publish job replaces the assets of an existing release and leaves its notes alone.

## Dry runs

The `build` job runs without publishing when:

- a PR changes the release workflow, the notes header, `scripts/version.mjs`, `src-tauri/tauri.conf.json` or `src-tauri/installer/`;
- someone starts the workflow by hand (_Actions → Release → Run workflow_, on any branch).

Only a pushed `v*` tag runs the `publish` job, the only job allowed to write to the repository.

A manual run on `main` also warms the Rust build cache that tag builds use.

## Code signing

The installer is **not signed** yet, so SmartScreen warns on first run. When a code-signing certificate exists: add the signing step marked `TODO(code signing)` in the workflow (Tauri's `bundle.windows.signCommand` or `certificateThumbprint`, secrets only in the `build` job), and drop the SmartScreen note from the release notes header and the README.
