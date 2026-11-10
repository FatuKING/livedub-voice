import { describe, expect, it } from "vitest";
import {
  LANGUAGES,
  languageLabel,
  languageName,
  languageOptions,
} from "./languages";

describe("translator languages", () => {
  it("lists every target language of the model once", () => {
    expect([...LANGUAGES].sort()).toEqual(
      [
        "de",
        "en",
        "es",
        "fr",
        "hi",
        "id",
        "it",
        "ja",
        "ko",
        "pt",
        "ru",
        "vi",
        "zh",
      ].sort(),
    );
  });

  it("names languages in the panel language", () => {
    expect(languageName("es", "en")).toBe("Spanish");
    expect(languageName("ja", "en")).toBe("Japanese");
    expect(languageName("es", "es")).toBe("español");
    expect(languageName("de", "es")).toBe("alemán");
    expect(languageLabel("de", "es")).toBe("Alemán");
  });

  it("puts English and Spanish on top, then sorts by localized name (English panel)", () => {
    const labels = languageOptions("en").map((option) => option.label);

    expect(labels).toEqual([
      "English",
      "Spanish",
      "Chinese",
      "French",
      "German",
      "Hindi",
      "Indonesian",
      "Italian",
      "Japanese",
      "Korean",
      "Portuguese",
      "Russian",
      "Vietnamese",
    ]);
  });

  it("puts Spanish and English on top, then sorts by localized name (Spanish panel)", () => {
    const options = languageOptions("es");

    expect(options.slice(0, 2).map((option) => option.value)).toEqual([
      "es",
      "en",
    ]);
    const rest = options.slice(2).map((option) => option.label);
    expect(rest).toEqual([...rest].sort((a, b) => a.localeCompare(b, "es")));
    expect(rest).toContain("Alemán");
    expect(options).toHaveLength(LANGUAGES.length);
  });
});
