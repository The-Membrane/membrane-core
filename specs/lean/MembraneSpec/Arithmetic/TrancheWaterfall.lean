-- specs/lean/MembraneSpec/Arithmetic/TrancheWaterfall.lean
--
-- Transmuter bad-debt waterfall bounds.
-- Source: contracts/transmuter/src/contract.rs:852-873
--   j_loss = min(amount, j.total_staked)
--   s_loss = min(amount - j_loss, s.total_staked)
--
-- Total loss equals the input amount EXACTLY when amount ≤ j+s.

import Mathlib.Tactic
import MembraneSpec.Bridge.RustEncoding

namespace MembraneSpec.Arithmetic

open MembraneSpec.Bridge

/-- Junior tranche absorbs first. -/
def juniorLoss (amount jStaked : Nat) : Nat := min amount jStaked

/-- Senior tranche takes the remainder. -/
def seniorLoss (amount jStaked sStaked : Nat) : Nat :=
  min (amount - juniorLoss amount jStaked) sStaked

/-- `TrancheWaterfall.bounds`: when the cap is respected
    (`amount ≤ j + s`), each tranche-loss is bounded by its tranche
    state and the total loss equals the input amount. -/
theorem waterfall_bounds
    (amount jStaked sStaked : Nat)
    (h : amount ≤ jStaked + sStaked) :
    juniorLoss amount jStaked ≤ jStaked ∧
    seniorLoss amount jStaked sStaked ≤ sStaked ∧
    juniorLoss amount jStaked + seniorLoss amount jStaked sStaked = amount := by
  unfold juniorLoss seniorLoss
  refine ⟨?_, ?_, ?_⟩
  · -- min amount jStaked ≤ jStaked
    exact min_le_right _ _
  · -- min (amount - min amount jStaked) sStaked ≤ sStaked
    exact min_le_right _ _
  · -- The totalling argument: case split on whether amount ≤ jStaked.
    show juniorLoss amount jStaked + min (amount - juniorLoss amount jStaked) sStaked = amount
    unfold juniorLoss
    by_cases hj : amount ≤ jStaked
    · -- Case: junior absorbs everything.
      have h_min_j : min amount jStaked = amount := min_eq_left hj
      rw [h_min_j]
      omega
    · -- Case: junior fills, senior takes remainder.
      push_neg at hj
      have h_min_j : min amount jStaked = jStaked := min_eq_right hj.le
      have h_remainder_le : amount - jStaked ≤ sStaked := by omega
      have h_min_s : min (amount - jStaked) sStaked = amount - jStaked :=
        min_eq_left h_remainder_le
      rw [h_min_j, h_min_s]
      omega

/-- Non-negativity is automatic on `Nat`. Witness exposed for downstream
    consumers that need it explicitly. -/
theorem waterfall_nonneg (amount jStaked sStaked : Nat) :
    juniorLoss amount jStaked ≥ 0 ∧
    seniorLoss amount jStaked sStaked ≥ 0 :=
  ⟨Nat.zero_le _, Nat.zero_le _⟩

end MembraneSpec.Arithmetic
