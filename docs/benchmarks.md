# TokenZero Benchmarks

Reproducible with `scripts/benchmark_tokens.sh` (run from the repo root; any
machine with the repo, `jq`, and ripgrep). Both sides of every row are
counted with TokenZero's own estimator (`count_tokens`: word/punctuation
boundaries — not a model tokenizer like tiktoken, whose absolute counts
differ by single-digit to low-double-digit percentages on code). Raw and
visible columns use the same scale, so the savings ratios hold; treat the
absolute token numbers as estimates, and don't compare them against numbers
another tool counted with a different tokenizer. Every TokenZero serve keeps
exact `tz://` refs, so the raw bytes stay one `tz_expand` away. This is the
difference between these numbers and headline claims elsewhere: TokenZero's
savings are **lossless** — nothing in the saved column is unrecoverable.

## Raw tool output vs TokenZero-visible output

Measured 2026-06-10 on this repository (Apple Silicon macOS, release build,
cold benchmark cache):

| Workload | Raw tokens | TokenZero visible | Savings |
| --- | ---: | ---: | ---: |
| Read a 5k-line source file | 52,016 | 177 | 99% |
| Re-read the same file (seen-set dedup) | 52,016 | 45 | 99% |
| Repo-wide grep (`fn ` across crates/) | 44,862 | 493 | 98% |
| `cargo test` run (filters suite) | 246 | 147 | 40% |
| Directory listing (find vs `tree --depth 3`) | 802 | 224 | 72% |
| Re-find stored content (`recall` vs re-grep) | 44,862 | 209 | 99% |
| **Total** | **194,804** | **1,295** | **99.3%** |

How to read this honestly:

- The visible capsule is a compact, anchor-preserving view — failure anchors
  (exit codes, assertion lines, error text) survive by contract
  (`protected-anchor-audit` holds recall 1.0 in CI), and the full bytes are
  recoverable from the refs in the visible text. A row's savings are real
  context savings, not information deletion.
- Small outputs save less by design: the adaptive floor renders raw text
  whenever framing would cost more than the payload, so savings are never
  negative. The `cargo test` row (40%) is the floor doing its job.
- The dedup and recall rows are the redundancy layer: the second serve of
  content an agent already saw costs a two-line note (~45 tokens) instead of
  the payload, and re-finding stored content never re-runs the original
  command.
- Tool results dominate agent context, and every token here is also paid on
  every subsequent turn of a session (transcript prefill), so per-session
  savings compound well beyond the per-call numbers.

## Against published numbers from comparable tools

These are the other projects' own published claims, quoted as-is — their
methodologies differ from ours (notably: neither guarantees recovery of what
was removed).

| Tool | Mechanism | Published claim | Fidelity contract |
| --- | --- | --- | --- |
| TokenZero (this repo) | compress at source, capsule + exact refs | 99.3% on the mixed suite above (measured, reproducible) | Lossless: exact bytes one `tz_expand` away; anchor recall 1.0 audited in CI |
| context-mode | sandbox execution + BM25 retrieval against a stated intent | "315 KB becomes 5.4 KB. 98% reduction." | Lossy: a retrieval miss is invisible to the model; no recovery audit |
| opencode-dynamic-context-pruning | rewrite conversation history with summaries | (history-dependent; no single headline figure) | Lossy summaries; self-reported prompt-cache hit drop 90%→85% from mutating history |

Two structural notes the table can't show:

1. **Prompt caching.** TokenZero compresses *before* output enters the
   transcript; rendering is deterministic and refs are content hashes, so
   transcript prefixes stay byte-stable and provider prompt caches keep
   hitting. History-pruning approaches mutate prefixes and pay for it (the
   cache-hit drop above is DCP's own measurement of itself).
2. **Accounting honesty.** TokenZero's pulse ledger is recovery-adjusted:
   when an agent expands a ref, that cost is charged against the savings.
   Headline percentages that never subtract recovery or correctness cost are
   not comparable to recovery-adjusted ones.
