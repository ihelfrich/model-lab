# The legacy tree laboratory

`src/ui.rs` is the complete "Tree Ensemble Lab 0.5" interface that was
delivered to a student on 2026-11-03. It still ships inside Model Lab behind
the "Open tree laboratory" button and the `--legacy-tree-lab` flag.

## Status: frozen (2026-11-07)

The legacy surface receives bug fixes only. No new features, content or
visual changes land in `ui.rs`, `studio.rs`, `instrument.rs` or `quiz.rs`.
The delivered 0.5 archive and its checksums remain the reference for what
the student received.

## What migrates, and where

These interactions are the best teaching mechanisms in the product and are
kept, but they move into the new shell rather than being extended here:

| Legacy mechanism | Location today | Destination |
| --- | --- | --- |
| "Predict the next move" before a boosting round | `ui.rs` Explore | first lesson of the new shell |
| `old + rate × h = new` arithmetic for the selected observation | `ui.rs` Explore | observation panel of every tree lesson |
| Add-one-tree stepping with real recorded stage predictions | `ui.rs` playback | shared replay control |
| Four guided experiments and pinned comparison | `ui.rs` Labs | lesson experiments |
| 100-question bank with explanations | `quiz.rs`, `questions.json` | one question system, per-lesson, persisted |
| OOB error, permutation importance, PDP and ICE | `science.rs` via Science tab | diagnostics panel, with the slice-vs-PDP distinction kept |
| Transparent standard-library Python with traced lines | `python.rs` | job supervisor, with limits |

Known defects in the legacy surface that are not fixed here because the
surface is frozen: dark theme forced, no screen-reader support on Mac, six
hand-drawn plot routines, integer-coded tabs, and a background fit started
at every launch even when the laboratory is never opened. They are recorded
in the platform design document.
