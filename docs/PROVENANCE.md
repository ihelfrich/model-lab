# Provenance

This repository was created on 2026-11-07 from the untracked source tree at
`/Users/ian/Documents/ChatGPT/Tutoring/outputs/tree-ensemble-lab-rust/`
(Model Lab 0.8.0, formerly Tree Ensemble Lab), which Codex sessions built
between 2026-11-02 and 2026-11-05. That folder, including `Releases/`,
`dist/` and `student-delivery/`, remains the archive of delivered artifacts and is
not modified from here. Build outputs, virtual environments, release
archives, screenshots and private provenance files were not imported.

Windows CI for 0.8 ran against commit `0ed26e41` of the throwaway GitHub
repository `ihelfrich/tree-ensemble-lab`, pushed from a `/private/tmp`
staging copy. Nothing ties the locally built Mac release or the source zip
to that commit; this repository's first commit is the first durable record
of the source.

## Model Lab 0.8.0 release archive checksums (SHA-256)

Taken from `Releases/0.8.0/verification-release.json` in the archive folder.

| File | SHA-256 |
| --- | --- |
| Model-Lab-0.8-Mac-Apple-Silicon.zip | 1e4e664edf65395a477e92bb36cb7b2d2eafdb5c6029167d6b9cbab274ab5061 |
| Model-Lab-0.8-Source.zip | 481eb0dc7bd797032b2452a9de15eb3b2432d6d29ced35a4034d3692b0057ef5 |
| Model-Lab-0.8-Windows.zip (shipped, local repack) | f1988d80a5aadf44ae74fa4d6b34ed63739c10580c19bb2f0e00cc0385153d31 |
| Windows CI artifact (original, before repack) | 2f4b27a77c25a5b8b882ca1c360ad1d53734e37ef27dbdd7f8159be7c88777b3 |
| Model-Lab-0.8-Worked-Example.zip | bbce59076329b0689809e87884221ea1ede244d38c3bf8d558814ee1b5162e86 |
| Frozen protocol-1 worker (`python/research_worker.py`) | 3522d9b34131b8f551ecd2b499690a62cda1b4e8058be5c0cd8d3e891ac35406 |

The shipped Windows zip differs from the CI artifact because notes and
licences were appended locally after CI; the claim that the executable and
runtime entries were preserved is self-reported in
`verification-windows-package.json`.
