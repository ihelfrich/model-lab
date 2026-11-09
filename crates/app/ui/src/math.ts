import katex from "katex";

/**
 * Render every element carrying a `data-tex` attribute with KaTeX. The TeX
 * source stays in the attribute and, through KaTeX's MathML annotation, in
 * the DOM: that is the text alternative. Rendering never throws; a bad
 * formula is shown as its source in red by KaTeX itself.
 */
export function renderMath(root: ParentNode): number {
  const nodes = root.querySelectorAll<HTMLElement>("[data-tex]");
  nodes.forEach((el) => {
    const tex = el.getAttribute("data-tex") ?? "";
    katex.render(tex, el, {
      throwOnError: false,
      output: "htmlAndMathml",
      displayMode: el.classList.contains("math-display"),
    });
  });
  return nodes.length;
}
