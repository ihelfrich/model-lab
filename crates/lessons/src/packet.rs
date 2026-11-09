//! Student and instructor packets: self-contained HTML plus the data, the
//! specification and the results, written to a directory. The student
//! packet never carries answers, explanations or instructor notes.
use crate::lesson::Lesson;
use crate::markdown::to_html;
use crate::template::{render, Context};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// What the packet is built from; the shell assembles it from a run.
pub struct PacketInput {
    pub context: Context,
    /// `Run::to_json` for a fitted run, or the by-hand rounds.
    pub results: Value,
    pub data_csv: String,
    /// Figure ids the shell will save as `figures/<id>.svg` next.
    pub figure_ids: Vec<String>,
    pub figure_titles: Vec<String>,
}

pub struct PacketFiles {
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
}

const CSS: &str = "body{font-family:system-ui,sans-serif;max-width:46rem;margin:2rem auto;padding:0 1rem;line-height:1.5;color:#1b1f24}\
h1,h2{line-height:1.2}figure{margin:1.5rem 0}img{max-width:100%}.math{font-family:ui-monospace,monospace}\
.option{margin:.2rem 0}.key{border-left:4px solid #F2A541;padding-left:.8rem;margin:.6rem 0}table{border-collapse:collapse}td,th{border:1px solid #d5dbe1;padding:.2rem .6rem}";

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // yyyymmdd-hhmm from a civil-date conversion (UTC).
    let days = secs / 86400;
    let (h, m) = ((secs % 86400) / 3600, (secs % 3600) / 60);
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}")
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>{}</title><style>{CSS}</style></head><body>{body}</body></html>\n",
        escape(title)
    )
}

fn lesson_sections(lesson: &Lesson, ctx: &Context, input: &PacketInput) -> String {
    let mut out = String::new();
    out.push_str(&format!("<h1>{}</h1>", escape(&lesson.front.title)));
    out.push_str(&to_html(&render(&lesson.body, ctx).text));
    if !lesson.derivation.is_empty() {
        out.push_str("<h2>Derivation</h2>");
        for (i, step) in lesson.derivation.iter().enumerate() {
            out.push_str(&format!("<h3>Step {}</h3>{}", i + 1, to_html(&render(step, ctx).text)));
        }
    }
    if !input.figure_ids.is_empty() {
        out.push_str("<h2>Figures</h2>");
        for (id, title) in input.figure_ids.iter().zip(&input.figure_titles) {
            out.push_str(&format!(
                "<figure><img src=\"figures/{id}.svg\" alt=\"{}\"><figcaption>{}</figcaption></figure>",
                escape(title),
                escape(title)
            ));
        }
    }
    out.push_str("<h2>Experiment</h2><p>The settings, the data and the results are beside this page as <code>experiment.json</code>, <code>data.csv</code> and <code>results.json</code>.</p>");
    out
}

fn student_questions(lesson: &Lesson, ctx: &Context) -> String {
    let mut out = String::new();
    if !lesson.predictions.is_empty() {
        out.push_str("<h2>Predict before you look</h2>");
        for p in &lesson.predictions {
            out.push_str(&format!("<div class=\"prompt\">{}</div>", to_html(&render(&p.prompt, ctx).text)));
        }
    }
    out.push_str("<h2>Questions</h2>");
    for (i, q) in lesson.questions.iter().enumerate() {
        out.push_str(&format!("<h3>Question {}</h3>{}", i + 1, to_html(&render(&q.stem, ctx).text)));
        for (j, o) in q.options.iter().enumerate() {
            out.push_str(&format!("<div class=\"option\">{}. {}</div>", (b'a' + j as u8) as char, to_html(&render(o, ctx).text)));
        }
    }
    out
}

fn instructor_keys(lesson: &Lesson, ctx: &Context) -> String {
    let mut out = String::new();
    if let Some(notes) = &lesson.instructor_notes {
        out.push_str("<h2>Instructor notes</h2>");
        out.push_str(&to_html(&render(notes, ctx).text));
    }
    if !lesson.predictions.is_empty() {
        out.push_str("<h2>Predictions: what the experiment reveals</h2>");
        for p in &lesson.predictions {
            let actual = ctx.get(&p.reveals).map(|v| v.to_string()).unwrap_or_else(|| format!("[missing: {}]", p.reveals));
            out.push_str(&format!(
                "<div class=\"key\">{}<p>Reveals <code>{}</code> = {}</p>{}</div>",
                to_html(&render(&p.prompt, ctx).text),
                escape(&p.reveals),
                escape(&actual),
                p.explanation.as_deref().map(|e| to_html(&render(e, ctx).text)).unwrap_or_default()
            ));
        }
    }
    out.push_str("<h2>Answer keys</h2>");
    for (i, q) in lesson.questions.iter().enumerate() {
        out.push_str(&format!("<h3>Question {} ({})</h3>{}", i + 1, escape(&q.concept), to_html(&render(&q.stem, ctx).text)));
        for (j, o) in q.options.iter().enumerate() {
            let mark = if q.correct.contains(&j) { " <strong>(answer)</strong>" } else { "" };
            out.push_str(&format!("<div class=\"option\">{}. {}{mark}</div>", (b'a' + j as u8) as char, to_html(&render(o, ctx).text)));
        }
        out.push_str(&format!("<div class=\"key\">{}</div>", to_html(&render(&q.explanation, ctx).text)));
    }
    out
}

