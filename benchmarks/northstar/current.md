# TokenZero Northstar

Snapshot: `20260715T193244Z-283fb2d2660d`  
Commit: `283fb2d2660d61c644aeb6ba7daf406cfe9f93c5`  
Mode: `run-components`

## Headline vs raw

| Raw tokens | TokenZero visible | Savings |
| ---: | ---: | ---: |
| 189,991 | 2,814 | **98.0%** |

## Per-operation compression

| Workload | Raw tokens | TokenZero visible | Savings |
| --- | ---: | ---: | ---: |
| read large source file (crates/tokenzero-mcp/src/lib.rs) | 1,598 | 1,598 | 0.0% |
| re-read same file (seen-set dedup) | 1,598 | 45 | 97.0% |
| repo-wide grep ('fn ' across crates/) | 77,105 | 509 | 99.0% |
| cargo test run (tokenzero-filters suite) | 148 | 122 | 17.0% |
| directory listing (find vs tree, depth 3) | 32,437 | 494 | 98.0% |
| re-find stored content (recall vs re-grep) | 77,105 | 46 | 99.0% |

## Boot cost

| Corpus | Files | Boot tokens |
| --- | ---: | ---: |
| repository | 11,541 | 21 |
| synthetic-100k | 100,000 | 21 |

## Expand latency

| Size | Samples | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: |
| 1KB | 50 | 29.106 ms | 30.078 ms | 30.117 ms |
| 100KB | 50 | 41.356 ms | 44.917 ms | 45.149 ms |
| 1MB | 30 | 183.300 ms | 218.223 ms | 219.207 ms |
| 10MB | 30 | 1260.090 ms | 1271.727 ms | 1272.798 ms |
| 100MB | 3 | 12328.070 ms | 12334.743 ms | 12335.336 ms |

## Trend

Initial stored northstar snapshot; no prior trend exists.
