import verification.wave2_family5.Wave2Family5

namespace Wave2Family5

def boolExactEnumExplicit : ExactEnum Bool where
  elems := [false, true]
  nodup := by
    apply List.nodup_cons.mpr
    constructor
    · intro hMem
      cases hMem
    · apply List.nodup_cons.mpr
      constructor
      · intro hMem
        cases hMem
      · exact List.nodup_nil
  complete := by
    intro b
    cases b with
    | false =>
        exact .head _
    | true =>
        exact .tail _ (.head _)

end Wave2Family5
