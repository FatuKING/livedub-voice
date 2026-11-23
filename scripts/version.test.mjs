import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  FILES,
  checkVersions,
  parseVersion,
  readAll,
  writeVersion,
} from "./version.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function repoTexts() {
  return Object.fromEntries(
    FILES.map((file) => [
      file.path,
      readFileSync(join(root, file.path), "utf8"),
    ]),
  );
}

function bumpAll(texts, version) {
  return Object.fromEntries(
    FILES.map((file) => [
      file.path,
      writeVersion(file, texts[file.path], version),
    ]),
  );
}

describe("parseVersion", () => {
  it("accepts versions and v-prefixed tags", () => {
    expect(parseVersion("0.1.0")).toBe("0.1.0");
    expect(parseVersion("v0.1.0")).toBe("0.1.0");
    expect(parseVersion("v1.2.3-beta.1")).toBe("1.2.3-beta.1");
  });

  it("rejects anything else", () => {
    for (const bad of ["", "v", "1.2", "01.2.3", "1.2.3+build", "vv1.2.3"]) {
      expect(() => parseVersion(bad)).toThrow(/not a version/);
    }
  });
});

describe("the repository", () => {
  it("has the same version in every file", () => {
    const texts = repoTexts();
    const version = checkVersions(texts);
    expect(readAll(texts).map((entry) => entry.version)).toEqual(
      FILES.map(() => version),
    );
  });
});

describe("writeVersion", () => {
  it("changes only the app's version strings", () => {
    const texts = repoTexts();
    const bumped = bumpAll(texts, "9.8.7-rc.1");
    expect(checkVersions(bumped, "v9.8.7-rc.1")).toBe("9.8.7-rc.1");

    for (const file of FILES) {
      const before = texts[file.path].split("\n");
      const after = bumped[file.path].split("\n");
      expect(after).toHaveLength(before.length);
      const changed = after.filter((line, index) => line !== before[index]);
      expect(changed).toHaveLength(1);
      expect(changed[0]).toContain("9.8.7-rc.1");
    }
  });

  it("leaves dependency versions in Cargo.toml and Cargo.lock alone", () => {
    const toml = FILES.find((file) => file.path === "src-tauri/Cargo.toml");
    const text =
      '[package]\nname = "livedub-voice"\nversion = "0.1.0"\nedition = "2024"\n\n' +
      '[dependencies]\nserde = { version = "1" }\n';
    expect(writeVersion(toml, text, "0.2.0")).toBe(
      text.replace('version = "0.1.0"', 'version = "0.2.0"'),
    );

    const lock = FILES.find((file) => file.path === "src-tauri/Cargo.lock");
    const lockText =
      '[[package]]\nname = "libc"\nversion = "0.1.0"\n\n' +
      '[[package]]\nname = "livedub-voice"\nversion = "0.1.0"\n';
    expect(writeVersion(lock, lockText, "0.2.0")).toBe(
      lockText.replace(
        'livedub-voice"\nversion = "0.1.0"',
        'livedub-voice"\nversion = "0.2.0"',
      ),
    );
  });

  it("works with Windows line endings", () => {
    const crlf = Object.fromEntries(
      Object.entries(repoTexts()).map(([path, text]) => [
        path,
        text.replace(/\r?\n/g, "\r\n"),
      ]),
    );
    const bumped = bumpAll(crlf, "3.0.0");
    expect(checkVersions(bumped, "v3.0.0")).toBe("3.0.0");
    for (const text of Object.values(bumped)) {
      expect(text.replace(/\r\n/g, "")).not.toContain("\n");
    }
  });
});

describe("checkVersions", () => {
  it("fails when the files disagree", () => {
    const texts = repoTexts();
    const file = FILES[1];
    texts[file.path] = writeVersion(file, texts[file.path], "0.0.1");
    expect(() => checkVersions(texts)).toThrow(/differs between files/);
  });

  it("fails when the tag does not match", () => {
    const texts = bumpAll(repoTexts(), "0.3.0");
    expect(() => checkVersions(texts, "v0.3.1")).toThrow(/does not match/);
    expect(() => checkVersions(texts, "v0.3.0-beta.1")).toThrow(
      /does not match/,
    );
    expect(() => checkVersions(texts, "release-0.3.0")).toThrow(
      /not a version/,
    );
  });
});
