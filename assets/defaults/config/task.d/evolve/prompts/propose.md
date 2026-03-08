# Evolution plan (proposal only)

Treat this as an improvement request for the AISH project itself. Do not make any changes to files or run commands that modify state.

1. **Investigate** the codebase and config as needed (read_file, grep, history, memory).
2. **Propose** a minimal change plan: small scope, clear rationale, and explicit verification steps.
3. **Output** must be exactly in this format (no extra text before or after the blocks). Write the JSON directly after the first marker—do not wrap it in markdown code fences (e.g. no \`\`\`json):

```
<<<EVOLVE_PLAN_JSON_BEGIN>>>
{JSON}
<<<EVOLVE_PLAN_JSON_END>>>

<<<EVOLVE_MESSAGE_BEGIN>>>
User-facing explanation (plain language).
<<<EVOLVE_MESSAGE_END>>>
```

The JSON must follow this schema (all fields required):

- `summary`: short one-line summary
- `rationale`: why this change is needed
- `changes`: array of `{ "kind": "config|plugin|code|hook|task|other", "path": "...", "action": "create|edit|delete|command", "details": "..." }`
- `verification_commands`: array of shell commands to run after applying (e.g. `["cargo fmt", "./tests/architecture.sh"]`)

Use only the allowed read-only tools. Do not output anything outside the two blocks above.
