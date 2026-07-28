# Adjacent math + AI proof methods (for RADC invention)

**Added:** 2026-07-27  
**Expanded:** 2026-07-27 (last ~2 weeks X + FLT shortcuts audit PDF)  
**Purpose:** Capture sources that are **not** TokenZero domain math, but show **how** frontier models invent / refute / fail at math -- so Sol Pro / later waves can copy *method*, not domain content.  
**Your thesis:** RADC math will be invented from **principles others have proved and disproved** with AI (conjecture → certificate → Lean/Python → infinite family → honest scope).  
**Training-data note:** Most of the episodes below are **July 2026** (or late spring 2026 unit-distance cluster). Treat them as **post-cutoff exemplars**: short proofs, counterexamples, obstruction maps, and failed "shortcuts." Do **not** re-solve graph theory / algebraic geometry / number theory. Steal **workflow and status vocabulary**.

**Honesty rule (non-negotiable):**  
X posts claim faster than referees. Label every episode with a **proof-status** tag (see §0). "Someone on X said GPT solved X" is not a theorem. Prefer expert mathematicians naming the result (Lichtman, Bloom, Fithian, Gowers-adjacent commentary) + Lean/Python certificates + arXiv when present.

---

## 0. Proof-status vocabulary (steal this for RADC)

From the **FLT shortcuts audit** (GPT-5.6 Sol / Codex, 25 Jul 2026 -- see §B). Use the same hierarchy in every RADC claim:

| Tag | Meaning |
|-----|---------|
| **Published input** | Theorem with bibliographic source |
| **Derived result** | Deduction from stated inputs + explicit hypotheses; **not** peer-reviewed novelty |
| **Exact computation** | Finite integer / field / ideal / poly arithmetic with a checker; proves **only** the finite assertion |
| **Bounded experiment** | Search over a declared finite family or time/memory bound; failure ≠ unbounded theorem |
| **Speculative bridge** | Unproved statement that would advance a route if established |

**RADC mapping:** Q4-DA Lean identity → exact computation or derived result. "Production TokenZero wins Pareto" → speculative bridge until measured. ρ_fail phase kill outside gauge → derived result under stated hypotheses. Float λ_min vibes without Sturm/IVT → not even exact computation.

**Additional status language used on X this fortnight (Acer / Lichtman norms):**

- "GPT **claims** a proof" ≠ "we proved"
- Short counterexample disproofs ≠ long-horizon theory breakthroughs
- Expert review + formal check is the real milestone, not the model announcement

---

## 1. Catalog: last ~2 weeks (and immediately adjacent) AI-math episodes

Window: roughly **2026-07-12 → 2026-07-27**, plus the spring unit-distance cluster still discussed this week. Ordered by **method type**, not hype.

### A. DISPROOFS BY COUNTEREXAMPLE (short, high transfer)

These are the pattern that dominates successful frontier AI math: **falsify uniqueness / existence with an explicit object**, often ≤~15 pages of standard tools (Lichtman rule of thumb, 15 Jul 2026).

