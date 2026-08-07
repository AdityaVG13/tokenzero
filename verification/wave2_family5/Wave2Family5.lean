import Init.Data.List.Pairwise
import Init.Data.List.Lemmas
import Init.Data.Nat.Lemmas

universe u v w

namespace Wave2Family5

def RangeCovered
    {S : Type u} {C : Type v}
    (states : List S)
    (codes : List C)
    (encode : S → C) : Prop :=
  ∀ ⦃s : S⦄, s ∈ states → encode s ∈ codes

def CollisionOn
    {S : Type u} {C : Type v}
    (states : List S)
    (encode : S → C) : Prop :=
  ∃ s₁ : S,
    s₁ ∈ states ∧
    ∃ s₂ : S,
      s₂ ∈ states ∧
      s₁ ≠ s₂ ∧
      encode s₁ = encode s₂

def FiberCompatibleOn
    {S : Type u} {C : Type v} {A : Type w}
    (states : List S)
    (encode : S → C)
    (required : S → A) : Prop :=
  ∀ ⦃s₁ : S⦄,
    s₁ ∈ states →
    ∀ ⦃s₂ : S⦄,
      s₂ ∈ states →
      encode s₁ = encode s₂ →
      required s₁ = required s₂

def ImplementsOn
    {S : Type u} {C : Type v} {A : Type w}
    (states : List S)
    (encode : S → C)
    (required : S → A)
    (downstream : C → A) : Prop :=
  ∀ ⦃s : S⦄,
    s ∈ states →
    downstream (encode s) = required s

theorem mem_map_witness
    {α : Type u} {β : Type v}
    (f : α → β)
    {y : β} {xs : List α}
    (h : y ∈ xs.map f) :
    ∃ x : α, x ∈ xs ∧ f x = y := by
  induction xs with
  | nil =>
      cases h
  | cons a xs ih =>
      cases h with
      | head =>
          exact ⟨a, .head _, rfl⟩
      | tail _ hTail =>
          obtain ⟨x, hx, hfx⟩ := ih hTail
          exact ⟨x, .tail _ hx, hfx⟩

theorem mem_map_of_mem
    {α : Type u} {β : Type v}
    (f : α → β)
    {x : α} {xs : List α}
    (h : x ∈ xs) :
    f x ∈ xs.map f := by
  induction xs with
  | nil =>
      cases h
  | cons a xs ih =>
      cases h with
      | head =>
          exact .head _
      | tail _ hTail =>
          exact .tail _ (ih hTail)

inductive DeletesOne
    {α : Type u}
    (a : α) : List α → List α → Prop
  | head (xs : List α) :
      DeletesOne a (a :: xs) xs
  | tail (b : α) {xs ys : List α} :
      DeletesOne a xs ys →
      DeletesOne a (b :: xs) (b :: ys)

theorem exists_deletesOne_of_mem
    {α : Type u}
    {a : α} {xs : List α}
    (h : a ∈ xs) :
    ∃ ys : List α, DeletesOne a xs ys := by
  induction h with
  | head =>
      exact ⟨_, .head _⟩
  | tail b _ ih =>
      obtain ⟨ys, hDelete⟩ := ih
      exact ⟨b :: ys, .tail b hDelete⟩

theorem deletesOne_length_eq_succ
    {α : Type u}
    {a : α} {xs ys : List α}
    (h : DeletesOne a xs ys) :
    xs.length = Nat.succ ys.length := by
  induction h with
  | head =>
      rfl
  | tail _ _ ih =>
      exact congrArg Nat.succ ih

theorem deletesOne_mem_of_ne
    {α : Type u}
    {a z : α} {xs ys : List α}
    (hDelete : DeletesOne a xs ys) :
    z ≠ a →
    z ∈ xs →
    z ∈ ys := by
  induction hDelete with
  | head tail =>
      intro hNe hMem
      cases hMem with
      | head =>
          exact False.elim (hNe rfl)
      | tail _ hTail =>
          exact hTail
  | tail b hDelete ih =>
      intro hNe hMem
      cases hMem with
      | head =>
          exact .head _
      | tail _ hTail =>
          exact .tail _ (ih hNe hTail)

