<div align="center">

<img src="../.github/assets/banner.gif" alt="TokenZero: Recovery-Aware Context Compression" width="100%">

</div>

# TokenZero demo

A self-contained, byte-honest demo that walks an AI agent's "day in the life"
through TokenZero and reports how many tokens it hid versus how many it
actually fed back to the agent.

## What it shows

Seven real scenarios run against this repository's own source tree:

| # | Scenario | What you should see |
| -: | :-- | :-- |
| 1 | small read (`crates/tokenzero/Cargo.toml`)        | pass-through; capsule never costs more than raw |
| 2 | large read (`crates/tokenzero-mcp/src/lib.rs`)    | heavy savings + a `tz://blob/...` ref |
| 3 | re-read same large file                            | seen-set dedup: visible tokens drop sharply against the same cache |
| 4 | grep `fn ` across `crates\`                       | recoverable hit set; raw is the full `rg` dump |
| 5 | `expand <ref>` of the large-read blob              | **byte-exact** round-trip check — the script fails if a single byte differs |
| 6 | `recall 'fn main'`                                | re-find content already in cache, no filesystem rescan |
| 7 | `run -- git --version` (or `cmd /c ver`)          | shell stream captured behind a ref |

The driver counts raw tokens by piping the raw output through
`tokenzero ingest --stdin` (TokenZero's own tokenizer) and reads
`accounting.visible_tokens` from each call's JSON. Same tokenizer on both
sides → the savings number is fair.

## Visualization

The driver writes `demo_results.json` and then renders a fully self-contained
`demo_viz.html` (inline CSS + inline SVG, no CDN, no JS). Pass `-OpenViz` to
have it pop in your default browser at the end:

```powershell
pwsh -File ./demo/run_demo.ps1 -OpenViz
```

The page has two sections:

**Performance** — donut for the recovery-aware total savings + raw / visible / savings
stats, plus per-scenario panels with side-by-side raw vs visible bars
(log-scaled so the 11-token shell row and the 79,000-token grep row are both
legible). A `byte-exact recovery: PASS` badge is derived from scenario 5,
and a warning callout fires if the dedup row's savings does not improve on
the first-read row (the empirical gap I observed against v1.0.1).

**Bugs flagged for the developer** — ranked CRITICAL → HIGH → MEDIUM → LOW,
from `demo/gap_report.json`. Each finding is an expandable card with impact,
evidence (file:line citations), repro (where applicable), fix sketch, and
the review pass that surfaced it. The header includes a `N bugs flagged
(critical/high/medium/low)` button that jumps to the section.

Re-render without re-running the demo:

```powershell
pwsh -File ./demo/build_viz.ps1 -Open
```

Re-render against a custom gap report:

```powershell
pwsh -File ./demo/build_viz.ps1 -GapReportPath ./my_gaps.json -Open
```

Skip the viz entirely (just write `demo_results.json`):

```powershell
pwsh -File ./demo/run_demo.ps1 -NoViz
```

## Run it

```powershell
# from the repo root
pwsh -File ./demo/run_demo.ps1
```

The script will:

1. Use `tokenzero` from `PATH` if present;
2. else reuse `demo/.tokenzero-bin/tokenzero` (or `tokenzero.exe` on Windows)
   if it's already there;
3. else download the `v1.0.1` GitHub Release asset for the current OS/CPU
   (`x86_64-pc-windows-msvc.zip`, `x86_64-unknown-linux-gnu.tar.gz`,
   `aarch64-apple-darwin.tar.gz`, or `x86_64-apple-darwin.tar.gz`), verify
   the published SHA256, and extract it into `demo/.tokenzero-bin/`.

All runtime state lives under `demo/.cache/` (deleted at the top of every
run) so the demo never touches your real TokenZero cache or telemetry.

### Options

```text
-BinaryPath <path>   Use a specific tokenzero binary (skip PATH/download)
-ReleaseTag <vX.Y.Z> Release to download if no binary is found (default: v1.0.1)
-SkipDownload        Fail instead of downloading when no binary is found
```

## What the output looks like

You'll get a Markdown-friendly table on stdout, plus a machine-readable
`demo\demo_results.json` you can diff between runs or post-process:

```json
{
  "tokenzero_version": "tokenzero 1.0.1",
  "workloads": [
    { "workload": "large read (...)", "raw_tokens": 16929, "visible_tokens": 150, "savings_pct": 99.1 },
    ...
  ],
  "totals": { "raw_tokens": ..., "visible_tokens": ..., "savings_pct": ... }
}
```

## Reading the numbers honestly

TokenZero's claim is **Recovery-Aware Context Compression**: tokens hidden
behind a `tz://` ref that the agent later has to `expand` *do not count* as
savings. That's exactly why scenario 5 round-trips the large-read ref and
fails the demo if recovery isn't byte-exact — a "saving" you can't actually
recover wouldn't be a saving.

## Extending the demo

Each scenario is one block in `run_demo.ps1` that ends in `Add-Row`. Copy
one, point it at another path / command / query, and it will show up in
the summary and the JSON automatically.
