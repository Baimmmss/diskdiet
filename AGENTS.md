# Instructions for every agent and session

These rules apply to every project folder in this workspace (for example `mannerly/`, `DiskDiet/`). Follow them in each session, before doing anything else.

## 1. Read first

Before you plan or change anything in a project folder, read these two files in that folder:

- **`PRD.md`**: the product requirements. What the project is, who it's for, goals, non-goals, requirements, risks, milestones, open questions and research notes. Treat it as the source of truth for *what* to build.
- **`log.json`**: the change log. What has already been changed, in which files, and why. Read it so you don't redo or undo past work.

If the user's request conflicts with `PRD.md`, point out the conflict and ask before going ahead.

## 2. Create them if missing

If the project folder you're working in has no `PRD.md` or no `log.json`, create it before making other changes:

- `PRD.md`: fill it from what you can learn (README, code, the user's request). Use the same sections as `mannerly/PRD.md`: a status table (version, status, updated), Problem, Target users, Goals, Non-goals, Success metrics, Requirements (Must / Should / Could), Risks, Milestones, Open questions. Put anything you guessed under Open questions.
- `log.json`: start it with one entry describing the folder's current state (and any change you're about to make).

## 3. Log every change

`log.json` records **only changes**, like `git log`. No plans, research or ideas: those go in `PRD.md`.

After each set of changes, add one entry at the **top** of `entries` (newest first):

```json
{
  "id": 8,
  "date": "YYYY-MM-DD",
  "summary": "Short imperative line, like a commit subject",
  "files": { "added": [], "modified": [], "deleted": [] },
  "why": "What changed and why, in a sentence or three."
}
```

- `id` is one more than the current highest id.
- File paths are relative to the project folder.
- Use real dates (no "today" or "yesterday").
- Keep the file valid JSON (`python3 -m json.tool log.json`).

## 4. Keep PRD.md current

When a decision changes the product (a new requirement, a dropped feature, a finished milestone, an answered open question), update `PRD.md` and its Updated date in the same session. Log that edit in `log.json` too.

## 5. Other rules

- Don't publish anything (git push, crates.io, npm, releases) unless the user asks in this session.
- Don't commit unless the user asks.
