# TokenZero Universal Routing

Status: design (branch `opt/universal-routing`). Grounded in harness-docs research as of 2026-06; confidence per harness noted inline. Low/medium-confidence items are marked — do not treat them as verified harness capabilities.

## 1. Goal

Every agent tool call, on any AI harness, routes through TokenZero. The contract is RTK-style **passthrough-by-default**:

- **Never break a call.** If TokenZero cannot improve a command or read, it executes/serves it unchanged. All interception layers fail open.
- **Compress when profitable.** Compact renderings are emitted only when strictly cheaper than raw (existing invariant: capsules never cost more than raw; `pick_cheaper`).
- **Always recoverable.** Exact bytes stay one `tz_expand` away behind `tz://` refs.
- **Invisible.** No harness-visible behavior change beyond smaller tool results.

Prior art validates the thesis. RTK (`rtk-ai/rtk`, ~60k stars) is a Rust CLI proxy claiming 60–90% token reduction on 100+ dev commands at <10 ms overhead, with three integration patterns selected per harness by `rtk init`: (1) hook-based auto-rewrite, (2) instruction/rules injection where hooks don't exist, (3) plugin/rules-file adapters. RTK's documented limitation is also ours: harness built-in tools (Read/Grep/Glob) bypass shell hooks — which is why TokenZero pairs hooks with the MCP tool surface and the PATH shim layer below. Related prior art: `atlassian-labs/mcp-compressor` (MCP-proxy pattern), `ppgranger/token-saver` (per-file-type output compression).

## 2. Mechanism matrix

| Harness | Interception mechanism | Adapter | Can rewrite command? | Confidence |
|---|---|---|---|---|
| Claude Code | `PreToolUse` hook, `updatedInput` rewrite | native-hook | Yes | High |
| Gemini CLI | `BeforeTool` hook (v0.26.0+, on by default); `hookSpecificOutput.tool_input` merges over model args | native-hook | Yes | High |
| Codex CLI | `PreToolUse` hook (v0.124.0+, `[features] hooks = true`), `updatedInput`; plus `[shell_environment_policy]` env injection | native-hook + env shim | Yes (gap: `unified_exec` calls) | High |
| Cursor | `preToolUse` hook `updated_input` rewrite; `beforeShellExecution` allow/deny/ask | native-hook | Yes (`preToolUse` only) | High |
| Windsurf (Devin Desktop) | Cascade hooks block-only (exit 2); no rewrite, no context injection | env+path-shim (+ hook backstop) | No | High |
| Cline | `PreToolUse` script: cancel + `contextModification`; no parameter mutation | native-hook (deny+steer) + rc shim | No | High |
| Roo Code | None (EOL 2026-05-15, repo archived; no hooks shipped) | env+path-shim | No | Medium |
| Factory droid | `PreToolUse` in `hooks.json`, `updatedInput`; shell tool = `Execute` | native-hook | Yes | High |
| OpenCode | Plugin `tool.execute.before` mutates `output.args`; `shell.env` injects env | native-hook (plugin) | Yes | High (caveats below) |
| Crush | `PreToolUse` hook, `updated_input`; accepts Claude Code hook output format | native-hook | Yes | High |
| Aider | None (no hooks/plugins/native MCP) | env+path-shim + conventions file | No | Medium |
| Zed | None (hooks are an open request); MCP `context_servers` + `.rules`; ACP `agent_servers` wrapper is the only true seam | mcp-only | No | Medium |

### Claude Code — native-hook (High) — implemented

