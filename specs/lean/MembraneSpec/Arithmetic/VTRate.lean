-- specs/lean/MembraneSpec/Arithmetic/VTRate.lean
--
-- Round-trip correctness for vault-token rate conversions.
-- Source: packages/membrane/src/stability_pool_vault.rs
--   calculate_vault_tokens:
--     if total_staked == 0: base * DEFAULT_RATIO
--     else:                 vault_supply * base / total_staked
--   calculate_base_tokens:
--     if vault_supply == 0: vt / DEFAULT_RATIO
--     else:                 total_staked * vt / vault_supply
--
-- Round-tripping through the non-empty-pool branch never INCREASES the
-- base amount (it can lose at most one ulp to floor rounding).

import Mathlib.Tactic
import MembraneSpec.Bridge.RustEncoding

namespace MembraneSpec.Arithmetic

open MembraneSpec.Bridge

def DEFAULT_VT_PER_BASE : Nat := 1000000

/-- VT mint formula. -/
def calculateVaultTokens (baseAmount totalStaked vaultSupply : Uint128) : Uint128 :=
  if totalStaked = 0 then baseAmount * DEFAULT_VT_PER_BASE
  else (vaultSupply * baseAmount) / totalStaked

/-- VT burn formula. -/
def calculateBaseTokens (vtAmount totalStaked vaultSupply : Uint128) : Uint128 :=
  if vaultSupply = 0 then vtAmount / DEFAULT_VT_PER_BASE
  else (totalStaked * vtAmount) / vaultSupply

/-- `VTRate.roundtrip_le`: for non-empty pools, round-tripping through
    mint + burn never increases the base amount. -/
theorem roundtrip_le
    (baseAmount totalStaked vaultSupply : Uint128)
    (hStaked : totalStaked > 0) (hSupply : vaultSupply > 0) :
    calculateBaseTokens
      (calculateVaultTokens baseAmount totalStaked vaultSupply)
      totalStaked vaultSupply
    ≤ baseAmount := by
  unfold calculateBaseTokens calculateVaultTokens
  have hs : totalStaked ≠ 0 := Nat.pos_iff_ne_zero.mp hStaked
  have hv : vaultSupply ≠ 0 := Nat.pos_iff_ne_zero.mp hSupply
  simp [hs, hv]
  -- Goal: (totalStaked * ((vaultSupply * baseAmount) / totalStaked)) / vaultSupply ≤ baseAmount
  -- Step 1: totalStaked * (vaultSupply * baseAmount / totalStaked) ≤ vaultSupply * baseAmount
  have h1 : (vaultSupply * baseAmount) / totalStaked * totalStaked
              ≤ vaultSupply * baseAmount :=
    Nat.div_mul_le_self _ _
  have h2 : totalStaked * ((vaultSupply * baseAmount) / totalStaked)
              ≤ vaultSupply * baseAmount := by
    rw [Nat.mul_comm]; exact h1
  -- Step 2: divide both sides by vaultSupply.
  have h3 :
      (totalStaked * ((vaultSupply * baseAmount) / totalStaked)) / vaultSupply
        ≤ (vaultSupply * baseAmount) / vaultSupply :=
    Nat.div_le_div_right h2
  -- Step 3: (vaultSupply * baseAmount) / vaultSupply = baseAmount.
  have h4 : (vaultSupply * baseAmount) / vaultSupply = baseAmount :=
    Nat.mul_div_cancel_left baseAmount hSupply
  exact Nat.le_trans h3 h4.le

/-- Identity round-trip when staked = supply (each base unit corresponds
    to exactly one VT, modulo floor rounding). -/
theorem roundtrip_identity_at_unit_rate
    (baseAmount staked : Uint128) (h : staked > 0) :
    calculateBaseTokens
      (calculateVaultTokens baseAmount staked staked)
      staked staked
    = baseAmount := by
  unfold calculateBaseTokens calculateVaultTokens
  have hs : staked ≠ 0 := Nat.pos_iff_ne_zero.mp h
  -- After `simp [hs]`, both branches resolve to baseAmount via mul_div_cancel.
  simp [hs, Nat.mul_div_cancel_left baseAmount h]

/-- Empty-pool initialization: first depositor gets DEFAULT_VT_PER_BASE
    VTs per base unit. -/
theorem mint_empty_pool (baseAmount : Uint128) :
    calculateVaultTokens baseAmount 0 0 = baseAmount * DEFAULT_VT_PER_BASE := by
  unfold calculateVaultTokens
  simp

end MembraneSpec.Arithmetic
