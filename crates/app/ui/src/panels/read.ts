import { api } from "../api";
import { renderMath } from "../math";
import { el } from "../dom";

/** The Read panel: lesson text, mathematics and a derivation revealed step by step. */
export async function renderRead(id: string, host: HTMLElement): Promise<void> {
  const panel = await api.readPanel(id);
  host.replaceChildren();
  if (panel.status) {
    host.append(el("p", { class: "notice", role: "note" }, `Status: ${panel.status}.`));
  }
  if (panel.missing.length) {
    host.append(
      el(
        "p",
        { class: "notice", role: "note" },
        `Run the experiment to fill in: ${panel.missing.join(", ")}.`,
      ),
    );
  }
  const article = el("article", { class: "lesson-text" });
  article.innerHTML = panel.html;
  host.append(article);
  if (panel.derivation_html.length) {
    const section = el("section", { class: "derivation", "aria-label": "Derivation" });
    section.append(el("h2", {}, "Derivation"));
    const steps: HTMLElement[] = panel.derivation_html.map((html, i) => {
      const step = el("div", { class: "derivation-step", hidden: i === 0 ? null : "" });
      step.innerHTML = html;
      return step;
    });
    const button = el("button", { type: "button", class: "button" }, "Reveal the next step");
    let shown = 1;
    button.addEventListener("click", () => {
      const next = steps[shown];
      if (next) {
        next.hidden = false;
        shown += 1;
      }
      if (shown >= steps.length) {
        button.disabled = true;
        button.textContent = "All steps shown";
      }
    });
    if (steps.length <= 1) {
      button.disabled = true;
      button.textContent = "All steps shown";
    }
    section.append(...steps, button);
    host.append(section);
  }
  if (panel.instructor_html) {
    const notes = el("section", { class: "instructor-notes", "aria-label": "Instructor notes" });
    notes.append(el("h2", {}, "Instructor notes"));
    const body = el("div");
    body.innerHTML = panel.instructor_html;
    notes.append(body);
    host.append(notes);
  }
  renderMath(host);
}
