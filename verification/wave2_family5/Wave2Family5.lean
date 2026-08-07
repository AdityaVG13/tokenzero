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

structure RelEquivalence
    {α : Type u}
    (rel : α → α → Prop) : Prop where
  refl : ∀ a : α, rel a a
  symm : ∀ {a b : α}, rel a b → rel b a
  trans : ∀ {a b c : α}, rel a b → rel b c → rel a c

def RelFiberCompatibleOn
    {S : Type u} {C : Type v} {A : Type w}
    (states : List S)
    (encode : S → C)
    (required : S → A)
    (rel : A → A → Prop) : Prop :=
  ∀ ⦃s₁ : S⦄,
    s₁ ∈ states →
    ∀ ⦃s₂ : S⦄,
      s₂ ∈ states →
      encode s₁ = encode s₂ →
      rel (required s₁) (required s₂)

def RelImplementsOn
    {S : Type u} {C : Type v} {A : Type w}
    (states : List S)
    (encode : S → C)
    (required : S → A)
    (rel : A → A → Prop)
    (downstream : C → A) : Prop :=
  ∀ ⦃s : S⦄,
    s ∈ states →
    rel (downstream (encode s)) (required s)

def EqDownstreamCongruent
    {C : Type u} {A : Type v}
    (rel : A → A → Prop)
    (downstream : C → A) : Prop :=
  ∀ ⦃c₁ c₂ : C⦄,
    c₁ = c₂ →
    rel (downstream c₁) (downstream c₂)

theorem eqDownstreamCongruent_of_refl
    {C : Type u} {A : Type v}
    {rel : A → A → Prop}
    (hRel : RelEquivalence rel)
    (downstream : C → A) :
    EqDownstreamCongruent rel downstream := by
  intro c₁ c₂ hEq
  cases hEq
  exact hRel.refl _

theorem relationDecisionRelevantCollision_not_both_correct
    {S : Type u} {C : Type v} {A : Type w}
    {rel : A → A → Prop}
    (hRel : RelEquivalence rel)
    (encode : S → C)
    (required : S → A)
    (downstream : C → A)
    (hCongruent : EqDownstreamCongruent rel downstream)
    {s₁ s₂ : S}
    (hCode : encode s₁ = encode s₂)
    (hAction : ¬ rel (required s₁) (required s₂)) :
    ¬ (
      rel (downstream (encode s₁)) (required s₁) ∧
      rel (downstream (encode s₂)) (required s₂)
    ) := by
  intro hCorrect
  apply hAction
  exact
    hRel.trans
      (hRel.symm hCorrect.1)
      (hRel.trans (hCongruent hCode) hCorrect.2)

theorem firstAction_relCorrect_of_relFiberCompatible
    {S : Type u} {C : Type v} {A : Type w}
    [DecidableEq C]
    (default : A)
    (encode : S → C)
    (required : S → A)
    (rel : A → A → Prop) :
    ∀ (states : List S),
      RelFiberCompatibleOn states encode required rel →
      RelImplementsOn
        states encode required rel
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
            RelFiberCompatibleOn tail encode required rel := by
          intro s₁ hs₁ s₂ hs₂ hEq
          exact hCompatible (.tail _ hs₁) (.tail _ hs₂) hEq
        have hTailMem : s ∈ tail := by
          cases hMem with
          | head =>
              exact False.elim (hCode rfl)
          | tail _ hs =>
              exact hs
        exact ih hTailCompatible hTailMem

theorem relImplementableCongruent_implies_relFiberCompatible
    {S : Type u} {C : Type v} {A : Type w}
    {rel : A → A → Prop}
    (hRel : RelEquivalence rel)
    (encode : S → C)
    (required : S → A)
    (states : List S)
    (downstream : C → A)
    (hImplements :
      RelImplementsOn states encode required rel downstream)
    (hCongruent : EqDownstreamCongruent rel downstream) :
    RelFiberCompatibleOn states encode required rel := by
  intro s₁ hs₁ s₂ hs₂ hCode
  exact
    hRel.trans
      (hRel.symm (hImplements hs₁))
      (hRel.trans (hCongruent hCode) (hImplements hs₂))

