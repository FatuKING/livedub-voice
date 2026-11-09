import {
  render as rtlRender,
  type RenderOptions,
  type RenderResult,
} from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import I18nProvider from "../i18n/I18nProvider";
import type { UiLanguage } from "../i18n/types";

export interface I18nRenderOptions extends Omit<RenderOptions, "wrapper"> {
  /** Panel language; English unless a test says otherwise. */
  uiLanguage?: UiLanguage;
  /** OS languages, for `"system"`. */
  languages?: readonly string[];
}

/** Renders `ui` inside an `I18nProvider`, in English by default. */
export function render(
  ui: ReactElement,
  {
    uiLanguage = "en",
    languages = ["en-US"],
    ...options
  }: I18nRenderOptions = {},
): RenderResult {
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <I18nProvider preference={uiLanguage} languages={languages}>
        {children}
      </I18nProvider>
    );
  }
  return rtlRender(ui, { wrapper: Wrapper, ...options });
}
