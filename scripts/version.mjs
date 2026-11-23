// Keeps the app version in sync across the files that carry it.
//
//   pnpm version:check            all files agree; prints the version
//   pnpm version:check v0.2.0     ...and it matches this tag (the release workflow runs this)
//   pnpm version:bump 0.2.0       writes 0.2.0 into every file
//
// Files: `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and the app's own
// entry in `src-tauri/Cargo.lock` (CI builds with `--locked`, so it must follow `Cargo.toml`).
// Only the version strings change; every other byte of each file is kept as is.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

/** `MAJOR.MINOR.PATCH` with an optional pre-release (`-beta.1`); no build metadata. */
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$/;

/** The crate name in `Cargo.toml` / `Cargo.lock`. */
const CRATE = "livedub-voice";

/**
 * How to read and replace the version in each file. Each pattern captures the text before the
 * version (group 1), the version (group 2) and the text after it (group 3).
 */
export const FILES = [
  {
    path: "package.json",
    // The top-level key: two-space indent, as Prettier writes it.
    pattern: /^( {2}"version": ")([^"]*)(",?)$/m,
  },
  {
    path: "src-tauri/tauri.conf.json",
    pattern: /^( {2}"version": ")([^"]*)(",?)$/m,
  },
  {
    path: "src-tauri/Cargo.toml",
    // The first `version =` after `[package]`, before any other table.
    pattern: /^(\[package\]\r?\n(?:(?!\[)[^\n]*\n)*?version = ")([^"]*)(")/m,
  },
  {
    path: "src-tauri/Cargo.lock",
    pattern: new RegExp(
      `^(\\[\\[package\\]\\]\\r?\\nname = "${CRATE}"\\r?\\nversion = ")([^"]*)(")`,
      "m",
    ),
  },
];

/** `v1.2.3` → `1.2.3`; throws on anything that is not a version. */
export function parseVersion(input) {
  const version = input.startsWith("v") ? input.slice(1) : input;
  if (!SEMVER.test(version)) {
    throw new Error(
      `"${input}" is not a version like 1.2.3 or 1.2.3-beta.1 (a leading "v" is allowed)`,
    );
  }
  return version;
}

/** The version in one file's text; throws if the pattern does not match. */
export function readVersion(file, text) {
  const match = file.pattern.exec(text);
  if (!match) throw new Error(`No version found in ${file.path}`);
  return match[2];
}

/** The file's text with its version replaced. */
export function writeVersion(file, text, version) {
  readVersion(file, text);
  return text.replace(
    file.pattern,
    (_match, before, _old, after) => `${before}${version}${after}`,
  );
}

/** Each file's version, as `[{ path, version }]`. */
export function readAll(texts) {
  return FILES.map((file) => ({
    path: file.path,
    version: readVersion(file, texts[file.path]),
  }));
}

/**
 * The version every file agrees on. Throws if they differ, or if `expected` (a version or a
 * `v`-prefixed tag) is given and does not match.
 */
export function checkVersions(texts, expected) {
  const found = readAll(texts);
  const versions = new Set(found.map((entry) => entry.version));
  const listing = found
    .map((entry) => `  ${entry.path}: ${entry.version}`)
    .join("\n");
  if (versions.size !== 1) {
    throw new Error(
      `The version differs between files:\n${listing}\nRun \`pnpm version:bump <version>\`.`,
    );
  }
  const [version] = versions;
  parseVersion(version);
  if (expected !== undefined && parseVersion(expected) !== version) {
    throw new Error(
      `"${expected}" does not match the version in the files:\n${listing}\n` +
        `Bump the version through a PR first, or tag the commit that has it.`,
    );
  }
  return version;
}

function main([command, argument]) {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const texts = Object.fromEntries(
    FILES.map((file) => [
      file.path,
      readFileSync(join(root, file.path), "utf8"),
    ]),
  );

  if (command === "check") {
    console.log(checkVersions(texts, argument));
    return;
  }

  if (command === "bump" && argument !== undefined) {
    const version = parseVersion(argument);
    for (const file of FILES) {
      const text = texts[file.path];
      const updated = writeVersion(file, text, version);
      if (updated !== text) writeFileSync(join(root, file.path), updated);
      texts[file.path] = updated;
    }
    checkVersions(texts, version);
    console.log(`Version set to ${version} in:`);
    for (const file of FILES) console.log(`  ${file.path}`);
    return;
  }

  throw new Error(
    "Usage: node scripts/version.mjs check [vX.Y.Z]\n" +
      "       node scripts/version.mjs bump X.Y.Z",
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
