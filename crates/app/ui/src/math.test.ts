// @vitest-environment jsdom
import { describe, expect, test } from "vitest";
import { renderMath } from "./math";

describe("renderMath", () => {
  test("renders every data-tex element and keeps the source", () => {
    document.body.innerHTML =
      '<p>Loss <span class="math" data-tex="\\frac{1}{n}\\sum e_i^2">\\frac{1}{n}\\sum e_i^2</span></p>' +
      '<div class="math math-display" data-tex="y = f(x)">y = f(x)</div>';
    const count = renderMath(document.body);
    expect(count).toBe(2);
    const nodes = document.querySelectorAll<HTMLElement>("[data-tex]");
    nodes.forEach((el) => {
      expect(el.querySelector(".katex")).not.toBeNull();
      expect(el.querySelector("annotation")?.textContent).toBe(el.getAttribute("data-tex"));
    });
    expect(nodes[0]?.getAttribute("data-tex")).toBe("\\frac{1}{n}\\sum e_i^2");
  });
});
