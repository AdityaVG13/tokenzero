# Pulse spend and rank-frequency report

Corpus: 17,706 valid events from events.jsonl.1; 0 malformed and 0 duplicates skipped.

## Spend fractions and Amdahl p

The metric is visible tokens. For an optimization confined to one class, its fraction is Amdahl p.

| Class | Tokens | Fraction / p |
|---|---:|---:|
| history | 4,838,543 | 0.295703 |
| content | 9,894,119 | 0.604670 |
| protocol | 0 | 0.000000 |
| plan | 1,630,174 | 0.099627 |
| prose | 0 | 0.000000 |

## Zipf exponent fits

| Kind | Method | s | 95% CI | n |
|---|---|---:|---:|---:|
| refs | hill | 0.537717 | [0.443283, 0.652269] | 103 |
| refs | log_log | 0.324793 | [0.320825, 0.328761] | 10792 |
| operations | hill | 0.408668 | [0.131801, 1.267130] | 3 |
| operations | log_log | 1.546558 | [0.814380, 2.278736] | 11 |

## Caveats

- The operation taxonomy is a proxy: Pulse records tool outputs, not full model context composition.
- A zero class fraction means no matching operation occurred; it does not prove zero global spend.
- Fits assume independent observations and are descriptive, not causal or workload forecasts.
- Small distinct-operation and Hill-tail samples can produce wide or unstable intervals.
- This corpus has 17,706 events, below the approximate 20,000-event target; no events were synthesized.
