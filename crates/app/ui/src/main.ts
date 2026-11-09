import "katex/dist/katex.min.css";
import "./styles.css";
import { api, type LessonCard } from "./api";
import { el, announce } from "./dom";
import { applyTheme } from "./theme";
import { renderRead } from "./panels/read";

type Panel = "read" | "run" | "check";

interface Route {
  kind: "curriculum" | "lesson" | "jobs";
  id?: string;
  panel?: Panel;
}

function parseRoute(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  if (parts[0] === "lesson" && parts[1]) {
    const panel = (["read", "run", "check"] as Panel[]).find((p) => p === parts[2]) ?? "read";
    return { kind: "lesson", id: decodeURIComponent(parts[1]), panel };
  }
  if (parts[0] === "jobs") return { kind: "jobs" };
  return { kind: "curriculum" };
}

const header = document.getElementById("header") as HTMLElement;
const main = document.getElementById("main") as HTMLElement;

async function renderHeader(): Promise<void> {
  const instructor = await api.instructor().catch(() => false);
  header.replaceChildren(
    el("a", { href: "#/", class: "brand" }, "Model Lab"),
    el("nav", { "aria-label": "Primary" }, el("a", { href: "#/" }, "Curriculum"), el("a", { href: "#/jobs" }, "Jobs")),
    (() => {
      const label = el("label", { class: "toggle" });
      const box = el("input", { type: "checkbox" }) as HTMLInputElement;
      box.checked = instructor;
      box.addEventListener("change", async () => {
        await api.setInstructor(box.checked);
        announce(box.checked ? "Instructor mode on" : "Instructor mode off");
        render();
      });
      label.append(box, " Instructor mode");
      return label;
    })(),
  );
}

function lessonCard(card: LessonCard): HTMLElement {
  const item = el("li", { class: "card" });
  item.append(el("a", { href: `#/lesson/${encodeURIComponent(card.id)}/read`, class: "card-title" }, card.title));
  const meta = [`${card.level}`, `${card.estimated_minutes} min`];
  item.append(el("p", { class: "card-meta" }, meta.join(" · ")));
  if (card.prerequisite_titles.length) {
    item.append(el("p", { class: "card-meta" }, `Assumes: ${card.prerequisite_titles.join(", ")}`));
  }
  if (card.status) item.append(el("p", { class: "card-meta" }, `Status: ${card.status}`));
  return item;
}

async function renderCurriculum(): Promise<void> {
  const [lessons, errors] = await Promise.all([api.listLessons(), api.lessonErrors()]);
  main.replaceChildren(el("h1", {}, "Curriculum"));
  if (!lessons.length) {
    main.append(el("p", { class: "notice" }, "No lessons found in the bundled lessons folder or your own lessons folder."));
  }
  main.append(el("ul", { class: "cards" }, ...lessons.map(lessonCard)));
  if (errors.length) {
    const list = el("ul");
    for (const e of errors) list.append(el("li", {}, `${e.file}: ${e.field}: ${e.message}`));
    main.append(el("section", { class: "errors", "aria-label": "Lessons that did not load" }, el("h2", {}, "Lessons that did not load"), list));
  }
}

async function renderLesson(id: string, panel: Panel): Promise<void> {
  main.replaceChildren();
  const tabs = el("nav", { class: "tabs", "aria-label": "Lesson panels" });
  for (const p of ["read", "run", "check"] as Panel[]) {
    const link = el("a", { href: `#/lesson/${encodeURIComponent(id)}/${p}`, "aria-current": p === panel ? "page" : null }, p[0]!.toUpperCase() + p.slice(1));
    tabs.append(link);
  }
  const host = el("section", { class: "panel", id: "panel" });
  main.append(tabs, host);
  if (panel === "read") {
    await renderRead(id, host);
  } else {
    host.append(el("p", { class: "notice" }, `The ${panel} panel arrives with the next task.`));
  }
}

async function render(): Promise<void> {
  const route = parseRoute(location.hash);
  await renderHeader();
  try {
    if (route.kind === "lesson" && route.id) await renderLesson(route.id, route.panel ?? "read");
    else if (route.kind === "jobs") main.replaceChildren(el("h1", {}, "Jobs"), el("p", { class: "notice" }, "Supervised jobs arrive with a later task."));
    else await renderCurriculum();
  } catch (e) {
    main.replaceChildren(el("h1", {}, "Something did not load"), el("pre", { class: "error" }, String(e)));
  }
  main.focus();
}

applyTheme();
window.addEventListener("hashchange", render);
render();
