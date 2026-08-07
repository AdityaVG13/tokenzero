import verification.wave2_family5.Wave2Family5

universe u v w x y

namespace Wave2Family5

def frozenObserved
    {S : Type u} {St : Type v} {O : Type w} {A : Type x}
    (transition : S × St → O × St)
    (frozenState : St)
    (observe : O × St → A)
    (input : S) : A :=
  observe (transition (input, frozenState))

theorem frozenObserved_cardinality_forces_decisionFailure
    {S : Type u} {St : Type v} {O : Type w}
    {A : Type x} {C : Type y}
    [DecidableEq C]
    (transition : S × St → O × St)
    (frozenState : St)
    (observe : O × St → A)
    (encode : S → C)
    (states : List S)
    (codes : List C)
    (hStates : states.Nodup)
    (hRange : RangeCovered states codes encode)
    (hGap : codes.length < states.length)
    (hSeparates :
      ActionSeparatesOn states
        (frozenObserved transition frozenState observe)) :
    ∃ s₁ : S,
      s₁ ∈ states ∧
      ∃ s₂ : S,
        s₂ ∈ states ∧
        s₁ ≠ s₂ ∧
        encode s₁ = encode s₂ ∧
        frozenObserved transition frozenState observe s₁ ≠
          frozenObserved transition frozenState observe s₂ ∧
        ∀ downstream : C → A,
          ¬ (
            downstream (encode s₁) =
              frozenObserved transition frozenState observe s₁ ∧
            downstream (encode s₂) =
              frozenObserved transition frozenState observe s₂
          ) := by
  obtain ⟨s₁, hs₁, s₂, hs₂, hNe, hCode⟩ :=
    finiteList_noninjective_collision
      encode states codes hStates hRange hGap
  have hObservedNe :
      frozenObserved transition frozenState observe s₁ ≠
        frozenObserved transition frozenState observe s₂ :=
    hSeparates hs₁ hs₂ hNe
  exact
    ⟨s₁, hs₁, s₂, hs₂, hNe, hCode, hObservedNe,
      fun downstream =>
        decisionRelevantCollision_not_both_correct
          encode
          (frozenObserved transition frozenState observe)
          downstream hCode hObservedNe⟩

end Wave2Family5
