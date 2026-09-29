-- specs/lean/MembraneSpec/Arithmetic/IsqrtMonotone.lean
--
-- Integer-sqrt monotonicity used in the LTV Disco slot weight formula.
-- Source: contracts/ltv_disco/src/ (calculate_slot_weights uses isqrt).
-- Docs: docs/README.md:121-127
--   weight(slot) = 10^18 / isqrt(deposits_above + avg_slot_size)

import Mathlib.Tactic
import Mathlib.Data.Nat.Sqrt
import MembraneSpec.Bridge.RustEncoding

namespace MembraneSpec.Arithmetic

open MembraneSpec.Bridge

/-- Integer square root as `Nat.sqrt`. -/
def isqrt (n : Nat) : Nat := Nat.sqrt n

/-- `IsqrtMonotone.le`: isqrt is monotone non-decreasing.
    Direct consequence of `Nat.sqrt_le_sqrt`. -/
theorem isqrt_le_isqrt {a b : Nat} (h : a ≤ b) : isqrt a ≤ isqrt b := by
  unfold isqrt
  exact Nat.sqrt_le_sqrt h

/-- `IsqrtMonotone.bounds`: k = isqrt n satisfies k*k ≤ n < (k+1)*(k+1). -/
theorem isqrt_bounds (n : Nat) :
    isqrt n * isqrt n ≤ n ∧ n < (isqrt n + 1) * (isqrt n + 1) := by
  unfold isqrt
  refine ⟨?_, ?_⟩
  · exact Nat.sqrt_le n
  · exact Nat.lt_succ_sqrt n

/-- The riskiest slot's raw weight is the largest. Witness for
    `DI.inv_slot_weight_riskiest_largest`: when `deposits_above = 0`,
    the divisor `isqrt(0 + avg_slot_size) = isqrt(avg_slot_size)` is the
    smallest possible — strictly smaller than any divisor with
    `deposits_above > 0` — so the resulting weight is the largest. -/
theorem isqrt_strict_mono_lower_bound
    {avg : Nat} (havg : avg > 0)
    (deposits_above : Nat) :
    isqrt avg ≤ isqrt (deposits_above + avg) :=
  isqrt_le_isqrt (Nat.le_add_left avg deposits_above)

end MembraneSpec.Arithmetic
