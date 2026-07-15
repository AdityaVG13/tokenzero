# TokenZero Northstar

Snapshot: `20260715T211950.649367Z-c35c8efaabb1`
Commit: `c35c8efaabb1e7bf01248624dd969bfb98563885`
Mode: `run-components`

## Headline vs raw

| Raw tokens | TokenZero visible | Savings |
| ---: | ---: | ---: |
| 207,092 | 941 | **99.0%** |

## Per-operation compression

| Workload | Raw tokens | TokenZero visible | Savings |
| --- | ---: | ---: | ---: |
| read large source file (crates/tokenzero-mcp/src/lib.rs) | 1,598 | 45 | 97.0% |
| re-read same file (seen-set dedup) | 1,598 | 45 | 97.0% |
| repo-wide grep ('fn ' across crates/) | 85,453 | 36 | 99.0% |
| cargo test run (tokenzero-filters suite) | 233 | 257 | -11.0% |
| directory listing (find vs tree, depth 3) | 32,757 | 512 | 98.0% |
| re-find stored content (recall vs re-grep) | 85,453 | 46 | 99.0% |

## Boot cost

| Corpus | Files | Boot tokens |
| --- | ---: | ---: |
| repository | 11,570 | 21 |
| synthetic-100k | 100,000 | 21 |

## Expand latency

| Size | Samples | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: |
| 1KB | 50 | 3.400 ms | 3.623 ms | 3.756 ms |
| 100KB | 50 | 3.958 ms | 4.296 ms | 4.586 ms |
| 1MB | 30 | 10.134 ms | 11.474 ms | 11.559 ms |
| 10MB | 30 | 58.639 ms | 62.379 ms | 66.093 ms |
| 100MB | 3 | 547.501 ms | 557.339 ms | 558.214 ms |

## Trend

Initial stored northstar snapshot; no prior trend exists.