| Episode | Status (as of X/web 2026-07-27) | Method skeleton | RADC transfer |
|---------|----------------------------------|-----------------|---------------|
| **Jacobian conjecture** (polynomial maps C^n → C^n with Jac det constant nonzero ⇒ invertible) | **Claimed disproved** by **Claude Fable** counterexample; heavy X chatter mid–late Jul; Lean explorations (e.g. Harmonic/Aristotle "candidate map constant nonzero Jac, not injective"); follow-on arXiv traffic (e.g. Hessian-related 2607.22198). Treat as **disproof-by-counterexample class**, confirm Lean/peer before calling settled | Construct poly map; check Jac; exhibit collision of points (or non-injectivity); formalize | **Kill claimed uniqueness of "invertible policy" classes.** Extremal claim: "Jac-like regularity ⇒ global bijection." Counterexample family > one freak map. Honest: formal class ≠ "all polynomial maps" marketing |
| **Erdős unit-distance** (geometry conjecture) | **Disproved** earlier (spring 2026 cluster, still cited this week); Gowers-class commentary that expert would accept at Annals; technique reused by humans for sum-product style kills | Explicit geometric configuration / combinatorial construction; Lean optional | **Configuration that breaks a long-standing extremal claim** while leaving related inequalities intact |
| **Benjamini–Hochberg / FDR-adjacent open conjecture** (stats; Fithian: "most interesting open problem in my area") | **Disproved** by **GPT-5.6** counterexample (~15 Jul 2026). Expert owns the regret ("wish a human had") | Short clever construction in existing methods | **Domain expert validation** is part of the method. Counterexamples are the AI sweet spot when proofs are short |
| **Dinitz–Garg–Goemans** (~30y open) | **Claimed disproved** "just asking AI" (~22 Jul), days after Jacobian wave. Status: high-engagement X; treat as **counterexample claim pending expert writeup** | Same short-counterexample class | Another instance: open problem longevity ≠ proof hardness for AI when a counterexample exists |
| **Fan–Yu–Wang full-range minimizer** (graph spectral) | **Refuted** (Codex + Fable; Alcaide X; gist hypnopump). Smallest (n,χ)=(9,8); infinite family (2r+1,2r). **Does not** kill Tang–Elphick **proved bound** arXiv:2511.07712 | Bound proved; extremal uniqueness conjectured; multi-model kill; exact + Lean + Python; infinite family; honest §5-only scope | **Canonical RADC template** -- see §3 |
| **Grothendieck "power / rank" question** (finite locally free scheme of order n killed by n?) | **Counterexample** via **Mathlib PR** (Akhil Mathew; AI-generated manuscript noted by Acer ~15 Jul: rank-four counterexample tex) | Formal library as publication surface; explicit algebraic counterexample | **Ship the certificate in the formal system of record**, not only a tweet |

**Shared method principles (disproof class):**

1. Prefer **counterexample search** when the claim is universal ("for all maps / graphs / procedures…").  
2. After first hit → **infinite family** or **phase** (when does the construction work?).  
3. **Preserve** the related inequality if it is actually proved (Fan–Yu–Wang vs Tang–Elphick).  
4. **Lean / Mathlib / exact arithmetic** turns "AI said" into a checkable object.  
5. Expert human (Fithian, Lichtman, Bloom…) is part of the **verification stack**, not optional PR.

### B. PROOFS / RESOLUTIONS (short Gordian-knot style)

| Episode | Status | Method skeleton | RADC transfer |
|---------|--------|-----------------|---------------|
| **Erdős problem #119** (strong form) | **Claimed solved** by **GPT-5.6** (~19 Jul). Bloom: strongest form in ~1 page standard harmonic analysis vs Beck's 44-page Annals for weaker form. Lichtman: "Gordian knot" cut (echo of #1196) | Attack the **harder** quantifier; short classical tools; human expert commentary | **Harder formal class can be easier.** RADC: prove a stronger demand-aware statement that collapses, rather than a weak vague Pareto claim. Prefer 1-page clean formal class over 44-page narrative |
| **Erdős #421** | **Claimed** complete proof exposition + Lean (Animish Sharma et al., ~25 Jul). Treat as **claim + formalization in progress** until independent check | Exposition + Lean repo | Certificate-first publishing |
| **Erdős–Gyárfás** (open since 90s) | **Partial progress** (~25 Jul): bound 4/7 → 2/3 cubic vertices in minimal counterexample + structural restriction; Claude + Grok assist; **Lean 4**. Not a full solution | Bound sharpening inside minimal-counterexample framework; formalize as you go | **Don't only binary prove/disprove.** Intermediate structural theorems + Lean. RADC: margins, ρ_fail thresholds, handle-cost phases as partial theorems |

**Lichtman meta (15 Jul):** if a problem has a **short** proof/disproof with existing methods (clever execution), frontier AI can likely find it; "short" is lengthening (~15 pages). Explains why **counterexamples dominate headlines**.

