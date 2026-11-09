import { api, type CheckPanel, type PredictionView, type QuestionView } from "../api";
import { announce, el } from "../dom";
import { renderMath } from "../math";
import { runs } from "../runs";

function predictionControl(lessonId: string, runId: string, p: PredictionView, refresh: () => void): HTMLElement {
  const box = el("section", { class: "prediction card", "aria-label": "Prediction" });
  const prompt = el("div", { class: "prompt" });
  prompt.innerHTML = p.prompt_html;
  box.append(prompt);
  if (p.revealed) {
    const r = p.revealed;
    box.append(
      el("p", { class: r.correct ? "result ok" : "result" }, `You predicted “${p.committed}”. The actual value is ${r.actual}. ${r.correct ? "Your prediction was right." : "Compare your prediction with the result."}`),
    );
    const expl = el("div", { class: "explanation" });
    expl.innerHTML = r.explanation_html;
    box.append(expl);
    if (!r.saved) box.append(el("p", { class: "muted" }, "Not saved to your record."));
    return box;
  }
  const form = el("form", { class: "predict-form" });
  let input: HTMLInputElement | HTMLSelectElement;
  if (p.kind === "direction") {
    const select = el("select", { "aria-label": "Your prediction" }) as HTMLSelectElement;
    for (const [v, label] of [["up", "It goes up"], ["down", "It goes down"], ["same", "It stays the same"]] as const) select.append(el("option", { value: v }, label));
    input = select;
  } else if ("number" in p.kind) {
    input = el("input", { type: "number", step: "any", required: "", "aria-label": "Your prediction" }) as HTMLInputElement;
  } else {
    const select = el("select", { "aria-label": "Your prediction" }) as HTMLSelectElement;
    for (const o of p.kind.choice.options) select.append(el("option", { value: o }, o));
    input = select;
  }
  const commit = el("button", { type: "submit", class: "button primary" }, "Commit my prediction");
  form.append(input, commit);
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    try {
      const reveal = await api.commitPrediction(lessonId, runId, p.id, input.value);
      announce(reveal.correct ? "Your prediction was right." : `The actual value is ${reveal.actual}.`);
      refresh();
    } catch (err) {
      form.append(el("p", { class: "error" }, String(err)));
    }
  });
  box.append(form);
  return box;
}

function questionControl(lessonId: string, runId: string, q: QuestionView, refresh: () => void): HTMLElement {
  const box = el("section", { class: "question card", "aria-label": `Question on ${q.concept}` });
  const stem = el("div", { class: "stem" });
  stem.innerHTML = q.stem_html;
  box.append(el("p", { class: "card-meta" }, `Concept: ${q.concept}`), stem);
  if (q.locked_until_prediction) {
    box.append(el("p", { class: "notice" }, "Commit the prediction above before answering this question."));
    return box;
  }
  if (q.answered && !q.answered.from_earlier_setting) {
    const a = q.answered;
    box.append(el("p", { class: a.correct ? "result ok" : "result" }, a.correct ? "Correct." : "Not quite."));
    const expl = el("div", { class: "explanation" });
    expl.innerHTML = a.explanation_html;
    box.append(expl);
    if (!a.saved) box.append(el("p", { class: "muted" }, "Not saved to your record."));
    return box;
  }
  if (q.answered?.from_earlier_setting) {
    const earlier = el("details", { class: "earlier" });
    earlier.append(el("summary", {}, `You answered this under an earlier setting (${q.answered.correct ? "correct" : "not correct"} then). Answer again with the current numbers.`));
    const expl = el("div", { class: "explanation" });
    expl.innerHTML = q.answered.explanation_html;
    earlier.append(expl);
    box.append(earlier);
  }
  const form = el("form", { class: "answer-form" });
  const group = el("fieldset");
  group.append(el("legend", { class: "visually-hidden" }, "Options"));
  q.options_html.forEach((html, i) => {
    const id = `${q.id}-opt-${i}`;
    const input = el("input", { type: q.multiple ? "checkbox" : "radio", name: q.id, id, value: String(q.order[i]) });
    const label = el("label", { for: id });
    label.innerHTML = html;
    group.append(el("div", { class: "option" }, input, label));
  });
  const submit = el("button", { type: "submit", class: "button primary" }, "Answer");
  form.append(group, submit);
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const chosen = [...form.querySelectorAll<HTMLInputElement>("input:checked")].map((i) => Number(i.value));
    if (!chosen.length) return;
    try {
      const a = await api.answerQuestion(lessonId, runId, q.id, chosen);
      announce(a.correct ? "Correct." : "Not quite; read the explanation.");
      refresh();
    } catch (err) {
      form.append(el("p", { class: "error" }, String(err)));
    }
  });
  box.append(form);
  return box;
}

/** The Check panel: predictions first, then questions, then the record. */
export async function renderCheck(lessonId: string, host: HTMLElement): Promise<void> {
  host.replaceChildren();
  const current = runs.get(lessonId);
  if (!current) {
    host.append(el("p", { class: "notice" }, "Run the experiment first: the questions use its real numbers."), el("a", { href: `#/lesson/${encodeURIComponent(lessonId)}/run`, class: "button" }, "Go to Run"));
    return;
  }
  const runId = current.runId;
  async function draw(): Promise<void> {
    const panel: CheckPanel = await api.checkPanel(lessonId, runId);
    host.replaceChildren();
    if (panel.record_notice) host.append(el("p", { class: "notice", role: "alert" }, panel.record_notice));
    if (panel.predictions.length) {
      host.append(el("h2", {}, "Predict before you look"));
      for (const p of panel.predictions) host.append(predictionControl(lessonId, runId, p, () => void draw()));
    }
    host.append(el("h2", {}, "Questions"));
    for (const q of panel.questions) host.append(questionControl(lessonId, runId, q, () => void draw()));
    const h = panel.history;
    const record = el("section", { class: "record card", "aria-label": "Your record for this lesson" });
    record.append(
      el("h2", {}, "Your record for this lesson"),
      el("p", {}, `${h.attempts} attempt${h.attempts === 1 ? "" : "s"}, ${h.predictions} prediction${h.predictions === 1 ? "" : "s"}, ${h.correct_answers} of ${h.answers} answers correct.`),
    );
    if (panel.mastered.length) record.append(el("p", {}, `Concepts mastered so far: ${panel.mastered.join(", ")}.`));
    const exportButton = el("button", { type: "button", class: "button" }, "Export my record as JSON");
    exportButton.addEventListener("click", async () => {
      const path = await api.exportRecord();
      record.append(el("p", { class: "muted" }, `Saved to ${path}`));
    });
    record.append(exportButton);
    host.append(record);
    renderMath(host);
  }
  await draw();
}
