-- specs/lean/MembraneSpec/Arithmetic/InterestAccrual.lean
--
-- Interest accrual formula additivity over non-overlapping intervals.
-- Source: contracts/cdp/src/rates.rs
-- Docs: docs/README.md:86-92
--   accumulated = principal * rate * elapsed / SECONDS_PER_YEAR
--
-- Floor-rounded sum-of-parts is bounded above by floor of sum.

import Mathlib.Tactic
import MembraneSpec.Bridge.RustEncoding

namespace MembraneSpec.Arithmetic

open MembraneSpec.Bridge

/-- Seconds per year (matches Rust constant). -/
def SECONDS_PER_YEAR : Nat := 31536000

private theorem seconds_per_year_pos : SECONDS_PER_YEAR > 0 := by
  unfold SECONDS_PER_YEAR; decide

/-- Linearized accrual: `principal * rate * elapsed / SECONDS_PER_YEAR`.
    Matches the simplified spec in `docs/README.md:86-92`. The Rust
    contract uses a more elaborate compounding curve at small intervals,
    but this lemma proves the linearized core. -/
def accrued (principal rate elapsed : Nat) : Nat :=
  (principal * rate * elapsed) / SECONDS_PER_YEAR

/-- `InterestAccrual.additive`: floor-rounded sum-of-parts is at most
    the floor of the combined accrual. The discrepancy is at most one
    ulp per partition (one floor-rounding loss per `accrued` call). -/
theorem accrued_additive
    (principal rate t1 t2 : Nat) :
    accrued principal rate t1 + accrued principal rate t2
      ≤ accrued principal rate (t1 + t2) := by
  unfold accrued
  -- principal * rate * (t1 + t2) = principal * rate * t1 + principal * rate * t2
  rw [Nat.mul_add (principal * rate) t1 t2]
  -- Goal: a/c + b/c ≤ (a + b)/c, where c = SECONDS_PER_YEAR.
  -- Use Nat.div_add_div_le or prove directly via Nat.le_div_iff_mul_le.
  set a := principal * rate * t1
  set b := principal * rate * t2
  set c := SECONDS_PER_YEAR with hc_def
  -- Need: a/c + b/c ≤ (a+b)/c.
  -- Direct approach: (a/c + b/c) * c ≤ a + b, then divide.
  have hc : 0 < c := seconds_per_year_pos
  have h1 : a / c * c ≤ a := Nat.div_mul_le_self a c
  have h2 : b / c * c ≤ b := Nat.div_mul_le_self b c
  have h3 : (a / c + b / c) * c ≤ a + b := by
    rw [Nat.add_mul]
    omega
  exact (Nat.le_div_iff_mul_le hc).mpr h3

/-- Identity at zero elapsed time: no interest accrues. -/
theorem accrued_zero (principal rate : Nat) :
    accrued principal rate 0 = 0 := by
  unfold accrued
  simp

/-- Monotonicity in elapsed time: longer intervals never decrease accrual. -/
theorem accrued_mono_elapsed
    (principal rate : Nat) {t1 t2 : Nat} (h : t1 ≤ t2) :
    accrued principal rate t1 ≤ accrued principal rate t2 := by
  unfold accrued
  exact Nat.div_le_div_right (Nat.mul_le_mul_left _ h)

end MembraneSpec.Arithmetic
