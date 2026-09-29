-- specs/lean/MembraneSpec/Arithmetic/LiqFeeCap.lean
--
-- Liquidation fee-cap safety.
-- Source: contracts/liquidation-engine/src/reply.rs:362-388
-- Docs: docs/README.md:106-111
--   if (caller_fee + protocol_fee) * repay_value > collateral_value:
--     caller_fee  = min(1%, collateral_value / repay_value)
--     protocol_fee = 0
--
-- After the cap fires, `caller_fee * repay_value ≤ collateral_value`
-- holds without exception.

import Mathlib.Tactic
import MembraneSpec.Bridge.RustEncoding

namespace MembraneSpec.Arithmetic

open MembraneSpec.Bridge

/-- One-percent raw value (matches Decimal::percent(1) on cosmwasm_std). -/
def ONE_PERCENT_RAW : Nat := DECIMAL_FRACTIONAL / 100

private theorem decimal_fractional_pos : DECIMAL_FRACTIONAL > 0 := by
  unfold DECIMAL_FRACTIONAL; decide

/-- Helper: when `repayValue > 0`, the collateral-ratio Decimal
    `(collateralValue * D) / repayValue` cleanly multiplies back to a
    bound on `collateralValue`. -/
private theorem collRatio_mul_repay_le
    (repayValue collateralValue : Nat) (hRepay : repayValue > 0) :
    let collRatioRaw := (collateralValue * DECIMAL_FRACTIONAL) / repayValue
    (collRatioRaw * repayValue) / DECIMAL_FRACTIONAL ≤ collateralValue := by
  intro collRatioRaw
  -- collRatioRaw * repayValue ≤ collateralValue * D
  have h1 : (collateralValue * DECIMAL_FRACTIONAL) / repayValue * repayValue
              ≤ collateralValue * DECIMAL_FRACTIONAL := by
    exact Nat.div_mul_le_self _ _
  -- Divide both sides by D to recover the bound on collateralValue.
  have h2 : ((collateralValue * DECIMAL_FRACTIONAL) / repayValue * repayValue)
              / DECIMAL_FRACTIONAL
            ≤ (collateralValue * DECIMAL_FRACTIONAL) / DECIMAL_FRACTIONAL :=
    Nat.div_le_div_right h1
  have h3 : (collateralValue * DECIMAL_FRACTIONAL) / DECIMAL_FRACTIONAL
              = collateralValue :=
    Nat.mul_div_cancel collateralValue decimal_fractional_pos
  exact h3 ▸ h2

/-- Same shape but with the `min(1%, collRatio)` cap factored explicitly. -/
private theorem onePct_capped_mul_repay_le
    (repayValue collateralValue : Nat) (hRepay : repayValue > 0) :
    let collRatioRaw := (collateralValue * DECIMAL_FRACTIONAL) / repayValue
    let cappedRaw := min ONE_PERCENT_RAW collRatioRaw
    (cappedRaw * repayValue) / DECIMAL_FRACTIONAL ≤ collateralValue := by
  intro collRatioRaw cappedRaw
  -- cappedRaw ≤ collRatioRaw, so cappedRaw * repay ≤ collRatioRaw * repay.
  have hCap : cappedRaw ≤ collRatioRaw := min_le_right _ _
  have hMul : cappedRaw * repayValue ≤ collRatioRaw * repayValue :=
    Nat.mul_le_mul_right repayValue hCap
  have hDivLe : (cappedRaw * repayValue) / DECIMAL_FRACTIONAL
                  ≤ (collRatioRaw * repayValue) / DECIMAL_FRACTIONAL :=
    Nat.div_le_div_right hMul
  have hRatioBound :
      (collRatioRaw * repayValue) / DECIMAL_FRACTIONAL ≤ collateralValue :=
    collRatio_mul_repay_le repayValue collateralValue hRepay
  omega

/-- `LiqFeeCap.safe`: under the cap branch, the caller fee multiplied by
    repay value is bounded above by collateral value, ensuring the
    liquidator can always be paid in full from the position's
    collateral. -/
theorem liq_fee_cap_safe
    (repayValue collateralValue : Nat) (hRepay : repayValue > 0) :
    let onePct : Decimal := ⟨ONE_PERCENT_RAW⟩
    let collRatio : Decimal := ⟨(collateralValue * DECIMAL_FRACTIONAL) / repayValue⟩
    let capped : Decimal :=
      if onePct.raw ≤ collRatio.raw then onePct else collRatio
    capped.mulUint repayValue ≤ collateralValue := by
  intro onePct collRatio capped
  unfold Decimal.mulUint
  by_cases h : onePct.raw ≤ collRatio.raw
  · -- capped = onePct = ⟨ONE_PERCENT_RAW⟩.
    simp [capped, h]
    -- Goal: (ONE_PERCENT_RAW * repayValue) / D ≤ collateralValue
    -- ONE_PERCENT_RAW ≤ collRatio.raw by hypothesis.
    have hMul : ONE_PERCENT_RAW * repayValue
                  ≤ ((collateralValue * DECIMAL_FRACTIONAL) / repayValue) * repayValue :=
      Nat.mul_le_mul_right repayValue h
    have hDivLe : (ONE_PERCENT_RAW * repayValue) / DECIMAL_FRACTIONAL
                    ≤ (((collateralValue * DECIMAL_FRACTIONAL) / repayValue) * repayValue)
                        / DECIMAL_FRACTIONAL :=
      Nat.div_le_div_right hMul
    have hBound :
        (((collateralValue * DECIMAL_FRACTIONAL) / repayValue) * repayValue)
          / DECIMAL_FRACTIONAL ≤ collateralValue :=
      collRatio_mul_repay_le repayValue collateralValue hRepay
    exact Nat.le_trans hDivLe hBound
  · -- capped = collRatio = ⟨(collateralValue * D) / repayValue⟩.
    push_neg at h
    simp [capped, Nat.not_le.mpr h]
    -- Goal: ((collateralValue * D / repayValue) * repayValue) / D ≤ collateralValue
    exact collRatio_mul_repay_le repayValue collateralValue hRepay

end MembraneSpec.Arithmetic
