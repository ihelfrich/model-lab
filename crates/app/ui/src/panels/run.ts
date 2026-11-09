import { api, type Learner, type RunPanel, type Status } from "../api";
import { announce, el } from "../dom";
import { renderPlot, type PlotHandle } from "../plots";
import { SelectionStore } from "../selection";
import { runs } from "../runs";
import { initialControlValues } from "../controls";

const fmt = (v: number | null | undefined, digits = 4) => (v === null || v === undefined ? "–" : v.toFixed(digits));

function controlsFor(controls: Learner[], current: Record<string, unknown>, onRun: (overrides: Record<string, number>) => void): HTMLElement {
  const form = el("form", { class: "controls", "aria-label": "Experiment settings" });
  const values = initialControlValues(controls, current);
  for (const c of controls) {
    const id = `ctl-${c.field}`;
    const start = values[c.field] ?? c.min;
    const label = el("label", { for: id }, `${c.field}: `);
    const out = el("output", {}, String(start));
    const input = el("input", { id, type: "range", min: String(c.min), max: String(c.max), step: String(c.step), value: String(start) }) as HTMLInputElement;
    input.addEventListener("input", () => {
      values[c.field] = Number(input.value);
      out.textContent = input.value;
    });
    form.append(el("div", { class: "control" }, label, input, out));
  }
  const run = el("button", { type: "submit", class: "button primary" }, controls.length ? "Run with these settings" : "Run the experiment");
  form.append(run);
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    onRun({ ...values });
  });
  return form;
}

function arithmetic(panel: RunPanel): HTMLElement {
  const o = panel.observation;
  const box = el("section", { class: "arithmetic", "aria-label": "Arithmetic from the selected observation" });
  box.append(el("h2", {}, "Arithmetic from the selected observation"));
  if (!o) {
    box.append(el("p", { class: "muted" }, "Click a point in the scatter (or choose a row) to follow its numbers."));
    return box;
  }
  const xs = o.x.map(([n, v]) => `${n} = ${fmt(v, 3)}`).join(", ");
  box.append(el("p", {}, `Observation ${o.row + 1} (${o.in_training ? "training" : "validation"}): ${xs}; outcome ${fmt(o.y, 3)}.`));
  if (o.h !== null && o.rate !== null) {
    box.append(
      el("p", { class: "mono" }, `${fmt(o.before, 3)} + ${fmt(o.rate, 3)} × (${fmt(o.h, 3)}) = ${fmt(o.after, 3)}`),
      el("p", { class: "mono" }, `residual: ${fmt(o.residual_before, 3)} → ${fmt(o.residual_after, 3)}`),
    );
  } else if (o.h !== null) {
    box.append(
      el("p", { class: "mono" }, `tree ${panel.stage} votes ${fmt(o.h, 3)}; the forest's average moves ${fmt(o.before, 3)} → ${fmt(o.after, 3)}`),
      el("p", { class: "mono" }, `residual: ${fmt(o.residual_before, 3)} → ${fmt(o.residual_after, 3)}`),
    );
  } else {
    box.append(el("p", { class: "mono" }, `prediction ${fmt(o.after, 3)}; residual ${fmt(o.residual_after, 3)}`));
  }
  if (o.rules.length) {
    const list = el("ul", { class: "rules" });
    for (const r of o.rules) list.append(el("li", { class: "mono" }, r));
    box.append(el("details", {}, el("summary", {}, `The tree added at stage ${panel.stage}`), list));
  }
  return box;
}

async function waitFor(runId: string): Promise<Status> {
  for (;;) {
    const s = await api.experimentStatus(runId);
    if (s !== "Running") return s;
    await new Promise((r) => setTimeout(r, 200));
  }
}

