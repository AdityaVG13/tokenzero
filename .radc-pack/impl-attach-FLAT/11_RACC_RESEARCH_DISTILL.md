# RACC research distill (for math / Pareto work)

**Purpose:** Research ideas only — not “how to build the training stack.”  
**Sources:** TokenZero public `docs/racc.md`; spark repo `/Volumes/sparkdata/repos/RACC` (FRONTIER, RACC-SPEC, concepts, related-work, evidence-ladder, paper draft).  
**Date:** 2026-07-27  

Use this when prompting GPT-5.6 Sol Pro so formal work stays aligned with the real RACC object, not a generic “summarize harder” model.

---

## 1. Public TokenZero RACC (shipped contract)

File: `Pareto/docs/racc-public.md` (copy of `docs/racc.md`).

**Objective (not min visible length):** minimize **total task cost** while exact recovery remains available.

| Term | Research meaning |
|------|------------------|
| Visible capsule | What the model sees now |
| Exact cached payload | Byte-identical store outside context |
| Recovery handles | `tz://` / local refs with selectors |
| Recovery-adjusted savings | Visible + later expand tokens |
| RATC | Visible + recovery + retry/failure penalties |
| Task-lossless savings | After success + exact recovery gates |
| Anchor recall | Protected facts must survive |

**Omission contract:** omit only if protected anchor **or** exact ref with concrete selector **or** declared lossy (`recovery_may_be_needed` + policy id).  
**Zero loss by recovery:** while entry exists, expand is exact; eviction → typed error, never wrong bytes.  
**Promotion:** never by visible savings alone.

**Math hooks already implied:**

- Two-part code: gist + expandable pointer  
- Cost functional with **recovery side-channel**  
- Hard constraints vs soft metrics (recall, exactness)

---

## 2. Standalone RACC substrate research frame (spark)

From `RACC-SPEC` / `paper.md` / `concepts`:

\[
\mathrm{RACC}(x) = (c, r)
\]

- \(x\) = tool observation / artifact  
- \(c\) = model-visible capsule  
- \(r\) = content-addressed recoverable state (`racc://sha256/…#selector`)

**Hard gates (never soft rewards):** task correctness, exact recovery, anchor recall.  
**Rate:** recovery-adjusted **token-turn** cost (multi-turn occupancy).  
**Distortion:** decision quality vs full-context teacher (decision-preserving).

**Hard invariants H1–H5 (research relevance):**

| H | Content | Math / systems implication |
|---|---------|----------------------------|
| H1 | Exact recovery while present | Sound expand operator |
| H2 | CAS identity / rehash | Integrity of certificates |
| H3 | Never wrong bytes on miss | Typed failure ≠ silent corruption |
| H4 | Anchor floors hard | Chance constraint recall = 1 |
| H5 | Secret mask before store | Privacy as pre-compression transform |

**Secret masking Option A:** mask before CAS put (hash is of post-mask bytes). Affects any “byte identity of source” theorem — state which identity.

---

## 3. Frontier research directions (spark FRONTIER.md) — inventable math

These are **hypotheses / design spaces**, not proofs. Sol Pro may formalize or refute.

### F1 — Content-addressed KV splice

- Today: EXPAND reinjects tokens → full prefill → \(M_{\mathrm{rec}}\) expensive.  
- Idea: key KV blocks by `(model_id, position scheme, content hash, selector)`; splice on expand.  
- **Math:** when is KV reuse valid under RoPE / position? Lower bound on prefill savings vs attention correctness.  
- Accounting: still count attended tokens; add **wall-clock / energy** ledger column separate from token \(M\).

### F2 — Speculative recovery

- Prefetch expand candidates; wrong guesses cost RAM only.  
- **Math:** ski-rental / newsvendor / bandit for prefetch under miss costs; never change correctness set.

### F3 — Memory hierarchy as online paging

- Visible = registers; handle table = L1; CAS = RAM; expand = page fault; forget = eviction; consolidator = swap daemon.  
- **Math:** exactly Sol Pro T-P3 territory — competitive paging with heterogeneous miss costs and partial rehydration.

### F4 — Capsule as multi-agent interchange

