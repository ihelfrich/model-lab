import type { Learner } from "./api";

/** Initial slider values: the setting in force for each field the learner may change. */
export function initialControlValues(controls: Learner[], config: Record<string, unknown>): Record<string, number> {
  const values: Record<string, number> = {};
  for (const c of controls) {
    const current = config[c.field];
    values[c.field] = typeof current === "number" ? Math.min(c.max, Math.max(c.min, current)) : c.min;
  }
  return values;
}
