---
name: commit-small-features
description: Implement and commit small, self-contained repository features. Use whenever Codex is asked to add, build, implement, or change a modest feature that can be completed and validated in one task, including small UI controls, commands, configuration options, behaviors, endpoints, or focused refactors that directly support the feature. Finish by creating one focused local Git commit unless the user explicitly says not to commit.
---

# Commit Small Features

Treat the Git commit as part of completing a small feature, not as an optional follow-up.

## Workflow

1. Inspect `git status --short` before editing. Treat existing changes as user-owned unless clearly created for the current feature.
2. Implement only the requested feature and any directly necessary tests or documentation.
3. Run the narrowest meaningful validation, then broader checks when warranted by risk or repository guidance.
4. Review the final diff and identify exactly which hunks and untracked files belong to the feature.
5. Stage only those changes. Use path-specific staging when whole files belong to the feature; use patch staging when a file contains unrelated edits.
6. Verify the staged patch with `git diff --cached` and confirm unrelated changes are absent.
7. Create one non-interactive commit with a concise imperative message describing the feature.
8. Report the commit hash, commit subject, and validation performed.

## Commit Rules

- Always commit a successfully completed small feature unless the user explicitly requests no commit.
- Never stage or commit pre-existing, unrelated, or uncertain changes.
- Never discard, overwrite, stash, or clean user changes merely to make the commit easier.
- Never amend an existing commit, push, rebase, or rewrite history unless the user explicitly asks.
- Do not commit when validation fails or the feature is incomplete. Explain the blocker and leave the worktree intact.
- If current-task changes overlap inseparably with unrelated edits in the same hunk, do not guess. Leave them uncommitted and explain why a safe focused commit could not be made.
- Include directly related tests, lockfile changes, generated files, and documentation only when they genuinely belong to the feature.

## Scope Judgment

Consider a feature small when it has one clear outcome, a reviewable focused diff, and can reasonably be represented by one commit. For a larger multi-feature request, apply the repository's normal planning and commit conventions instead of forcing everything into a single commit.