- Signed capsule as unit of communication; B expands only what it needs.  
- **Math:** composition of decision-preserving maps; provenance = hash chains.

### F5 — RACC-bench as capability

- Decisions per token-turn, not long-context recall@length.  
- Aligns with Sol Pro preregistered evaluation protocol.

### F6–F7 — Flywheel + effective vs physical context

- Thesis: **read as little as possible, recover anything.**  
- Effective context ≫ physical window under exact refs.  
- **Math:** rate–distortion with a **retrieval side channel** (Wyner–Ziv / Kaspi demand-aware retrieval — already named in Kimi package).

---

## 4. Related-work positioning (spark related-work.md)

RACC differentiators claimed for research:

| Capability | vs LLMLingua family | vs MemGPT / SUPO / ReSum | vs VISTA / Memex |
|------------|---------------------|---------------------------|------------------|
| Source-level **tool** compression | LLMLingua is prompt pruning | often summary / memory | partial archive |
| **Byte-exact** refs | no | rare | VISTA/Memex partial yes |
| Hard **anchor floors** | no | usually soft | unknown |
| **Recovery-adjusted** accounting | usually no | often no | unknown |
| Decision-divergence training signal | N/A | partial | N/A if training-free |

**Cite carefully:** absence of a feature in a skim is **unknown**, not “competitor fails.”

**DeMem (arXiv:2605.10870):** decision-centric rate–distortion for *memory states* — closest academic RD framing; not tool CAS. Use as prior art for decision-distortion, not as solving RADC.

---

## 5. Evidence ladder (claim discipline)

Spark formalizes that **mechanism proof ≠ efficacy ≠ release**:

- unit / fake-backend / live-mechanism / autonomous-selection / efficacy / performance / release  
- PASS at lower level must not smuggle higher claims  
- Synthetic fixtures ≠ measurements  

**For Sol Pro math claims:** map theorems to claim levels:

| Theorem type | Max claim level |
|--------------|-----------------|
| Packing feasibility, eviction competitive bound | unit-proof / mechanism |
| Dominance on open task polytope with margins | efficacy (needs prereg + measurement design) |
| Production “we beat LLMLingua in prod” | release-proof (out of scope for pure math run) |

---

## 6. Formal objects to invent / name (suggested for Wave 2+)

These are **open invention targets** consistent with both TokenZero RACC and spark RACC:

1. **Demand-aware RACC rate** \(R_{\mathrm{da}}(D)\) — rate with expand side-channel after demand \(S\) revealed; prove gap vs one-shot \(R(D)\).  
2. **Recovery-adjusted token-turn rate** under cache multiplier \(d\) — generalize Sol Pro 1/d crossover.  
3. **Anchor-constrained rate–distortion** — minimize \(M\) s.t. recall=1 structural invariant (not empirical needles).  
4. **Capsule morphism** — when composition of compressors preserves decision-equivalence.  
5. **KV-splice soundness** — conditions for content-addressed attention reuse.  
6. **Baseline class formalization** \(\mathcal{B}_{\mathrm{formal}}\) — Sol Pro’s named gap for LLMLingua-class / masking / identity as mathematical sets.  
7. **Open task polytope \(\Theta\)** — positive-dimensional set of task weights where dominance holds with rational margins \(\gamma_M,\gamma_D,\gamma_L\).

---

## 7. What **not** to treat as research theorems

- Training campaign PASS/REJECT for small adapters  
- CPT / SFT / GRPO recipe details  
- Rental GPU logistics  
- Product packaging / crates.io  

Those are engineering; exclude from proof packages unless used only as empirical citation.

---

## 8. Files in this folder

| Path | Content |
|------|---------|
| `racc-public.md` | TokenZero shipped RACC contract |
| `spark-racc-research/FRONTIER.md` | long-horizon research vision |
| `spark-racc-research/RACC-SPEC.md` | normative hard invariants |
| `spark-racc-research/concepts.md` | operational glossary |
| `spark-racc-research/related-work.md` | competitor matrix + citations |
| `spark-racc-research/evidence-ladder.md` | claim levels |
| `spark-racc-research/paper.md` | systems paper scaffold (pre-results) |
| `spark-racc-research/bench.md` / `savings-v1.md` | eval/accounting notes |
