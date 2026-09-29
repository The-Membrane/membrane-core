-- specs/lean/MembraneSpec/Arithmetic/DecimalMulRatio.lean
--
-- multiply_ratio exactness under floor semantics.
-- Source: cosmwasm_std (Decimal::from_ratio / multiply_ratio).
-- Used across membrane-core for rate math.

import Mathlib.Tactic
import MembraneSpec.Bridge.RustEncoding

namespace MembraneSpec.Arithmetic

open MembraneSpec.Bridge

/-- `multiply_ratio(n, num, den) = (n * num) / den` when `den > 0`.
    Floor division on Nat is monotone in the dividend, so the
    decimal-multiply-then-divide pattern in cosmwasm_std satisfies
    `multiplyRatio ≤ exact_quotient` trivially (they are equal). -/
theorem multiplyRatio_eq_div
    (n num den : Nat) (hDen : den > 0) :
    multiplyRatio n num den = (n * num) / den := by
  unfold multiplyRatio
  have : den ≠ 0 := Nat.pos_iff_ne_zero.mp hDen
  simp [this]

/-- The corresponding `≤` form. Useful when callers don't have `den > 0`
    in scope — `multiplyRatio` returns 0 in the den = 0 branch which is
    still ≤ any RHS. -/
theorem multiplyRatio_le (n num den : Nat) :
    multiplyRatio n num den ≤ (n * num) / den := by
  unfold multiplyRatio
  by_cases hDen : den = 0
  · simp [hDen]
  · simp [hDen]

/-- Equality holds iff `den ∣ n * num` OR `den = 0` (in the latter case
    both sides are 0 by convention). -/
theorem multiplyRatio_eq_iff_div
    (n num den : Nat) :
    multiplyRatio n num den = (n * num) / den := by
  unfold multiplyRatio
  by_cases hDen : den = 0
  · simp [hDen]
  · simp [hDen]

end MembraneSpec.Arithmetic