Status: implemented as `tokenzero hook claude-code`. `PreToolUse` matcher `Bash`; the adapter reads the hook JSON on stdin and rewrites `tool_input.command` to `<tokenzero> run -- sh -c '<original>'` (single-quote escaped; `run --stdin` pipes to the child's stdin, so heredoc delivery is not usable). Fail-open: malformed input, non-Bash tools, and internal errors always exit 0 with no output. Skips: commands mentioning `tokenzero`, `cd ...`, heredocs, trailing `&`, interactive programs (vim/ssh/sudo/...), and `TOKENZERO_NO_WRAP`. Modes: `--mode rewrite|guide|off` (guide denies with a steer toward TokenZero MCP tools). Sibling `tool_input` keys (timeout, description) are preserved in `updatedInput`. Conformance: `crates/tokenzero/tests/passthrough_conformance.rs` (exit-code parity via `sh -c`, content recovery via `tokenzero expand` of `combined_ref`). Full contract in §3. Installer: the `hooks` capability (`tokenzero install --hooks`, included in the `clients` standard profile) merge-patches only the TokenZero-owned `PreToolUse` entry into `.claude/settings.json` — claude-scoped, idempotent, with plan/apply/rollback/detect parity with the MCP surface.

```json
// ~/.claude/settings.json (user scope so subagents are covered)
{ "hooks": { "PreToolUse": [ { "matcher": "Bash", "hooks": [
  { "type": "command", "command": "~/.tokenzero/bin/tokenzero hook claude-code", "timeout": 10 }
] } ] } }
```

Sources: https://code.claude.com/docs/en/hooks.md, https://code.claude.com/docs/en/hooks-guide.md, https://code.claude.com/docs/en/agent-sdk/hooks.md

### Gemini CLI — native-hook (High)

Hooks shipped enabled-by-default in v0.26.0 (2026-01-28). `BeforeTool` matcher is a regex on tool name (`run_shell_command`; MCP tools match as `mcp_<server>_<tool>`). Hook stdin JSON mirrors Claude Code's; stdout `hookSpecificOutput.tool_input` **merges with and overrides** model args, so the hook can rewrite `command` to `tokenzero run -- <orig>`. `decision:"deny"`/exit 2 blocks. Settings precedence: system-defaults < user `~/.gemini/settings.json` < project `.gemini/settings.json` < system overrides.

```json
// .gemini/settings.json
{ "hooks": { "BeforeTool": [ { "matcher": "run_shell_command", "hooks": [
  { "type": "command", "name": "tokenzero-route",
    "command": "$GEMINI_PROJECT_DIR/.gemini/hooks/tz-route.sh", "timeout": 10000 }
] } ] } }
// tz-route.sh stdout: {"decision":"allow","hookSpecificOutput":{"tool_input":{"command":"tokenzero run -- <orig>"}}}
```

Env fallback: Gemini auto-loads `.env` (project `.gemini/.env` preferred, never filtered); dotenv values are literal — `$PATH` does not expand, so a shim PATH must be written as a full string. No `shell_environment_policy` equivalent. `tools.sandbox` custom command can wrap the whole CLI (heavyweight).

Sources: https://geminicli.com/docs/hooks/reference/, https://geminicli.com/docs/hooks/, https://developers.googleblog.com/tailor-gemini-cli-to-your-workflow-with-hooks/, https://google-gemini.github.io/gemini-cli/docs/get-started/configuration.html, https://google-gemini.github.io/gemini-cli/docs/cli/sandbox.html, https://github.com/google-gemini/gemini-cli/discussions/17790

### Codex CLI — native-hook + env shim (High)

Hooks stable since v0.124.0, gated by `[features] hooks = true`; config in `~/.codex/hooks.json` or `[hooks]` tables in `config.toml` (user, repo, plugin, enterprise scopes). `PreToolUse` matcher regex on tool name (`^Bash$`, `apply_patch`, `mcp__*`); stdout can deny or rewrite via `permissionDecision:"allow"` + `updatedInput:{command}`. **Gaps:** PreToolUse does not intercept WebSearch or `unified_exec` shell calls — pair with the PATH shim. Non-managed hooks need one-time trust via `/hooks`.

```toml
# ~/.codex/config.toml
[features]
hooks = true

[shell_environment_policy]            # covers the unified_exec gap
inherit = "all"
ignore_default_excludes = true        # default excludes strip *TOKEN* vars, incl. TOKENZERO_*
set = { TOKENZERO_SHIM = "1", PATH = "/absolute/home/.tokenzero/shims:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin" }  # literal value — no $PATH or ~ expansion; spell out your home directory

[[hooks.PreToolUse]]
matcher = "^Bash$"
[[hooks.PreToolUse.hooks]]
type = "command"
command = "~/.tokenzero/bin/tz-codex-hook"
timeout = 30
```

Warning: `shell_environment_policy` default excludes drop vars matching `*KEY*`/`*SECRET*`/`*TOKEN*` — `TOKENZERO_*` contains `TOKEN`, so inherited `TOKENZERO_SHIM` is silently dropped unless injected via `set` and/or `ignore_default_excludes = true`.

Sources: https://developers.openai.com/codex/hooks, https://developers.openai.com/codex/config-reference, https://developers.openai.com/codex/config-advanced, https://github.com/openai/codex/releases, https://developers.openai.com/codex/changelog

### Cursor — native-hook (High)

`hooks.json` at `~/.cursor/hooks.json` / `<project>/.cursor/hooks.json` / enterprise paths (all merge). `preToolUse` **can rewrite** tool inputs via `{"updated_input":{"command":"..."}}`; `beforeShellExecution` can only allow/deny/ask + steer (`agent_message`). `postToolUse` can rewrite MCP outputs and inject context. Hooks apply to desktop, CLI, and cloud agents. Exit 0 = use JSON, exit 2 = block, other = fail-open (`failClosed` available per hook).

```json
// ~/.cursor/hooks.json
{ "version": 1, "hooks": {
  "preToolUse": [ { "command": "~/.tokenzero/hooks/cursor-rewrite", "timeout": 10 } ]
} }
// cursor-rewrite stdout: { "updated_input": { "command": "tokenzero run -- <orig>" } }
```

Sandbox interplay (2.0+): shell commands run under Seatbelt/Landlock; shims must live in sandbox-readable paths and any tz daemon socket may need allowlisting in `~/.cursor/sandbox.json`. Agent shells set `CURSOR_AGENT=1` — usable as an rc-file shim guard; `cursor-agent` CLI inherits the invoking shell env.

Sources: https://cursor.com/docs/hooks, https://blog.gitbutler.com/cursor-hooks-deep-dive, https://cursor.com/docs/agent/tools/terminal, https://cursor.com/blog/agent-sandboxing, https://cursor.com/docs/cli/reference/permissions, https://cursor.com/docs/mcp

### Windsurf / Devin Desktop — env+path-shim, hook backstop (High)

Cascade Hooks (Nov 2025; docs now redirect to docs.devin.ai after the Cognition rebrand) support 12 events incl. `pre_run_command`, but **cannot rewrite commands and cannot inject agent context** — exit 2 blocks (stderr shown to user), exit 0 allows. So the wrap layer is a PATH shim: on macOS Cascade uses a dedicated terminal that always runs zsh and sources zsh config (VS Code `terminal.integrated.env` does NOT apply) — shim activation goes in `~/.zshenv`. Hook = enforcement backstop only.

```sh
# ~/.zshenv
export TOKENZERO_SHIM=1
export PATH="$HOME/.tokenzero/shims:$PATH"
```
```json
// ~/.codeium/windsurf/hooks.json — block-only backstop
{ "hooks": { "pre_run_command": [
  { "command": "~/.tokenzero/hooks/windsurf-guard.sh", "show_output": false }
] } }
```

MCP fully supported at `~/.codeium/windsurf/mcp_config.json` (100-tool cap); `pre_mcp_tool_use` covers MCP calls.

Sources: https://docs.devin.ai/desktop/cascade/hooks, https://docs.devin.ai/desktop/terminal, https://docs.devin.ai/desktop/cascade/mcp, https://www.digitalapplied.com/blog/windsurf-swe-1-5-cascade-hooks-november-2025

### Cline — native-hook (deny+steer) + rc shim (High)

Hooks v3.36+ (macOS/Linux only; enable in Settings > Features): executable scripts named exactly after the event in `~/Documents/Cline/Rules/Hooks/` or `.clinerules/hooks/`. `PreToolUse` stdin carries `{preToolUse:{toolName,parameters}}` (shell = `execute_command`); stdout `{"cancel":bool,"errorMessage":str,"contextModification":str}` — **no parameter rewriting**, so the adapter denies raw `cat/grep/find/...` and steers to tz tools via `contextModification`, with the PATH shim as the wrap-instead-of-deny layer. Cline spawns the real shell via VS Code shell integration, so rc-file shims work; **terminal-profile env vars are NOT respected** (bug #7793) — inject via rc files. CLI variant inherits the invoking shell env.

```bash
# .clinerules/hooks/PreToolUse  (chmod +x, no extension)
#!/usr/bin/env bash
input=$(cat); tool=$(jq -r '.preToolUse.toolName' <<<"$input")
cmd=$(jq -r '.preToolUse.parameters.command // empty' <<<"$input")
if [[ "$tool" == "execute_command" && "$cmd" =~ ^(cat|head|tail|grep|rg|find|ls)\  ]]; then
  echo '{"cancel": true, "errorMessage": "Blocked raw command", "contextModification": "Use tokenzero MCP tools (tz_shell / tz_read / tz_grep) instead."}'
else echo '{"cancel": false}'; fi
```

Sources: https://docs.cline.bot/features/hooks/hook-reference, https://cline.ghost.io/cline-v3-36-hooks/, https://github.com/cline/cline/pull/6440, https://github.com/cline/cline/issues/7793, https://docs.cline.bot/mcp/configuring-mcp-servers

### Roo Code — env+path-shim (Medium; deprioritized)

**Product EOL 2026-05-15; repo archived.** No hooks ever shipped (#12025, #11504 open at archive time). Default "Inline Terminal" bypasses shell rc files and inherits the VS Code extension-host env — PATH must be injected at the app-process level; rc files cover only the VS-Code-terminal mode. Build future effort against the Kilo Code fork instead.

```sh
# macOS process-level injection, then restart VS Code:
launchctl setenv PATH "$HOME/.tokenzero/shims:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
# or: PATH="$HOME/.tokenzero/shims:$PATH" code .
```

MCP still works post-EOL: `.roo/mcp.json` / global `mcp_settings.json`.

Sources: https://docs.roocode.com/features/shell-integration, https://docs.roocode.com/advanced-usage/available-tools/execute-command, https://github.com/RooCodeInc/Roo-Code/issues/12025, https://nerova.ai/news/roo-code-shutting-down-may-15-2026-what-users-should-do-next, https://kilo.ai/articles/roo-to-kilo-migration-guide

### Factory droid — native-hook (High)

`hooks.json` at `~/.factory/hooks.json` (user/project/enterprise scopes; legacy `hooks` key in settings.json). `PreToolUse` matcher on tool name — shell tool is `Execute`; MCP tools `mcp__<server>__<tool>`. Stdout `hookSpecificOutput.permissionDecision` + `updatedInput` = first-class rewrite. Default timeout 60 s; hooks snapshot at startup (re-approve via `/hooks`). TokenZero MCP is already wired in `~/.factory/mcp.json`.

```json
// ~/.factory/hooks.json
{ "hooks": { "PreToolUse": [ { "matcher": "Execute", "hooks": [
  { "type": "command", "command": "~/.tokenzero/bin/tz-droid-rewrite.sh", "timeout": 10 }
] } ] } }
```

Sources: https://docs.factory.ai/reference/hooks-reference, https://docs.factory.ai/cli/configuration/hooks-guide, https://docs.factory.ai/cli/configuration/settings, https://docs.factory.ai/cli/configuration/mcp

### OpenCode — native plugin hook (High, with caveats)

JS/TS plugins in `.opencode/plugins/` or `~/.config/opencode/plugins/`. `tool.execute.before` can mutate `output.args` directly (official docs show rewriting `output.args.command` for the bash tool); `shell.env` injects env into ALL shell execution; `tool.execute.after` could compress results. **Caveats:** (1) issue #5894 — `tool.execute.before` does not intercept subagent (task-tool) calls, so coverage is primary-agent-only until fixed; (2) rtk#1706 reported the hook not firing on some versions — smoke-test the installed version before trusting it.

```ts
// ~/.config/opencode/plugins/tokenzero.ts
export const TokenZeroPlugin = async ({ project, client, $, directory }) => ({
  "tool.execute.before": async (input, output) => {
    if (input.tool === "bash")
      output.args.command = `~/.tokenzero/bin/tokenzero run -- ${output.args.command}`
  },
  "shell.env": async (input, output) => {
    output.env.TOKENZERO_SHIM = "1"
    output.env.PATH = `${process.env.HOME}/.tokenzero/shims:${output.env.PATH ?? process.env.PATH}`
  }
})
```

Sources: https://opencode.ai/docs/plugins/, https://github.com/sst/opencode/issues/5894, https://github.com/rtk-ai/rtk/issues/1706

### Crush — native-hook (High)

`hooks` block in `crush.json` / `~/.config/crush/crush.json`; only event is `PreToolUse` (name case-insensitive). Matcher regex vs tool name (shell tool is `bash`). Hook gets stdin JSON plus `CRUSH_TOOL_INPUT_COMMAND` env; exit-0 stdout `{decision, reason, context, updated_input}` — `updated_input` is a replacement tool input, i.e. native rewriting. Crush also accepts Claude Code hook output format; deny wins across multiple hooks, last non-empty `updated_input` wins; hooks run before permission checks.

```json
// crush.json
{ "hooks": { "PreToolUse": [
    { "matcher": "^bash$", "command": "$HOME/.tokenzero/bin/tz-crush-hook.sh", "timeout": 10 } ] },
  "mcp": { "tokenzero": { "type": "stdio", "command": "~/.tokenzero/bin/tokenzero", "args": ["mcp"] } } }
// hook stdout: {"decision":"allow","updated_input":{"command":"tokenzero run -- <orig>"}}
```

Sources: https://github.com/charmbracelet/crush, https://github.com/charmbracelet/crush/tree/main/docs/hooks

### Aider — env+path-shim + conventions (Medium)

No hooks, no plugin system, no native MCP (issues #3314, #4506 still open as of 2026-06; community MCP bridges are experimental bolt-ons). `/run`, `/test`, and confirmed LLM-suggested commands run as subprocesses inheriting the aider process env — env+PATH injection is the only lever, paired with an instruction layer (`--read` conventions file; the RTK pattern for hookless harnesses). Coverage caveat: shims only catch argv[0] matches; absolute-path invocations bypass. No way to compress aider's own repo-map/file reads.

```sh
#!/bin/sh
# ~/.tokenzero/bin/aider-tz
export TOKENZERO_SHIM=1
export PATH="$HOME/.tokenzero/shims:$PATH"
exec aider --read "$HOME/.tokenzero/conventions/tokenzero-aider.md" "$@"
```

Sources: https://aider.chat/docs/usage/commands.html, https://aider.chat/docs/config/options.html, https://github.com/Aider-AI/aider/issues/3314, https://github.com/Aider-AI/aider/issues/4506

### Zed — mcp-only (Medium)

No hooks (feature request #52688 open); profiles only gate which tools run. The `terminal` tool spawns a fresh non-interactive, non-login shell per invocation (no direnv; #35231). Env precedence: CLI-inherited env (when `zed` launched from a shell) > settings env > project login-shell env — PATH shims work but are fragile from Dock launches. Robust path: tokenzero MCP server via `context_servers` + a `.rules` file mandating tz tools over the terminal tool. For external agents, ACP `agent_servers` entries take a custom `command` + `env` — a JSON-RPC proxy wrapper there is Zed's only true interception seam (note #37469: ACP agents don't inherit Zed's project env).

```json
// Zed settings.json
{ "context_servers": {
    "tokenzero": { "source": "custom", "command": "~/.tokenzero/bin/tokenzero", "args": ["mcp"], "env": {} } },
  "agent_servers": {
    "claude-tz": { "type": "custom", "command": "~/.tokenzero/bin/acp-proxy", "args": ["--", "claude-code-acp"],
      "env": { "PATH": "~/.tokenzero/shims:/usr/local/bin:/usr/bin:/bin" } } } }
```

Sources: https://zed.dev/docs/ai/tools, https://zed.dev/docs/ai/mcp, https://zed.dev/docs/ai/external-agents, https://zed.dev/docs/environment, https://github.com/zed-industries/zed/issues/52688, https://github.com/zed-industries/zed/discussions/35231, https://github.com/zed-industries/zed/issues/37469

## 3. Claude Code hook design

### Contract (verified against official docs)

Stdin to the hook for a Bash call:

```json
{
  "session_id": "abc123",
  "transcript_path": "/path/to/transcript.jsonl",
  "cwd": "/current/directory",
  "permission_mode": "default",
  "hook_event_name": "PreToolUse",
  "tool_name": "Bash",
  "tool_input": { "command": "npm test" }
}
```

Stdout options on exit 0 (field names exact and case-sensitive: `hookEventName`, `permissionDecision`, `permissionDecisionReason`, `updatedInput`):

- **Allow unchanged:** `{}` or no JSON.
- **Deny:** `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"..."}}`
- **Rewrite:** `permissionDecision:"allow"` + `updatedInput` — replaces the entire `tool_input`:

```json
{ "hookSpecificOutput": { "hookEventName": "PreToolUse",
    "permissionDecision": "allow",
    "updatedInput": { "command": "tokenzero run -- npm test" } } }
```

Exit codes: 0 = parse JSON; 2 = blocking error (tool call blocked); other non-zero = non-blocking (first stderr line shown, call continues). Timeout (default 600 s, configurable per hook) blocks the call.

Matchers: `"Bash"` exact; `"Bash|Write|Edit"` lists; regex when other chars present; optional `"if"` condition (e.g. `"Bash(rm *)"`). Config precedence: `.claude/settings.local.json` > `.claude/settings.json` > `~/.claude/settings.json` > plugin `hooks/hooks.json` > skill/agent frontmatter.

Subagents: PreToolUse fires for subagent tool calls, but hooks are **not inherited** from the parent agent's runtime config — they must live in shared settings (`.claude/settings.json` or `~/.claude/settings.json`) or be passed via SDK `settingSources`. Background agents have independent configuration. SDK hook inputs add `agent_id`/`agent_type`. Install the TokenZero hook at the user scope so subagents are covered.

### Modes

- **rewrite (default):** emit `permissionDecision:"allow"` + `updatedInput.command = "tokenzero run -- <orig>"` (compound commands go through the compound-safe rewriter). Optionally attach `additionalContext` describing the change.
- **deny (policy mode, opt-in):** `permissionDecision:"deny"` with a reason steering to `tz_shell`/`tz_read`/`tz_grep`. Reserved for enforcement experiments; rewrite is the production mode because it never costs the agent a turn.

### Skip list (hook exits 0 with no JSON — passthrough unchanged)

- Interactive/TTY commands (editors, pagers, REPLs, ssh, top, …)
- Background jobs (trailing `&`)
- Heredocs (`<<`)
- Already-wrapped commands (begin with `tokenzero` / contain the wrapper)
- `TOKENZERO_NO_WRAP=1` set in the environment or inline in the command

### Fail-open semantics

The Claude Code harness is fail-closed on exit 2 and on timeout; the TokenZero hook therefore must never reach either state:

- Any internal error (parse failure, missing binary, rewrite panic) → exit 0 with no JSON: original command runs unchanged.
- Never exit 2; never emit `deny` in rewrite mode.
- Enforce an internal deadline well under the configured `timeout` (set `"timeout": 10`); on overrun, exit 0 unchanged.
- Malformed JSON on exit 0 is treated by the harness as continue — acceptable, but the hook should emit valid JSON or nothing.

## 4. Universal PATH shim design — implemented

Status: implemented as the `shim` install capability (`tokenzero install --shims`; opt-in, not part of the `clients` standard profile). A directory `~/.tokenzero/shims/` of tiny wrappers for the read-heavy commands agents abuse: `cat head tail grep rg find ls tree wc`. Each generated shim:

```sh
#!/bin/sh
# tokenzero shim for grep — generated; real binary resolved at install time.
REAL='/usr/bin/grep'
if [ "$TOKENZERO_SHIM" = "1" ] && [ -z "$TOKENZERO_INNER" ]; then
  TOKENZERO_INNER=1 exec '~/.tokenzero/bin/tokenzero-runtime-<hash>' run -- "$REAL" "$@"
fi
exec "$REAL" "$@"
```

Rules:

- **Inert by default.** Active only when `TOKENZERO_SHIM=1` and `TOKENZERO_INNER` is unset; otherwise `exec` the real binary. Putting the shim dir on PATH changes nothing until a harness opts in.
- **No recursion.** The shim itself prefixes the wrapped exec with `TOKENZERO_INNER=1` (env assignments on `exec` inherit to the child's children), so PATH lookups made by the child of `tokenzero run` fall straight through every shim to the real binary — no runtime-crate env plumbing needed.
- **Real-binary resolution at install time.** The planner resolves each tool's real path with the shim dir excluded from PATH — plus any other `.tokenzero/shims` dir or previously generated shim file — and bakes it in. Tools missing from PATH at install time are skipped (the plan is per-machine; no `rg` shim on a machine without ripgrep).
- **Known coverage gap:** only argv[0] matches are caught; absolute-path invocations (`/bin/cat`) bypass. Shims are the fallback layer, not the primary one.
- **Installer:** ships as the `shim` capability in the `tokenzero-install` planner, mirroring the existing `cli`/`cli-shim` pattern (planned writes under `~/.tokenzero/shims/<name>`, `content_for` arm, `make_executable_if_needed`, byte-level rollback via the existing manifest, dedicated `client_surface_checks` for detect/doctor). Conformance: `tokenzero-install` unit tests cover per-machine skip, recursion-guard short-circuit, rollback-to-absence, and exit-code parity of the inert shim vs real `grep`.

### Per-harness activation

| Harness | How `TOKENZERO_SHIM=1` + shim PATH get in |
|---|---|
| Claude Code | Not needed — hook rewrites Bash directly. |
| Gemini CLI | Project `.gemini/.env`: `TOKENZERO_SHIM=1` + **literal** full PATH string (dotenv does not expand `$PATH`). |
| Codex CLI | `[shell_environment_policy] set = { TOKENZERO_SHIM = "1", PATH = "<shims>:..." }` with `ignore_default_excludes = true` (the `*TOKEN*` exclude would drop `TOKENZERO_*`). Primary use: covers the `unified_exec` hook gap. |
| Cursor | rc-file guard: `if [ -n "$CURSOR_AGENT" ]; then export TOKENZERO_SHIM=1; PATH=...; fi` (agent shells set `CURSOR_AGENT=1`). Shims must be readable inside the sandbox. `cursor-agent` CLI inherits the invoking shell. |
| Windsurf | `~/.zshenv` exports (Cascade's dedicated macOS terminal always runs zsh and sources zsh config; VS Code terminal env settings do not apply). Primary mechanism here — hooks can't rewrite. |
| Cline | Shell rc files (real shell via VS Code shell integration). Do NOT use terminal-profile env (ignored, bug #7793). CLI inherits the invoking shell. |
| Roo Code | Process-level only for the default Inline Terminal: `launchctl setenv PATH ...` (macOS) or launch VS Code from a shimmed shell; rc files cover only VS-Code-terminal mode. |
| Factory droid | Inherits launch shell env (wrapper launcher possible) — unnecessary; hook rewrites. |
| OpenCode | `shell.env` plugin hook sets `output.env.TOKENZERO_SHIM` and prepends the shim dir to `output.env.PATH`. |
| Crush | Inherits launch env — unnecessary; hook rewrites. |
| Aider | `aider-tz` wrapper launcher exports both, then `exec aider --read <conventions>`. Primary mechanism. |
| Zed | Launch `zed` from a shimmed shell (CLI-inherited env has top precedence) or per-ACP-agent `env` in `agent_servers`. Fragile from Dock launches — Medium confidence. |

## 5. Redundancy layer: seen-set dedup + diff-aware re-reads

Hooks and shims route calls into TokenZero; this layer stops TokenZero from re-paying for content the session already saw. Two features, implemented entirely in `crates/tokenzero-mcp` (no edits to `tokenzero-core` or `tokenzero-recovery`).

Why TokenZero and not an indexer: codedb's persistent index (seq, trigram, outlines, contents cache) tracks **repo** state, not what the agent was shown. TokenZero's `RecoveryStore` already content-addresses every served payload behind `tz://` refs — so "unchanged since you saw it" and "diff vs what you saw" are session-truthful claims only TokenZero can make. The design keeps the daemonless, lazy at-call-check posture (the codedb external daemon disconnected mid-analysis with a cold 0-file index — the cautionary tale for daemon-dependent state).

### (a) Seen-set dedup

- **State:** `Mutex<SessionMemory>` field on `TokenZeroEngine` (one engine per stdio server process; server lifetime == client session). Keys: `ServeKey::File { path, start, end }` (canonicalized) and `ServeKey::Output { tool, query, roots }` for find/grep flat output. Records hold `sha256` of the exact served text, the minted `blob_ref`/`file_ref`, `raw_tokens`, line/byte counts, serve counters; mtime is telemetry only — **content hash is the invalidation source**, recomputed from bytes already read.
- **Hit path:** identical hash → render a one-line note instead of the full payload: `unchanged: <file_ref> (served earlier this session)` plus a summary line and `full bytes: expand <blob_ref>`. ROI guard: emit only if the note is strictly cheaper than the full render (the "capsules never cost more than raw" invariant). Refs are always freshly minted (storage unchanged; content-addressing makes re-store cheap).
- **In-memory only, by design:** a sidecar surviving restart would assert "served earlier this session" to a fresh session whose context never contained the bytes — a lie the model cannot detect — and two agents on one repo would cross-contaminate a shared file. Supervisor respawn (`--supervise`) loses the map and degrades to today's full-serve behavior: never wrong, only un-optimized. Mutex poisoning fails open to a full serve.
- **Bypass matrix:** `raw=true` (edit-pass contract requires exact bytes), passthrough mode, per-call `fresh: true`, env `TOKENZERO_MCP_DEDUP=0|off` (default on). `expand` is never deduped — it is the recovery path and must always return bytes.

### (b) Diff-aware re-reads

When the key hits but the hash changed:

1. Recover the previously served bytes via the existing public API `RecoveryStore::expand(blob_ref, "raw", ...)` — no recovery-crate changes. Base missing (cache pruned) → fall back to full serve.
2. Size guard: skip diffing above 2 MiB or 50k lines per side.
3. Unified diff (3 context lines; `similar` crate, or a bounded ~100-line Myers fallback if the dependency diet wins), headed `# read <path> — changed since served this session (diff vs <old_blob_ref>)`, tailed `full file: expand <new_blob_ref>`.
4. Token gate: serve the diff only when **strictly cheaper** by `count_tokens` than the full capsule (same philosophy as grouped search output / `pick_cheaper`). Tie or larger → full.
5. Update the record to the new hash/refs; the next identical read gets the (a) note. Range-keyed reads diff slice-vs-slice only under the same `(start, end)` key.
6. Env `TOKENZERO_MCP_DIFF_READS=0|off` disables (default on).

### Accounting and failure mode

Savings flow through the **existing** fields — no Pulse schema change: per-call `Accounting { raw_tokens, visible_tokens, recovery_tokens }` plus telemetry `output_strategy` (`seen_set_dedup` / `diff_since_served`) and `cache_hit`. The internal base expansion for diffs is charged as recovery tokens, keeping recovery-adjusted savings honest. `mem()` reports a session rollup: `{ records, dedup_hits, diff_hits, visible_tokens_saved, diff_tokens_saved }`.

Client-compaction failure mode: if the client compacted the earlier payload out of its context, the note's embedded ref recovers exact bytes in one `tz_expand` (stale/dangling refs already return typed `ref_stale`/`ref_not_found` errors with a rerun hint). Worst case ≈ raw + small overhead, honestly charged — Pulse shows when dedup back-fires for a compaction-happy client, and the lever is `TOKENZERO_MCP_DEDUP=off`.

## 6. Rollout order and measurement

### Rollout order

Ordered by (confidence × daily leverage), rewrite-capable hooks first:

1. **Redundancy layer** (§5) — benefits every harness through the MCP surface; ships first.
2. **Claude Code hook** — primary daily harness; MCP aliases already in use.
3. **Factory droid** — MCP already wired in `~/.factory/mcp.json`; identical `updatedInput` contract.
4. **Codex CLI** — hook + `shell_environment_policy` shim (covers `unified_exec` gap in one config).
5. **Gemini CLI** — `BeforeTool` rewrite.
6. **Crush**, then **OpenCode** — OpenCode only after smoke-testing `tool.execute.before` on the installed version (#1706) and accepting the subagent gap (#5894).
7. **Cursor** — `preToolUse` rewrite; verify shims/daemon socket are sandbox-visible first.
8. **PATH shim layer** + **Windsurf** (`~/.zshenv` + block backstop) and **Cline** (deny+steer hook + rc shim).
9. **Aider** wrapper launcher + conventions file; **Zed** mcp-only + `.rules`.
10. **Roo Code: skip** (EOL; archived). Track the Kilo Code fork instead.

Each adapter ships through the `tokenzero-install` planner as a capability: a `hooks` arm in `plan_for_agents` + a `merge_json_hooks` content arm (mirroring `merge_json_mcp`'s upsert-one-key strategy) + a dedicated `client_surface_checks` arm, and a `shim` capability mirroring `cli`/`cli-shim`. **New writes must be agent-scoped** — the wired-to-docs contract test (`cli_contract.rs:697`) asserts a grok-scoped plan contains no `.claude.json`; an unscoped hooks write breaks it. Detect/doctor visibility is free once planned (surfaces derive from the planner); also update the agent allowlist (`push_agent`), capability flag mappers, `clients_capabilities`, and `agent_surfaces.rs` `COMMANDS`/`canonical_invocations`.

### Measurement plan (pulse before/after)

Per adapter, A/B on a fixed task script run against this repo:

1. **Baseline (adapter off):** capture the Pulse session report — `raw_tokens`, `visible_tokens`, `recovery_tokens`, `output_strategy` histogram, `cache_hit` rate, call count.
2. **Enable adapter; rerun the same tasks.** Compare **recovery-adjusted savings** (visible-context savings minus charged expansions) and verify zero broken calls.
3. **Gate:** adapter stays default-on only if recovery-adjusted savings > 0 and the passthrough contract held (no call failed that would have succeeded raw). For the redundancy layer specifically, watch the back-fire signal: expansion charges exceeding note savings indicate a compaction-happy client → recommend `TOKENZERO_MCP_DEDUP=off`.
4. All signals are existing Pulse fields; no schema change.

Verification for every code change in this plan, per project rules: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`.