**Lichtman meta (27 Jul):** short counterexample disproofs are the current wave; **next frontier = long-horizon theory** (deeper than Euclid→Grothendieck-style jumps). RADC production dominance may sit in that harder bucket -- so invent **formal subclasses** that *are* short-proof accessible first (Q4-DA, Qn-DA, ρ_fail phase).

### C. NEGATIVE / OBSTRUCTION-MAP RESULTS (failed shortcuts -- highest RADC honesty transfer)

#### C1. FLT shortcuts audit (your PDF)

**Local:** `~/Downloads/flt-shortcuts-report.pdf` (also under `~/.grokclean/Downloads/`)  
**Title:** *An Audit of Proposed Shortcuts to Fermat's Last Theorem*  
**Author stack:** GPT-5.6 Sol (Extra High), under OpenAI Codex app  
**Date:** **25 July 2026**

**Principal conclusion (do not misread as "progress on FLT"):**

> None of the audited routes removed the need for a new global theorem comparable in depth to modularity / class-field / Diophantine inputs. Best viewed as an **obstruction map** and collection of **reproducible minor results**, **not** a proof of FLT or evidence a short proof is close. **Novelty not claimed; not peer reviewed.**

**Four audited strategy families:**

1. Specialized Frey / residual modularity shortcuts  
2. Uniform infinite descent (classical small-exponent skeleton → all p)  
3. Radical / Fermat-specific *abc*-style inequalities  
4. Low-genus / genus-zero compression of Fermat curves  

**What the audit *did* produce (method gold):**

- Explicit **open bottlenecks** (e.g. direct conductor-2 nonexistence still deep)  
- **No-go theorems** for specific strategy subclasses (scalar reciprocity alone; modular-unit collapse; proper-subfield norm blindness at p=37; local-trace saturation; positive-genus quotient lower bounds)  
- **Exact computations** (p=37 norm descent degree 1332 → 333; resolvent rank obstructions; maximal relative orders) that **do not** lift to uniform proofs  
- **Sufficient radical criteria** that would imply FLT *if proved*, plus a finite local-pattern theorem showing why local valuation screens cannot prove them  
- Full **artifact + checker table** (Python exact scripts, PARI/GP, per-route audit md files)  
- Explicit **computational caveats**: BNF class numbers under GRH; timed failures ≠ math no-gos; bounded 28,499-pair screens ≠ unbounded Diophantine claims  

**Why people misread this on X:** "Sol audited FLT shortcuts / p=37 calculations" gets compressed into "AI is attacking FLT / headway on FLT." The abstract says the opposite. **RADC lesson:** always lead with **principal conclusion status** before listing impressive intermediate calculations.

**RADC transfer from FLT audit:**

| FLT audit move | RADC analogue |
|----------------|---------------|
| Audit 4 shortcut routes; none remove the global step | Audit "one-token opaque handle wins forever", "Q4-DA lifts to production", "ρ_fail=0 always" -- kill or scope each |
| Common Kummer skeleton without common height | Shared RD algebra without a uniform competitive ratio for all n, ρ_fail, h |
| Sufficient inequalities that imply the goal *if true* | State sharp M / D floors that would dominate Pareto **if proved**; separate the implication from the proof |
| Exact p=37 progress ≠ uniform theorem | Exact Q4-DA certificate ≠ Qn-DA ≠ production TokenZero |
| "One remaining lemma" never appeared | Do not declare RADC proved when only a formal subclass closed |
| Proof-status vocabulary (§0) | Mandatory in every peer audit and bead |
| Obstruction map as the **deliverable** | Wave-4 / Sol Pro: a map of dead ends is a win, not a failure |

#### C2. Other "negative method" notes from the fortnight

