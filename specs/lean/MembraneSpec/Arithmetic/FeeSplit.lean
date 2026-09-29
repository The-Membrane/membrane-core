-- specs/lean/MembraneSpec/Arithmetic/FeeSplit.lean
--
-- Transmuter usage-fee split exactness.
-- Source: contracts/transmuter/src/contract.rs:972-995
--   fee_amount = usage_fee * input_amount       (Decimal × Uint128, floor)
--   out_amount = input_amount - fee_amount
--
-- With floor semantics, `fee + out == input` exactly when `fee ≤ input`.
-- That precondition holds whenever `usage_fee ≤ 1` (since the fractional
-- multiplier never exceeds 1). The Quint-side cap MAX_USAGE_FEE = 5%
-- guarantees this; we encode the more general `≤ 1` here.

import Mathlib.Tactic
import MembraneSpec.Bridge.RustEncoding

namespace MembraneSpec.Arithmetic

open MembraneSpec.Bridge

/-- DECIMAL_FRACTIONAL > 0. Helper used throughout. -/
private theorem decimal_fractional_pos : DECIMAL_FRACTIONAL > 0 := by
  unfold DECIMAL_FRACTIONAL
  decide

/-- The fee never exceeds the input when `d ≤ 1`. -/
theorem fee_le_input
    (d : Decimal) (n : Uint128)
    (hLe : d.raw ≤ DECIMAL_FRACTIONAL) :
    d.mulUint n ≤ n := by
  unfold Decimal.mulUint
  -- Goal: (d.raw * n) / DECIMAL_FRACTIONAL ≤ n
  -- d.raw * n ≤ DECIMAL_FRACTIONAL * n
  have hMul : d.raw * n ≤ DECIMAL_FRACTIONAL * n :=
    Nat.mul_le_mul_right n hLe
  -- (d.raw * n) / D ≤ (D * n) / D
  have hDiv : (d.raw * n) / DECIMAL_FRACTIONAL ≤ (DECIMAL_FRACTIONAL * n) / DECIMAL_FRACTIONAL :=
    Nat.div_le_div_right hMul
  -- (D * n) / D = n
  have hRhs : (DECIMAL_FRACTIONAL * n) / DECIMAL_FRACTIONAL = n :=
    Nat.mul_div_cancel_left n decimal_fractional_pos
  exact Nat.le_trans hDiv hRhs.le

/-- `fee + out == input` exactly. The floor-rounding residual lives entirely
    inside `fee`; the subtraction `input - fee` recovers the rest. -/
theorem fee_split_exact
    (d : Decimal) (n : Uint128)
    (hLe : d.raw ≤ DECIMAL_FRACTIONAL) :
    d.mulUint n + (n - d.mulUint n) = n := by
  exact Nat.add_sub_cancel' (fee_le_input d n hLe)

/-- Symmetric form: `out = input - fee`. -/
theorem out_amount_eq
    (d : Decimal) (n : Uint128)
    (hLe : d.raw ≤ DECIMAL_FRACTIONAL) :
    n - d.mulUint n = n - d.mulUint n := rfl

end MembraneSpec.Arithmetic
