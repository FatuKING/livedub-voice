import { fireEvent, screen, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "../App";
import type { Settings } from "../lib/api";
import { render } from "../test/i18n";
import { en } from "./en";
import { translateError } from "./errors";
import { es } from "./es";
import {
  createTranslator,
  errorDetail,
  formattingLocale,
  resolveLocale,
  systemLocale,
} from "./translate";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

/** `"a.b.c" → string` for every leaf (plural forms included). */
function flatten(node: object, prefix = ""): Map<string, string> {
  const out = new Map<string, string>();
  for (const [key, value] of Object.entries(node)) {
    const path = `${prefix}${key}`;
    if (typeof value === "string") out.set(path, value);
    else
      for (const [k, v] of flatten(value as object, `${path}.`)) out.set(k, v);
  }
  return out;
}

function placeholders(text: string): string[] {
  return [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();
}

describe("dictionaries", () => {
  const english = flatten(en);
  const spanish = flatten(es);

  it("has every English key in Spanish, and nothing else", () => {
    expect([...spanish.keys()].sort()).toEqual([...english.keys()].sort());
  });

  it("uses the same placeholders in both languages", () => {
    for (const [key, text] of english) {
      expect(placeholders(spanish.get(key) ?? ""), key).toEqual(
        placeholders(text),
      );
    }
  });

  it("has no empty strings", () => {
    for (const [key, text] of [...english, ...spanish]) {
      expect(text.trim(), key).not.toBe("");
    }
  });
});

describe("locale resolution", () => {
  it("maps any es* OS language to Spanish and everything else to English", () => {
    expect(systemLocale(["es-AR", "en-US"])).toBe("es");
    expect(systemLocale(["ES"])).toBe("es");
    expect(systemLocale(["en-GB", "es-ES"])).toBe("en");
    expect(systemLocale(["fr-FR"])).toBe("en");
    expect(systemLocale(["est"])).toBe("en");
    expect(systemLocale([])).toBe("en");
  });

  it("follows the OS only for system", () => {
    expect(resolveLocale("system", ["es-MX"])).toBe("es");
    expect(resolveLocale("en", ["es-MX"])).toBe("en");
    expect(resolveLocale("es", ["en-US"])).toBe("es");
  });

  it("formats with the OS regional variant of the panel language", () => {
    expect(formattingLocale("es", ["es-AR"])).toBe("es-AR");
    expect(formattingLocale("es", ["en-US"])).toBe("es");
    expect(formattingLocale("en", ["es-AR"])).toBe("en-US");
    expect(formattingLocale("en", ["en-GB"])).toBe("en-GB");
  });
});

describe("translator", () => {
  const english = createTranslator("en");
  const spanish = createTranslator("es");

  it("fills in placeholders and leaves unknown ones as they are", () => {
    expect(english.t("app.version", { version: "1.2.3" })).toBe(
      "Version 1.2.3",
    );
    expect(spanish.t("app.version", { version: "1.2.3" })).toBe(
      "Versión 1.2.3",
    );
    expect(english.t("app.version")).toBe("Version {version}");
  });

  it("picks plural forms by count", () => {
    expect(english.t("usage.minutes", { count: 1 })).toBe("1 minute");
    expect(english.t("usage.minutes", { count: 2 })).toBe("2 minutes");
    expect(english.t("usage.minutes", { count: 0 })).toBe("0 minutes");
    expect(spanish.t("usage.minutes", { count: 1 })).toBe("1 minuto");
    expect(spanish.t("usage.minutes", { count: 3 })).toBe("3 minutos");
    expect(english.t("usage.minutes", { count: 1500 })).toBe("1,500 minutes");
  });

  it("formats US dollars for the locale", () => {
    expect(english.format.currency(0.034, { maximumFractionDigits: 3 })).toBe(
      "$0.034",
    );
    expect(
      spanish.format.currency(0.034, { maximumFractionDigits: 3 }),
    ).toContain("0,034");
    expect(english.format.percent(0.2)).toBe("20%");
  });
});

describe("translateError", () => {
  const english = createTranslator("en").t;
  const spanish = createTranslator("es").t;

  it("translates known codes and keeps the core's detail", () => {
    const error = { code: "audio", message: "audio device error: busy" };
    expect(translateError(error, english)).toBe("Audio device error (busy).");
    expect(translateError(error, spanish)).toBe(
      "Error del dispositivo de audio (busy).",
    );
    expect(
      translateError(
        { code: "openai_auth", message: "OpenAI rejected the API key" },
        spanish,
      ),
    ).toBe("OpenAI rechazó la API key.");
  });

  it("falls back to the core's message for unknown codes", () => {
    expect(
      translateError({ code: "brand_new", message: "something new" }, spanish),
    ).toBe("something new");
  });

  it("handles rejections that are not AppErrors", () => {
    expect(translateError(new Error("boom"), english)).toBe("boom");
    expect(translateError("plain", english)).toBe("plain");
  });

  it("strips the core's message prefix", () => {
    expect(errorDetail("I/O error: denied")).toBe("denied");
    expect(errorDetail("no prefix")).toBe("no prefix");
  });
});

describe("I18nProvider", () => {
  const settings: Settings = {
    version: 2,
    uiLanguage: "system",
    incoming: {
      enabled: false,
      targetLanguage: "es",
      sourceApp: { kind: "allExceptSelf" },
      outputDeviceId: null,
      originalVolume: 20,
    },
    outgoing: {
      enabled: false,
      targetLanguage: "en",
      inputDeviceId: null,
      outputDeviceId: null,
    },
  };

  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation((command, args) => {
      switch (command) {
        case "get_settings":
          return Promise.resolve(settings);
        case "save_settings":
          return Promise.resolve((args as { settings: Settings }).settings);
        default:
          return new Promise(() => {});
      }
    });
  });

  it("follows a Spanish OS when set to system", () => {
    render(<App />, { uiLanguage: "system", languages: ["es-AR"] });

    expect(
      screen.getByRole("heading", { level: 1, name: "Inicio" }),
    ).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("es");
  });

  it("uses English for any other OS language when set to system", () => {
    render(<App />, { uiLanguage: "system", languages: ["fr-FR"] });

    expect(
      screen.getByRole("heading", { level: 1, name: "Home" }),
    ).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("en");
  });

  it("switches the whole panel at once and saves the choice", async () => {
    render(<App />, { uiLanguage: "system", languages: ["en-US"] });
    const nav = () => screen.getByRole("navigation");

    fireEvent.click(within(nav()).getByRole("button", { name: "Settings" }));
    fireEvent.change(screen.getByLabelText("App language"), {
      target: { value: "es" },
    });

    expect(
      screen.getByRole("heading", { level: 1, name: "Configuración" }),
    ).toBeInTheDocument();
    expect(
      within(nav()).getByRole("button", { name: "Inicio" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Idioma de la app")).toHaveValue("es");
    expect(document.documentElement.lang).toBe("es");

    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("save_settings", {
        settings: { ...settings, uiLanguage: "es" },
      }),
    );

    fireEvent.change(screen.getByLabelText("Idioma de la app"), {
      target: { value: "en" },
    });
    expect(
      within(nav()).getByRole("button", { name: "Home" }),
    ).toBeInTheDocument();
  });
});