theorem relFiberCompatible_iff_relImplementableCongruent
    {S : Type u} {C : Type v} {A : Type w}
    [DecidableEq C]
    {rel : A → A → Prop}
    (hRel : RelEquivalence rel)
    (default : A)
    (encode : S → C)
    (required : S → A)
    (states : List S) :
    RelFiberCompatibleOn states encode required rel ↔
    ∃ downstream : C → A,
      RelImplementsOn states encode required rel downstream ∧
      EqDownstreamCongruent rel downstream := by
  constructor
  · intro hCompatible
    let downstream :=
      firstAction default encode required states
    have hImplements :
        RelImplementsOn states encode required rel downstream :=
      firstAction_relCorrect_of_relFiberCompatible
        default encode required rel states hCompatible
    have hCongruent :
        EqDownstreamCongruent rel downstream :=
      eqDownstreamCongruent_of_refl hRel downstream
    exact ⟨downstream, hImplements, hCongruent⟩
  · intro hExists
    obtain ⟨downstream, hImplements, hCongruent⟩ := hExists
    exact
      relImplementableCongruent_implies_relFiberCompatible
        hRel encode required states downstream
        hImplements hCongruent

structure ExactEnum (α : Type u) where
  elems : List α
  nodup : elems.Nodup
  complete : ∀ a : α, a ∈ elems

def boolExactEnum : ExactEnum Bool where
  elems := [false, true]
  nodup := by
    simp
  complete := by
    intro b
    cases b <;> simp

inductive ProtectedDisposition where
  | admit
  | deopt
  | unknown

def protectedDispositionExactEnum :
    ExactEnum ProtectedDisposition where
  elems := [
    ProtectedDisposition.admit,
    ProtectedDisposition.deopt,
    ProtectedDisposition.unknown
  ]
  nodup := by
    simp
  complete := by
    intro disposition
    cases disposition <;> simp

theorem exactEnum_noninjective_collision
    {S : Type u} {C : Type v}
    [DecidableEq C]
    (stateEnum : ExactEnum S)
    (codeEnum : ExactEnum C)
    (encode : S → C)
    (hGap :
      codeEnum.elems.length < stateEnum.elems.length) :
    CollisionOn stateEnum.elems encode := by
  apply
    finiteList_noninjective_collision
      encode stateEnum.elems codeEnum.elems
  · exact stateEnum.nodup
  · intro s _hMem
    exact codeEnum.complete (encode s)
  · exact hGap

def ActionSeparatesOn
    {S : Type u} {A : Type v}
    (states : List S)
    (required : S → A) : Prop :=
  ∀ ⦃s₁ : S⦄,
    s₁ ∈ states →
    ∀ ⦃s₂ : S⦄,
      s₂ ∈ states →
      s₁ ≠ s₂ →
      required s₁ ≠ required s₂

theorem decisionRelevantCollision_not_both_correct
    {S : Type u} {C : Type v} {A : Type w}
    (encode : S → C)
    (required : S → A)
    (downstream : C → A)
    {s₁ s₂ : S}
    (hCode : encode s₁ = encode s₂)
    (hAction : required s₁ ≠ required s₂) :
    ¬ (
      downstream (encode s₁) = required s₁ ∧
      downstream (encode s₂) = required s₂
    ) := by
  intro hCorrect
  apply hAction
  calc
    required s₁ = downstream (encode s₁) :=
      hCorrect.1.symm
    _ = downstream (encode s₂) :=
      congrArg downstream hCode
    _ = required s₂ :=
      hCorrect.2

def frozenTransition
    {S : Type u} {St : Type v} {O : Type w}
    (transition : S × St → O × St)
    (frozenState : St)
    (input : S) : O × St :=
  transition (input, frozenState)

def frozenOutput
    {S : Type u} {St : Type v} {O : Type w}
    (transition : S × St → O × St)
    (frozenState : St)
    (input : S) : O :=
  (frozenTransition transition frozenState input).1

