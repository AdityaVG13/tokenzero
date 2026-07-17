# TokenZero Northstar

Snapshot: `20260717T014139.658900Z-ab0b3ca3090a`
Commit: `ab0b3ca3090a9f063ff4a445dda896d9dd394524`
Mode: `run-components`

## Headline vs raw

| Raw tokens | TokenZero visible | Savings |
| ---: | ---: | ---: |
| 222,392 | 1,244 | **99.0%** |

## Per-operation compression

| Workload | Raw tokens | TokenZero visible | Savings |
| --- | ---: | ---: | ---: |
| read large source file (crates/tokenzero-mcp/src/lib.rs) | 1,744 | 45 | 97.0% |
| re-read same file (seen-set dedup) | 1,744 | 45 | 97.0% |
| repo-wide grep ('fn ' across crates/) | 90,541 | 487 | 99.0% |
| cargo test run (tokenzero-filters suite) | 292 | 80 | 72.0% |
| directory listing (find vs tree, depth 3) | 37,530 | 541 | 98.0% |
| re-find stored content (recall vs re-grep) | 90,541 | 46 | 99.0% |

## Boot cost

| Corpus | Files | Boot tokens |
| --- | ---: | ---: |
| repository | 11,917 | 21 |
| synthetic-100k | 100,000 | 21 |

## Expand latency

| Size | Samples | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: |
| 1KB | 50 | 3.628 ms | 10.914 ms | 24.820 ms |
| 100KB | 50 | 4.135 ms | 6.777 ms | 10.106 ms |
| 1MB | 30 | 10.575 ms | 12.792 ms | 14.646 ms |
| 10MB | 30 | 64.824 ms | 68.243 ms | 69.498 ms |
| 100MB | 3 | 577.986 ms | 604.158 ms | 606.484 ms |

## Trend

Trend is not comparable to the previous stored snapshot:
- environment.python differs: '3.14.4' != '3.14.6'
