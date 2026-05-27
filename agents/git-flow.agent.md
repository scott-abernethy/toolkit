---
name: git-flow
description: Git commit and GitHub PR workflow — enforces consistent commit format, branch naming, and ensures all changes go via PR
compatibility: opencode
---

## What I do

- Enforce commit message format: `<type>: <ticket#>: <description>`
- Enforce branch naming: `<type>/<ticket#>-<kebabcase-short-description>`
- Create branches and commits following team conventions
- Open PRs with `gh pr create` (never push directly to `main`)
- Extract ticket numbers from branch names automatically

## When to use me

Use for any git or GitHub workflow task:
- Committing changes
- Creating branches
- Opening or checking PRs
- Pushing and raising pull requests

Trigger phrases: "commit", "PR", "pull request", "push", "branch", "open a PR", "raise a PR"

## Conventions

### Commit message format
```
<type>: <ticket#>: <description>
```
- `type`: one of `feat`, `fix`, `chore`
- `ticket#`: JIRA key e.g. `DOG-123` — omit if no ticket
- `description`: short imperative sentence, lowercase, no trailing period

**Examples:**
- `feat: DOG-456: add dividend advice parser`
- `fix: DOG-789: handle missing font fallback in PDF extraction`
- `chore: bump pdfbox to 2.0.27`

### Branch naming format
```
<type>/<ticket#>-<kebabcase-short-description>
```
**Examples:**
- `feat/DOG-456-add-dividend-advice-parser`
- `fix/DOG-789-handle-missing-font-fallback`
- `chore/bump-pdfbox` (no ticket)

### Rules
- **Never commit or push directly to `main`**
- All changes must go via a PR opened with `gh pr create`
- If working under a ticket, include it in both branch name and every commit message

## Workflow

### Step 1 — Gather context
1. Run `git branch --show-current`
2. Extract ticket number from branch name if it matches `<type>/<ticket#>-...`; otherwise ask the user
3. Infer type from branch name; otherwise ask the user to choose `feat / fix / chore`
4. If on `main`, stop and tell the user a branch is needed first

### Step 2 — Create branch (if needed)
If on `main` or branch doesn't follow convention:
1. Ask for a short description for the kebab-case suffix
2. Run: `git checkout -b <type>/<ticket#>-<kebabcase-description>`

### Step 3 — Stage and commit
1. Check changes: `git status` and `git diff --stat`
2. If nothing staged, ask what to stage (suggest `git add -A` or specific paths)
3. Commit: `git commit -m "<type>: <ticket#>: <description>"`

### Step 4 — Push and open PR
1. Push: `git push -u origin <branch>`
2. Open PR: `gh pr create --title "<type>: <ticket#>: <description>" --body "JIRA: <ticket#>" --base main`
3. Report the PR URL to the user

## Edge cases

- **Multiple commits**: repeat Step 3 for each logical change before Step 4
- **Branch already tracked remotely**: use `git push` without `-u`
- **PR already open**: run `gh pr view` rather than creating a duplicate
- **No ticket**: ask before proceeding — never fabricate a ticket number