- **Acer hygiene posts (~26 Jul):** team elicitation of GPT proofs; careful wording; independent checking; problem interest / nontriviality / generalization questions. Transfer: peer-audit gates, not trophy tweets.  
- **Prior OpenAI math claim overstated** (mentioned in unit-distance / Erdős geometry commentary): announcements lag expert confirmation.  
- **Repeated refinement without new organizing idea** (FLT §6.3): local condition → candidate global invariant → exact calc shows uncontrolled factor → more local structure. Same loop kills RADC runs that only tighten floats.

### D. INFRASTRUCTURE / FORMALIZATION WAVE (method, not theorem)

| Signal | Method value |
|--------|--------------|
| Lean 4 formalizations of Jacobian candidates, Erdős–Gyárfás structure, Erdős #421 repos | Formalization is the **publication medium** |
| Mathlib PR as counterexample vehicle (Grothendieck rank) | Prefer mergeable formal artifacts |
| Gowers / Leiden-declaration discussion (authorship of AI-proved, Lean-verified, refereed results) | Epistemic hygiene for multi-AI campaign outputs |
| New Scientist coverage of back-to-back conjecture disproofs (~23 Jul) | Popular compression; use expert primaries |

---

## 2. Primary sources (Fan–Yu–Wang core -- kept)

| Source | URL | What it is |
|--------|-----|------------|
| X: Eric Alcaide | https://x.com/eric_alcaide/status/2081396897848217828 | Full-range Fan–Yu–Wang **minimizer conjecture is false**; Codex & Fable |
| Parent tweet (WOWII 58) | https://x.com/eric_alcaide/status/2081369042443472973 | Graph-conjecture refutation → 31 vertices; Codex; DeepMind formal-conjectures |
| arXiv | https://arxiv.org/abs/2511.07712 | Tang–Elphick: **proved** spectral upper bound on χ via λ_n; §5 left extremal open |
| Gist | https://gist.github.com/hypnopump/f30cdce1e85510362560ca9b93694919 | Counterexample; Lean + Python; infinite family |
| FLT PDF (local) | `~/Downloads/flt-shortcuts-report.pdf` | **Negative** FLT shortcut audit; obstruction map; proof-status vocab |
| X (catalog) | Lichtman, Fithian, Acer, Bloom commentary threads Jul 15–27 2026 | Expert framing of AI short proofs / disproofs |
| arXiv (related) | e.g. Hessian/Jacobian follow-ons such as 2607.22198 | Post-Jacobian algebraic geometry traffic |

---

## 3. Fan–Yu–Wang mathematical content (one screen -- not for domain transplant)

### What Tang–Elphick **proved** (arXiv:2511.07712)

- For simple graphs, chromatic number χ in \(3 \le \chi \le n-1\), a closed-form upper bound on χ in terms of **least adjacency eigenvalue** λ_n (extends Fan–Yu–Wang range beyond χ ≤ n/2).  
- Equality characterization for a specific join construction when n, χ even.  
- Comparison with Wilf’s χ ≤ 1+λ_1.  
- Open directions including m-edge variants.

### What the Alcaide / hypnopump stack **disproved**

- Fan–Yu–Wang **full-range extremal minimizer** claim: balanced join uniquely minimizes λ_min for all 3 ≤ χ ≤ n−1.  
- **False.** Smallest counterexample (n,χ)=(9,8):  
  - Predicted P = K₄ ∨ (K₄ ∪ O₁)  
  - Better A = (K₃ ∪ O₁) ∨ K₅  
  - λ_min(A) ≈ −2.0801 < −2.0771 ≈ λ_min(P)  
- **Infinite family:** (n,χ)=(2r+1, 2r) for r ≥ 4; optimum drifts **lopsided**.  
- **Does not** refute Tang–Elphick’s **proved inequality**. Scope honesty explicit in the gist.

### WOWII 58 side thread

- Separate graph conjecture; refutation improved to **31 vertices** with Codex; DeepMind formal-conjectures Lean.

---

## 4. Unified method principles for RADC (steal these)

### 4.1 Design loop (prove / disprove / obstruct)

