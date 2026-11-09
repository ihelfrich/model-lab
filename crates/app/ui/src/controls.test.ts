import { expect, test } from "vitest";
import { initialControlValues } from "./controls";

test("sliders start at the settings in force, clamped to the lesson's range", () => {
  const controls = [
    { field: "depth", min: 1, max: 5, step: 1 },
    { field: "rate", min: 0.02, max: 0.5, step: 0.02 },
    { field: "trees", min: 20, max: 300, step: 10 },
  ];
  const values = initialControlValues(controls, { depth: 2, rate: 0.1, trees: 150, min_leaf: 5 });
  expect(values).toEqual({ depth: 2, rate: 0.1, trees: 150 });
  expect(initialControlValues(controls, { depth: 9 }).depth).toBe(5);
  expect(initialControlValues(controls, {}).rate).toBe(0.02);
});