/// Write `<out_root>/<lesson_id>-<stamp>/` with `student.html`,
/// `experiment.json`, `data.csv`, `results.json`, `figures/README.txt`, and
/// `instructor.html` when `instructor` is true.
pub fn write(lesson: &Lesson, input: &PacketInput, instructor: bool, out_root: &Path) -> Result<PacketFiles, String> {
    let dir = out_root.join(format!("{}-{}", lesson.front.id, stamp()));
    std::fs::create_dir_all(dir.join("figures")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files = vec![];
    let mut put = |name: &str, bytes: &[u8]| -> Result<(), String> {
        let p = dir.join(name);
        std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?;
        files.push(p);
        Ok(())
    };
    let ctx = &input.context;
    let student = page(&lesson.front.title, &format!("{}{}", lesson_sections(lesson, ctx, input), student_questions(lesson, ctx)));
    put("student.html", student.as_bytes())?;
    if instructor {
        let body = format!("{}{}", lesson_sections(lesson, ctx, input), instructor_keys(lesson, ctx));
        put("instructor.html", page(&format!("{} (instructor)", lesson.front.title), &body).as_bytes())?;
    }
    let experiment = std::fs::read(lesson.dir.join("experiment.json")).unwrap_or_default();
    put("experiment.json", &experiment)?;
    put("data.csv", input.data_csv.as_bytes())?;
    put("results.json", &serde_json::to_vec_pretty(&input.results).map_err(|e| e.to_string())?)?;
    let readme = format!(
        "Figures the application saves beside this file, one SVG per plot:\n{}\n",
        input.figure_ids.iter().map(|id| format!("{id}.svg")).collect::<Vec<_>>().join("\n")
    );
    put("figures/README.txt", readme.as_bytes())?;
    Ok(PacketFiles { dir, files })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lesson::load_dir;
    use serde_json::json;

    fn fixture() -> Lesson {
        load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/with-instructor")).unwrap()
    }
    fn input() -> PacketInput {
        PacketInput {
            context: Context(json!({"byhand": {"round": [{"mse_before": 10.0, "mse_after": 3.25}]}, "selected": {"h": -3.0, "before": 6.0, "after": 4.5}})),
            results: json!({"rounds": 3}),
            data_csv: "x,y\n1,2\n2,4\n3,8\n4,10\n".into(),
            figure_ids: vec!["byhand_rounds".into()],
            figure_titles: vec!["Predictions by round".into()],
        }
    }
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("model-lab-packet-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn student_packet_has_no_answers_or_notes() {
        let lesson = fixture();
        let files = write(&lesson, &input(), false, &tmp("student")).unwrap();
        let student = std::fs::read_to_string(files.dir.join("student.html")).unwrap();
        assert!(!student.contains("correct"), "{student}");
        assert!(!student.contains("depth-one tree"));
        assert!(!student.contains("Watch for the student"));
        assert!(!files.files.iter().any(|f| f.ends_with("instructor.html")));
        assert!(student.contains("Question 1"));
        assert!(files.dir.join("data.csv").is_file() && files.dir.join("results.json").is_file() && files.dir.join("figures/README.txt").is_file());
    }

    #[test]
    fn instructor_packet_renders_live_numbers() {
        let lesson = fixture();
        let files = write(&lesson, &input(), true, &tmp("instructor")).unwrap();
        let instructor = std::fs::read_to_string(files.dir.join("instructor.html")).unwrap();
        assert!(instructor.contains("3.2500"), "{instructor}");
        assert!(instructor.contains("Watch for the student"));
        assert!(instructor.contains("(answer)"));
    }

    #[test]
    fn packet_is_self_contained() {
        let lesson = fixture();
        let files = write(&lesson, &input(), true, &tmp("contained")).unwrap();
        for name in ["student.html", "instructor.html"] {
            let html = std::fs::read_to_string(files.dir.join(name)).unwrap();
            assert!(!html.contains("http://") && !html.contains("https://") && !html.contains("src=\"/"), "{name}");
        }
    }
}
