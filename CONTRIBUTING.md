# Contributing

This document defines how changes are proposed, written and merged in this repository. It applies to every contributor.

## 1. Branching model

| Branch | Purpose | Created from | Merges into |
|---|---|---|---|
| `main` | Always releasable. Never committed to directly after the initial commit. | n/a | n/a |
| `feature/<short-description>` | New capability, documentation or refactoring | `main` | `main` via pull request |
| `bugfix/<short-description>` | Fix for a defect | `main` | `main` via pull request |

Naming rules:
- Lowercase kebab case, for example `feature/ocr-script-probe` or `bugfix/tamil-prebase-reorder`.
- If an issue exists, put its number first: `feature/42-legacy-font-converter`.
- Keep branches short-lived. Rebase on `main` before opening or updating a pull request.

## 2. Commit messages

Commits follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/).

```
<type>(<optional scope>): <subject>

<body>

<footer>
```

**Types**

| Type | Use for |
|---|---|
| `feat` | A new capability |
| `fix` | A defect fix |
| `docs` | Documentation only |
| `refactor` | Code change that neither fixes a defect nor adds a capability |
| `perf` | Performance improvement |
| `test` | Adding or correcting tests |
| `build` | Dependencies, packaging, build tooling |
| `ci` | Continuous-integration configuration |
| `chore` | Maintenance that does not change behaviour |
| `revert` | Reverting an earlier commit |

**Scopes** in this repository: `cli`, `core`, `script`, `engine`, `ocr`, `pdf`, `sys`, `bench`, `docs`, `repo`.

**Subject line**
- Imperative mood ("add", not "added" or "adds").
- Lowercase, no trailing full stop, at most 72 characters.

**Body**
- Explains *what* changed and *why*, not how.
- Wrapped at 72 characters.
- Uses bullet points for lists of changes.

**Footer**
- References issues (`Refs: #42`, `Closes: #42`).
- Declares breaking changes (`BREAKING CHANGE: <description>`).

**Authorship**
- A commit is authored solely by the person who makes it.
- Commit messages contain no co-author trailers and no references to the tools used to produce the change.

Example:

```
feat(ocr): choose the Tesseract language pack from the page's scripts

Tesseract's orientation-and-script detection labels Sinhala pages as
Latin, which selects the wrong model. A short probe OCR over a central
crop now measures the script mix and selects sin, tam and/or eng.

- add a probe pass with a 0.5 s budget per page
- select every script above 15% of recognised letters
- record the chosen pack in the page provenance

Refs: #12
```

## 3. Pull requests

- Each pull request covers one logical change and targets `main`.
- The title follows the commit-message format, for example `feat(ingest): add resource governor`. It becomes the squash-commit subject.
- The description uses the repository template (`.github/pull_request_template.md`):
  - summary;
  - changes;
  - motivation;
  - testing performed;
  - risks and rollback.
- Descriptions are written in a formal, factual tone. They describe the change and its evidence, not how it was produced.
- Every pull request is reviewed before merge. It is merged with **squash and merge**, and the branch is then deleted.

## 4. What is never committed

- Data of any kind: benchmark corpora, model weights (`*.traineddata`, `*.onnx`, `*.gguf`), extraction output.
- Secrets and local configuration: `.env*`, keys, credentials.
- Configuration and instruction files for AI agents and editor assistants, for example `AGENTS.md`, `CLAUDE.md`, `.claude/`, `.cursor/`, `.github/copilot-instructions.md`. The full list is in `.gitignore`.

## 5. Before opening a pull request

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Changes that affect recognition quality must include before/after results from `lipi bench` on the same corpus.
