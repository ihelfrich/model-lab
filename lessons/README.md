# Lessons

One directory per lesson: `lesson.md` (YAML front matter and Markdown with
`$…$` mathematics and `{{live.numbers}}`), `experiment.json`, `questions.json`,
optional `derivation.md` (steps separated by `---`) and `instructor.md`.
The format is defined and validated by `crates/lessons/src/lesson.rs`.
These directories are bundled with the application; a learner's own lessons
folder in the application data directory is read after them and overrides by id.
