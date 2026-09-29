import Lake
open Lake DSL

package «membrane_spec» where
  -- Pin the Lean version in lean-toolchain.

-- Mathlib pin. The proofs use Nat.sqrt, Nat.div_mul_le_self,
-- Nat.div_le_div_right, Nat.mul_div_cancel_left, Nat.le_div_iff_mul_le,
-- min_le_right, min_eq_left/right, omega, push_neg, linarith — all
-- stable across recent Mathlib commits.
--
-- Pinned to the v4.13.0-compatible Mathlib release tag. To update:
--   1. cd specs/lean && lake update mathlib
--   2. git -C .lake/packages/mathlib rev-parse HEAD
--   3. replace the @ tag/hash below with that value
--   4. update lean-toolchain to match (lake will warn if mismatched)
-- v4.13.0 SHA: d7317655e2826dc1f1de9a0c138db2775c4bb841
require mathlib from git
  "https://github.com/leanprover-community/mathlib4.git" @ "d7317655e2826dc1f1de9a0c138db2775c4bb841"

@[default_target]
lean_lib «MembraneSpec» where
  globs := #[.andSubmodules `MembraneSpec]