theorem finiteList_noninjective_collision
    {S : Type u} {C : Type v}
    [DecidableEq C]
    (encode : S → C) :
    ∀ (states : List S) (codes : List C),
      states.Nodup →
      RangeCovered states codes encode →
      codes.length < states.length →
      CollisionOn states encode := by
  intro states
  induction states with
  | nil =>
      intro codes _hNodup _hRange hGap
      exact False.elim (Nat.not_lt_zero codes.length hGap)
  | cons s tail ih =>
      intro codes hNodup hRange hGap
      have hNodupParts : s ∉ tail ∧ tail.Nodup :=
        List.nodup_cons.mp hNodup
      by_cases hHit : encode s ∈ tail.map encode
      · obtain ⟨t, ht, hEq⟩ := mem_map_witness encode hHit
        have hNe : s ≠ t := by
          intro hst
          subst t
          exact hNodupParts.1 ht
        exact
          ⟨s, .head _, t, .tail _ ht, hNe, hEq.symm⟩
      · have hHeadRange : encode s ∈ codes :=
          hRange (.head _)
        obtain ⟨codes', hDelete⟩ :=
          exists_deletesOne_of_mem hHeadRange
        have hTailRange : RangeCovered tail codes' encode := by
          intro t ht
          have htRange : encode t ∈ codes :=
            hRange (.tail _ ht)
          have hNeCode : encode t ≠ encode s := by
            intro hEq
            apply hHit
            have hMapped : encode t ∈ tail.map encode :=
              mem_map_of_mem encode ht
            rw [hEq] at hMapped
            exact hMapped
          exact deletesOne_mem_of_ne hDelete hNeCode htRange
        have hGapTail : codes'.length < tail.length := by
          apply Nat.lt_of_succ_lt_succ
          calc
            Nat.succ codes'.length = codes.length :=
              (deletesOne_length_eq_succ hDelete).symm
            _ < (s :: tail).length := hGap
            _ = Nat.succ tail.length := rfl
        obtain ⟨x, hx, y, hy, hxy, hCode⟩ :=
          ih codes' hNodupParts.2 hTailRange hGapTail
        exact
          ⟨x, .tail _ hx, y, .tail _ hy, hxy, hCode⟩

def firstAction
    {S : Type u} {C : Type v} {A : Type w}
    [DecidableEq C]
    (default : A)
    (encode : S → C)
    (required : S → A) :
    List S → C → A
  | [], _ => default
  | s :: states, code =>
      if encode s = code then
        required s
      else
        firstAction default encode required states code

theorem firstAction_correct_of_fiberCompatible
    {S : Type u} {C : Type v} {A : Type w}
    [DecidableEq C]
    (default : A)
    (encode : S → C)
    (required : S → A) :
    ∀ (states : List S),
      FiberCompatibleOn states encode required →
      ImplementsOn
        states encode required
        (firstAction default encode required states) := by
  intro states
  induction states with
  | nil =>
      intro _hCompatible s hMem
      cases hMem
  | cons head tail ih =>
      intro hCompatible s hMem
      simp only [firstAction]
      by_cases hCode : encode head = encode s
      · rw [if_pos hCode]
        exact hCompatible (.head _) hMem hCode
      · rw [if_neg hCode]
        have hTailCompatible :
            FiberCompatibleOn tail encode required := by
          intro s₁ hs₁ s₂ hs₂ hEq
          exact hCompatible (.tail _ hs₁) (.tail _ hs₂) hEq
        have hTailMem : s ∈ tail := by
          cases hMem with
          | head =>
              exact False.elim (hCode rfl)
          | tail _ hs =>
              exact hs
        exact ih hTailCompatible hTailMem

theorem implementable_implies_fiberCompatible
    {S : Type u} {C : Type v} {A : Type w}
    (encode : S → C)
    (required : S → A)
    (states : List S)
    (downstream : C → A)
    (hImplements :
      ImplementsOn states encode required downstream) :
    FiberCompatibleOn states encode required := by
  intro s₁ hs₁ s₂ hs₂ hCode
  calc
    required s₁ = downstream (encode s₁) :=
      (hImplements hs₁).symm
    _ = downstream (encode s₂) :=
      congrArg downstream hCode
    _ = required s₂ :=
      hImplements hs₂

theorem fiberCompatible_iff_implementable
    {S : Type u} {C : Type v} {A : Type w}
    [DecidableEq C]
    (default : A)
    (encode : S → C)
    (required : S → A)
    (states : List S) :
    FiberCompatibleOn states encode required ↔
    ∃ downstream : C → A,
      ImplementsOn states encode required downstream := by
  constructor
  · intro hCompatible
    exact
      ⟨firstAction default encode required states,
        firstAction_correct_of_fiberCompatible
          default encode required states hCompatible⟩
  · intro hExists
    obtain ⟨downstream, hImplements⟩ := hExists
    exact
      implementable_implies_fiberCompatible
        encode required states downstream hImplements

end Wave2Family5