/** The Run panel: controls, replay, plots with tables, the arithmetic. */
export async function renderRun(lessonId: string, host: HTMLElement): Promise<void> {
  host.replaceChildren();
  const store = new SelectionStore();
  const status = el("p", { class: "notice", role: "status" });
  const controlsHost = el("div");
  const replay = el("div", { class: "replay" });
  const plotsHost = el("div", { class: "plots" });
  const arithmeticHost = el("div");
  const exportHost = el("div", { class: "export" });
  host.append(status, controlsHost, replay, plotsHost, arithmeticHost, exportHost);
  let handles: PlotHandle[] = [];

  let panel: RunPanel | null = null;
  let current = runs.get(lessonId);
  let request = 0;

  async function show(): Promise<void> {
    if (!current) return;
    // Only the latest request may touch the DOM: a slider move followed by a
    // click must not interleave two renders.
    const token = ++request;
    const s = store.get();
    const next = await api.runPanel(current.runId, s.stage, s.feature, s.selected);
    if (token !== request) return;
    panel = next;
    // The sliders show the settings this run was fitted with.
    controlsHost.replaceChildren(controlsFor(panel.controls, panel.config, (o) => void start(o)));
    replay.replaceChildren(replayControls(panel));
    plotsHost.replaceChildren();
    const fresh: PlotHandle[] = [];
    for (const p of panel.plots) {
      const handle = await renderPlot(p, plotsHost, (row) => store.set({ selected: row }));
      if (token !== request) {
        handle.element.remove();
        return;
      }
      fresh.push(handle);
    }
    handles = fresh;
    arithmeticHost.replaceChildren(arithmetic(panel));
    exportHost.replaceChildren(exportControls(panel));
    status.textContent = panel.kind === "by_hand"
      ? `Round ${panel.stage} of ${panel.stages - 1}.`
      : `Stage ${panel.stage} of ${panel.stages - 1}${panel.best_stage !== null ? `; best validation stage ${panel.best_stage}` : ""}.`;
  }

  function replayControls(p: RunPanel): HTMLElement {
    const bar = el("div", { class: "replay-bar", role: "group", "aria-label": "Replay" });
    const prev = el("button", { type: "button", class: "button" }, p.kind === "by_hand" ? "Previous round" : "Previous tree");
    const next = el("button", { type: "button", class: "button" }, p.kind === "by_hand" ? "Next round" : "Add next tree");
    const slider = el("input", { type: "range", min: "0", max: String(p.stages - 1), value: String(p.stage), "aria-label": "Stage" }) as HTMLInputElement;
    const stageOut = el("output", {}, `stage ${p.stage}`);
    prev.disabled = p.stage <= 0;
    next.disabled = p.stage >= p.stages - 1;
    prev.addEventListener("click", () => store.set({ stage: Math.max(0, store.get().stage - 1) }));
    next.addEventListener("click", () => store.set({ stage: Math.min(p.stages - 1, store.get().stage + 1) }));
    slider.addEventListener("change", () => store.set({ stage: Number(slider.value) }));
    bar.append(prev, slider, stageOut, next);
    if (p.best_stage !== null) {
      const best = el("button", { type: "button", class: "button" }, `Go to best stage (${p.best_stage})`);
      best.addEventListener("click", () => store.set({ stage: p.best_stage ?? 0 }));
      bar.append(best);
    }
    if (p.features.length > 1) {
      const select = el("select", { "aria-label": "Predictor on the x axis" }) as HTMLSelectElement;
      p.features.forEach((f, i) => select.append(el("option", { value: String(i), selected: i === store.get().feature ? "" : null }, f)));
      select.addEventListener("change", () => store.set({ feature: Number(select.value) }));
      bar.append(el("label", {}, "x axis: ", select));
    }
    const row = el("input", { type: "number", min: "1", "aria-label": "Observation number" }) as HTMLInputElement;
    row.value = store.get().selected === null ? "" : String((store.get().selected ?? 0) + 1);
    row.addEventListener("change", () => {
      const n = Number(row.value);
      store.set({ selected: row.value && Number.isFinite(n) && n >= 1 ? Math.floor(n) - 1 : null });
    });
    bar.append(el("label", {}, "observation: ", row));
    return bar;
  }

  function exportControls(p: RunPanel): HTMLElement {
    const box = el("section", { class: "export-box", "aria-label": "Packets" });
    const note = el("p", { class: "muted" });
    const make = async (studentOnly: boolean) => {
      if (!current) return;
      note.textContent = "Writing the packet…";
      try {
        const files = await api.exportPacket(lessonId, current.runId, studentOnly);
        let saved = 0;
        for (const [i, plot] of p.plots.entries()) {
          const h = handles[i];
          if (!h?.result) continue;
          const svg = await h.result.view.toSVG();
          await api.saveFigure(files.dir, plot.id, svg);
          saved += 1;
        }
        note.textContent = `Packet written to ${files.dir} (${files.files.length} files, ${saved} figures).`;
        announce("Packet written");
      } catch (e) {
        note.textContent = `Could not write the packet: ${String(e)}`;
      }
    };
    const student = el("button", { type: "button", class: "button" }, "Export student packet");
    student.addEventListener("click", () => void make(true));
    box.append(student);
    const instructorButton = el("button", { type: "button", class: "button" }, "Export instructor packet");
    instructorButton.addEventListener("click", () => void make(false));
    void api.instructor().then((on) => {
      if (on) box.append(instructorButton);
    });
    box.append(note);
    return box;
  }

  async function start(overrides: Record<string, number>): Promise<void> {
    status.textContent = "Fitting…";
    announce("Fitting the experiment");
    const cancel = el("button", { type: "button", class: "button" }, "Cancel");
    status.append(" ", cancel);
    try {
      const runId = await api.startExperiment(lessonId, overrides);
      cancel.addEventListener("click", () => api.cancelExperiment(runId));
      const result = await waitFor(runId);
      if (result === "Done") {
        current = { runId };
        runs.set(lessonId, current);
        store.set({ stage: 10_000, selected: store.get().selected });
        await show();
        announce("Experiment finished");
      } else if (result === "Canceled") {
        status.textContent = "Canceled.";
      } else if (typeof result === "object") {
        status.textContent = `The experiment failed: ${result.Failed}`;
      }
    } catch (e) {
      status.textContent = `Could not start: ${String(e)}`;
    }
  }

  store.subscribe(() => {
    void show();
  });

  if (current) {
    try {
      await show();
    } catch {
      current = undefined;
      runs.clear(lessonId);
    }
  }
  if (!current) {
    // Before the first run: the lesson's own controls; `show()` replaces
    // them with the fitted settings once the run exists.
    const lessonControls = await api.lessonControls(lessonId);
    controlsHost.replaceChildren(controlsFor(lessonControls, {}, (o) => void start(o)));
    status.textContent = "Run the experiment to see its real recorded states.";
    void start({});
  }
}
