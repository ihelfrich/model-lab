// @vitest-environment jsdom
import { expect, test } from "vitest";
import { compile } from "vega-lite";
import { parse, View } from "vega";
import { expressionInterpreter } from "vega-interpreter";
import { vegaOptions } from "./vega";

// A spec shaped like the Rust loss curve: a filter expression and a text label.
const spec = {
  $schema: "https://vega.github.io/schema/vega-lite/v6.json",
  datasets: { main: [{ stage: 0, series: "training (n=3)", loss: 2 }, { stage: 1, series: "training (n=3)", loss: 1 }] },
  layer: [
    { data: { name: "main" }, mark: "line", encoding: { x: { field: "stage", type: "quantitative" }, y: { field: "loss", type: "quantitative" }, color: { field: "series", type: "nominal" } } },
    { data: { name: "main" }, transform: [{ filter: "datum.stage > 0" }], mark: "point", encoding: { x: { field: "stage", type: "quantitative" }, y: { field: "loss", type: "quantitative" } } },
  ],
} as const;

test("plots render through the expression interpreter, with eval forbidden", async () => {
  const forbidden = () => {
    throw new EvalError("eval is blocked by the Content Security Policy");
  };
  const originalFunction = globalThis.Function;
  // Simulate the webview CSP: any attempt to compile code throws.
  (globalThis as unknown as { Function: unknown }).Function = new Proxy(originalFunction, { construct: forbidden, apply: forbidden });
  try {
    const runtime = parse(compile(spec as never).spec, undefined, { ast: true });
    const view = new View(runtime, { expr: expressionInterpreter, renderer: "none" });
    await view.runAsync();
    expect(view.data("data_1").length + view.data("main").length).toBeGreaterThan(0);
  } finally {
    (globalThis as unknown as { Function: unknown }).Function = originalFunction;
  }
  expect(vegaOptions.ast).toBe(true);
  expect(vegaOptions.expr).toBe(expressionInterpreter);
});