1. State a **bound** (rate, floor, competitive ratio, entropy).  
2. State an **extremal construction** or **policy family** claimed optimal / unique.  
3. Try to **kill uniqueness** (or existence) with a counterexample while **keeping the bound** if it is true.  
4. If kill succeeds → **infinite family** or **phase transition** (n, ρ_fail, h, demand class).  
5. If kill fails after honest search → either strengthen the formal class (Erdős #119 style: harder quantifier, shorter proof) or emit an **obstruction map** (FLT style).  
6. Certificate stack: exact arithmetic + optional Lean core + expert/peer audit + honest "not production" scope.  
7. Label every claim with §0 proof-status tags.

### 4.2 Transfer table (multi-episode)

| Principle | Source episodes | RADC analogue |
|-----------|-----------------|---------------|
| Prove inequality; conjecture extremal | Fan–Yu–Wang / Tang–Elphick | Cost/entropy floor vs "obvious" optimal policy |
| Counterexample kills uniqueness, not bound | Fan–Yu–Wang, unit distance, Jacobian class | ρ_fail phase: theorem true in a gauge, false outside |
| Infinite family after first hit | Fan–Yu–Wang (2r+1,2r); Jacobian follow-ons | Qn-DA lift or prove lift fails |
| Short Gordian-knot proof of **stronger** statement | Erdős #119 | Stronger formal class (demand-aware, zero-error) that collapses cleanly |
| Partial structure + Lean mid-flight | Erdős–Gyárfás 4/7→2/3 | Margin lemmas, handle-cost lemmas as shippable beads |
| Obstruction map as success | **FLT shortcuts audit** | Dead-end log for production RADC routes |
| Exact finite ≠ unbounded theorem | FLT p=37, radical screens | Q4-DA exact game ≠ all n, all ρ_fail |
| Multi-model + multi-tool | Codex+Fable; Claude+Grok; GPT-5.6+expert | Sol Pro + Kimi + Claude + Gemini peer stack |
| Expert is part of the stack | Fithian, Lichtman, Bloom, Gowers-adjacent | Domain + formal peer audit, not model self-score |
| Mathlib / Lean as surface | Grothendieck counterexample PR | Prefer mergeable certificates over PDF vibes |
| Careful claim language | Acer hygiene | "claims", "candidate", "formal class", "not production" |
| formal-conjectures / problem lists | DeepMind wall; Erdős problems site | Our RADC quantifiers + bead wall |

### 4.3 Anti-patterns (from FLT + X hygiene)

- **Impressive intermediate calculation → claim the big theorem** (p=37 ≠ FLT; Q4-DA ≠ RADC).  
- **Bounded search failure → nonexistence theorem.**  
- **Local conditions alone → global nonvanishing.**  
- **Renaming the hard step** (call modularity a black box) ≠ removing it.  
- **Trophy tweet without checker artifacts.**  
- **Confusing disproof of a conjecture with proof of its negation's "deep theory"** -- counterexample can be shallow and still correct.

---

## 5. How this should influence Wave 4 / Sol Pro

Optional add-on message (new chat or mid-run):

```text
Additional METHOD sources (not domain math to re-solve) -- post-cutoff exemplars:

DISPROOF / COUNTEREXAMPLE CLASS (Jul 2026 cluster + spring unit-distance):
- Fan–Yu–Wang full-range minimizer REFUTED (Codex+Fable; gist hypnopump; arXiv:2511.07712 bound SURVIVES)
- Jacobian conjecture claimed counterexample (Fable; Lean explorations)
- Unit-distance disproof (earlier; still cited); BH/FDR-area counterexample (GPT-5.6; Fithian)
- Dinitz–Garg–Goemans claimed disproof; Grothendieck rank/power counterexample via Mathlib PR

PROOF / SHORT RESOLUTION CLASS:
- Erdős #119 strong form claimed via ~1 page harmonic analysis (GPT-5.6; Bloom/Lichtman)
- Erdős–Gyárfás partial: 4/7→2/3 cubic in minimal counterexample + Lean 4

NEGATIVE / OBSTRUCTION CLASS (critical honesty):
- FLT shortcuts audit PDF (GPT-5.6 Sol Codex, 25 Jul 2026): principal conclusion NEGATIVE --
  no short Wiles replacement; obstruction map + minor exact results + proof-status vocabulary.
  Do NOT treat as "headway proving FLT." Treat as template for honest RADC dead-end maps.

Transfer only METHOD:
- bound vs extremal uniqueness
- counterexamples that preserve inequalities
- stronger formal class with shorter proof
- infinite families / phase transitions
- exact + Lean certificates
- obstruction maps when shortcuts fail
- proof-status tags (published / derived / exact / bounded / speculative)
- multi-model + expert peer check

Apply to RADC: (i) cost floors, (ii) claimed optimal policies (Q4-DA / Qn-DA),
(iii) kill uniqueness or lift families, (iv) ρ_fail and handle-cost phases,
(v) production scope honest, (vi) obstruction map for dead routes.
Do NOT spend the run re-proving chromatic graph theory, Jacobian AG, or FLT.
```

**Concrete RADC targets suggested by the multi-episode pattern:**

1. **Extremal policy conjecture:** "Among zero-error demand-aware policies, opaque 1-token handle + expand uniquely minimizes M on Θ_n." → counterexample family (lopsided vs balanced, like G(3,1) vs G(4,0)).  
2. **Phase diagram:** Claude’s ρ_fail critical value as a **phase boundary** over (n, ρ_fail, h).  
3. **Preserved inequality:** entropy / recovery floor remains true even when "optimal capsule" construction is wrong.  
4. **Stronger formal class first:** Qn-DA or demand-aware subclass that admits a short exact proof (Erdős #119 lesson).  
5. **Certificate dual stack:** exact Python on finite games + Lean on algebraic margin identities.  
6. **Obstruction map deliverable:** if production RADC shortcuts fail, emit FLT-style route table (progress / missing global input / assessment) -- that **is** research output.  
7. **Status tags on every lemma** in the peer audit log.

---

## 6. Files in Pareto

| Path | Role |
|------|------|
| `docs/ADJACENT_MATH_AI_PROOF_METHODS.md` | This note (canonical) |
| `solpro-attach-wave4-FLAT/20_ADJACENT_MATH_AI_PROOF_METHODS.md` | Attach copy for Sol Pro |
| Sources online | arXiv, gist, X, Mathlib PRs |
| FLT PDF | `~/Downloads/flt-shortcuts-report.pdf` (attach separately if product allows PDF; else this md summarizes method) |

```bash
cp Pareto/docs/ADJACENT_MATH_AI_PROOF_METHODS.md \
  Pareto/solpro-attach-wave4-FLAT/20_ADJACENT_MATH_AI_PROOF_METHODS.md
cp Pareto/docs/ADJACENT_MATH_AI_PROOF_METHODS.md \
  ~/Downloads/solpro-attach-wave4-FLAT/20_ADJACENT_MATH_AI_PROOF_METHODS.md 2>/dev/null || true
# Optional: copy FLT PDF into a research subfolder for humans (not Kimi md-only)
mkdir -p Pareto/sources/flt-audit
cp ~/Downloads/flt-shortcuts-report.pdf Pareto/sources/flt-audit/ 2>/dev/null \
  || cp ~/.grokclean/Downloads/flt-shortcuts-report.pdf Pareto/sources/flt-audit/ 2>/dev/null || true
```

---

## 7. One-line takeaways

**Graph / Jacobian / Erdős / FDR domain math ≠ RACC math.**  

**AI math workflow this fortnight =**  
(1) short counterexample disproofs,  
(2) short "harder quantifier" proofs,  
(3) partial structural Lean progress,  
(4) honest obstruction maps when big shortcuts fail (FLT audit).  

**That stack is exactly how RADC should be invented -- with proof-status tags, infinite families, phase boundaries, and production scope never overclaimed.**

**FLT PDF is a negative exemplar, not a positive FLT breakthrough.** Lead with that if anyone pastes it into a model context.