theorem frozenTransition_cardinality_forces_decisionFailure
    {S : Type u} {St : Type v} {O : Type w} {C : Type}
    [DecidableEq C]
    (transition : S × St → O × St)
    (frozenState : St)
    (encode : S → C)
    (states : List S)
    (codes : List C)
    (hStates : states.Nodup)
    (hRange : RangeCovered states codes encode)
    (hGap : codes.length < states.length)
    (hSeparates :
      ActionSeparatesOn states
        (frozenOutput transition frozenState)) :
    ∃ s₁ : S,
      s₁ ∈ states ∧
      ∃ s₂ : S,
        s₂ ∈ states ∧
        s₁ ≠ s₂ ∧
        encode s₁ = encode s₂ ∧
        frozenOutput transition frozenState s₁ ≠
          frozenOutput transition frozenState s₂ ∧
        ∀ downstream : C → O,
          ¬ (
            downstream (encode s₁) =
              frozenOutput transition frozenState s₁ ∧
            downstream (encode s₂) =
              frozenOutput transition frozenState s₂
          ) := by
  obtain ⟨s₁, hs₁, s₂, hs₂, hNe, hCode⟩ :=
    finiteList_noninjective_collision
      encode states codes hStates hRange hGap
  have hOutputNe :
      frozenOutput transition frozenState s₁ ≠
        frozenOutput transition frozenState s₂ :=
    hSeparates hs₁ hs₂ hNe
  exact
    ⟨s₁, hs₁, s₂, hs₂, hNe, hCode, hOutputNe,
      fun downstream =>
        decisionRelevantCollision_not_both_correct
          encode
          (frozenOutput transition frozenState)
          downstream hCode hOutputNe⟩

def FixedModelPreservesOn
    {S : Type u} {I : Type v} {O : Type w}
    (states : List S)
    (rawInput : S → I)
    (encodedInput : S → I)
    (model : I → O) : Prop :=
  ∀ ⦃s : S⦄,
    s ∈ states →
    model (encodedInput s) = model (rawInput s)

theorem installedDecoder_preservesOn
    {S : Type u} {C : Type v} {A : Type w}
    (states : List S)
    (encode : S → C)
    (decode : C → S)
    (baseline : S → A)
    (hLeftInverse :
      ∀ ⦃s : S⦄,
        s ∈ states →
        decode (encode s) = s) :
    ImplementsOn
      states encode baseline
      (fun code => baseline (decode code)) := by
  intro s hs
  exact congrArg baseline (hLeftInverse hs)

def boolNotEncoding (x : Bool) : Bool :=
  !x

def boolNotDecoder (x : Bool) : Bool :=
  !x

def boolIdentity (x : Bool) : Bool :=
  x

theorem boolNot_fiberCompatible :
    FiberCompatibleOn
      [false, true]
      boolNotEncoding
      boolIdentity := by
  intro s₁ _hs₁ s₂ _hs₂ hCode
  cases s₁ <;> cases s₂ <;> cases hCode <;> rfl

theorem boolNot_installedDecoder_preserves :
    ImplementsOn
      [false, true]
      boolNotEncoding
      boolIdentity
      (fun code => boolIdentity (boolNotDecoder code)) := by
  apply installedDecoder_preservesOn
  intro s _hs
  cases s <;> rfl

theorem boolNot_not_fixedModelPreserves :
    ¬ FixedModelPreservesOn
      [false, true]
      boolIdentity
      boolNotEncoding
      boolIdentity := by
  intro hPreserves
  have hBad : true = false :=
    hPreserves (.head _)
  cases hBad

theorem fiberCompatible_and_installedDecoder_do_not_imply_directPreservation :
    FiberCompatibleOn
        [false, true]
        boolNotEncoding
        boolIdentity ∧
    ImplementsOn
        [false, true]
        boolNotEncoding
        boolIdentity
        (fun code => boolIdentity (boolNotDecoder code)) ∧
    ¬ FixedModelPreservesOn
        [false, true]
        boolIdentity
        boolNotEncoding
        boolIdentity := by
  exact
    ⟨boolNot_fiberCompatible,
      boolNot_installedDecoder_preserves,
      boolNot_not_fixedModelPreserves⟩

theorem decoderInputPathBridge_implies_fixedModelPreserves
    {S : Type u} {C : Type v} {I : Type w} {O : Type}
    (states : List S)
    (encode : S → C)
    (decode : C → S)
    (rawInput : S → I)
    (encodedInput : S → I)
    (model : I → O)
    (hLeftInverse :
      ∀ ⦃s : S⦄,
        s ∈ states →
        decode (encode s) = s)
    (hDeployedPath :
      ∀ ⦃s : S⦄,
        s ∈ states →
        encodedInput s = rawInput (decode (encode s))) :
    FixedModelPreservesOn
      states rawInput encodedInput model := by
  intro s hs
  calc
    model (encodedInput s) =
        model (rawInput (decode (encode s))) :=
      congrArg model (hDeployedPath hs)
    _ = model (rawInput s) :=
      congrArg
        (fun restored => model (rawInput restored))
        (hLeftInverse hs)

end Wave2Family5
