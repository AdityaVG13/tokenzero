<!-- tokenzero:rust-core:start -->
Use `tokenzero read/find/tree/run/expand` or MCP aliases. Rust Core runs as a standalone binary for normal use.
<!-- tokenzero:rust-core:end -->

## Public Beads export

Before staging the tracked Beads export, sync, scrub, check, then stage only the explicit files:

~~~sh
br sync --flush-only
python3 scripts/scrub_beads_export.py
python3 scripts/check_no_host_paths.py
git add .beads/issues.jsonl scripts/scrub_beads_export.py scripts/check_no_host_paths.py .github/workflows/ci.yml AGENTS.md
~~~

Never stage .beads/issues.jsonl directly after br sync --flush-only; the scrub must run first.
