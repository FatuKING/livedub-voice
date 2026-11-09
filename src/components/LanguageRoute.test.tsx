import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import LanguageRoute from "./LanguageRoute";

describe("LanguageRoute", () => {
  it("shows both language chips in route order", () => {
    render(<LanguageRoute from="en" to="es" direction="incoming" />);

    const route = screen.getByRole("img", { name: "English to Spanish" });
    expect(route).toHaveTextContent(/^EN.*ES$/);
  });

  it("uses the direction for its color class", () => {
    const { rerender } = render(
      <LanguageRoute from="es" to="en" direction="outgoing" />,
    );
    const route = screen.getByRole("img", { name: "Spanish to English" });
    expect(route).toHaveClass("ld-route--outgoing");
    expect(route).not.toHaveClass("ld-route--incoming");

    rerender(<LanguageRoute from="es" to="en" direction="incoming" />);
    expect(route).toHaveClass("ld-route--incoming");
  });

  it("supports a compact size", () => {
    render(<LanguageRoute from="es" to="en" direction="outgoing" size="sm" />);

    expect(screen.getByRole("img", { name: "Spanish to English" })).toHaveClass(
      "ld-route--sm",
    );
  });
});
