---
description: Generate a commit message from staged changes
---

Analyze the currently staged git changes and generate a concise, descriptive commit message following conventional commit format.

Steps:
1. Run `git diff --cached` to see all staged changes
2. Run `git log -5 --oneline` to understand the commit message style used in this repository
3. Analyze the changes to understand:
   - What type of change this is (feat, fix, refactor, docs, test, chore, perf, style)
   - What area/module is affected
   - The purpose and impact of the changes
4. Generate a commit message that:
   - Uses conventional commit format: `type(scope): description`
   - Has a clear, concise subject line (50-72 chars)
   - Focuses on WHY the change was made, not just WHAT changed
   - Includes a body with additional details if the change is complex
   - Matches the style and tone of recent commits in the repository

Output only the commit message text, ready to be used with `git commit -F`.
