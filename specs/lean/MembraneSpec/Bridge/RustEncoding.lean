-- specs/lean/MembraneSpec/Bridge/RustEncoding.lean
--
-- Semantic stubs that mirror the Rust cosmwasm_std integer/decimal
-- contracts used across membrane-core. Kept deliberately thin — we only
-- need enough to express the arithmetic theorems below.
--
-- When the Rust side changes (e.g. Uint128 precision, Decimal scale),
-- sync this file first, then the dependent theorems.

namespace MembraneSpec.Bridge

/-- Mirror of `cosmwasm_std::Uint128`. We model it as a natural number
    with an upper bound check left to downstream theorems. -/
abbrev Uint128 := Nat

/-- Mirror of `cosmwasm_std::Decimal` — fixed-point `n / 10^18`. -/
structure Decimal where
  raw : Nat
deriving Repr, BEq

/-- Decimal fractional unit: 10^18. -/
def DECIMAL_FRACTIONAL : Nat := 1000000000000000000

/-- Decimal `one`. -/
def Decimal.one : Decimal := ⟨DECIMAL_FRACTIONAL⟩
def Decimal.zero : Decimal := ⟨0⟩

/-- Construct a Decimal from a percent (rounds to raw units). -/
def Decimal.ofPercent (p : Nat) : Decimal := ⟨p * (DECIMAL_FRACTIONAL / 100)⟩

/-- Multiply a Uint128 by a Decimal with truncating (floor) division,
    mirroring the floor semantics of `Decimal::mul` in cosmwasm_std. -/
def Decimal.mulUint (d : Decimal) (n : Uint128) : Uint128 :=
  (d.raw * n) / DECIMAL_FRACTIONAL

/-- `multiply_ratio(n, num, den)` with floor semantics, `0` if `den = 0`. -/
def multiplyRatio (n num den : Uint128) : Uint128 :=
  if den = 0 then 0 else (n * num) / den

end MembraneSpec.Bridge
