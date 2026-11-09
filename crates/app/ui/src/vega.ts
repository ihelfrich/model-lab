import type { EmbedOptions } from "vega-embed";
import { expressionInterpreter } from "vega-interpreter";

/**
 * Vega options for a page whose Content Security Policy forbids `eval`:
 * the webview loads no remote code and allows no `'unsafe-eval'`, so Vega
 * must interpret its expressions instead of compiling them with `Function`.
 */
export const vegaOptions: EmbedOptions = {
  ast: true,
  expr: expressionInterpreter,
  actions: { export: true, source: false, compiled: false, editor: false },
  renderer: "svg",
};
