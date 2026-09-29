// specs/quint/lib/money.qnt
//
// Arithmetic helpers that mirror cosmwasm_std Uint128 / Decimal operations
// and the VT-rate math used across Transmuter and LTV Disco.
// Detailed correctness proofs live in specs/lean/MembraneSpec/Arithmetic/.
//
// Source of truth for rate math:
//   packages/membrane/src/stability_pool_vault.rs (calculate_vault_tokens,
//                                                   calculate_base_tokens)
module money {
  import types.* from "./types"

  // VT mint ratio when pool is empty (docs/organs/ltv-disco/ltv-disco.md:41,
  // docs/organs/transmuter/transmuter.md:29). Disco uses 10^6; Transmuter uses
  // 10^12 precision internally but shares the same zero-supply semantics.
  pure val DEFAULT_VAULT_TOKENS_PER_STAKED_BASE_TOKEN: int = 1000000
  pure val TRANCHE_RATE_PRECISION: int = 1000000000000  // 10^12

  // calculate_vault_tokens — mints VT for `base_amount` into a pool with
  // (`vault_supply`, `total_staked`). Source: stability_pool_vault.rs.
  pure def calculate_vault_tokens(base_amount: Uint128,
                                  total_staked: Uint128,
                                  vault_supply: Uint128): Uint128 =
    if (total_staked == 0)
      base_amount * DEFAULT_VAULT_TOKENS_PER_STAKED_BASE_TOKEN
    else
      (vault_supply * base_amount) / total_staked

  // calculate_base_tokens — burns `vt_amount` for base tokens.
  pure def calculate_base_tokens(vt_amount: Uint128,
                                 total_staked: Uint128,
                                 vault_supply: Uint128): Uint128 =
    if (vault_supply == 0)
      vt_amount / DEFAULT_VAULT_TOKENS_PER_STAKED_BASE_TOKEN
    else
      (total_staked * vt_amount) / vault_supply

  // Per-tranche conversion rate used in TrancheRateAssurance.
  // contracts/transmuter/src/contract.rs:917-918
  pure def tranche_conversion_rate(state_total_staked: Uint128,
                                   state_vt_supply: Uint128): Decimal =
    if (state_vt_supply == 0) 0
    else decimal_from_ratio(state_total_staked, state_vt_supply)

  // Integer square root — abstract. Lean proves `IsqrtMonotone.le`.
  // We expose only the contract we need: a <= b => isqrt a <= isqrt b,
  // and isqrt(x)^2 <= x < (isqrt(x)+1)^2.
  pure def isqrt(n: int): int =
    // Concrete body omitted — Quint model-checking bounds keep n small
    // enough that a naive implementation converges. Implementers: replace
    // with a bounded-iteration newton's method when N grows large.
    if (n <= 0) 0
    else if (n == 1) 1
    else {
      // Find the largest k with k*k <= n, trying 1..n (bounded enough for
      // model-checking samples). NOT sound for arbitrary n — Lean owns the
      // monotonicity proof.
      val candidates = 1.to(n)
      candidates.filter(k => k * k <= n).fold(0, (acc, k) => max_int(acc, k))
    }

  // Liquidation fee cap branch — docs/README.md:106-111, proved in Lean
  // (`LiqFeeCap.safe`). Here we encode the predicate the Quint action uses.
  pure def liq_fee_cap_applies(caller_fee: Decimal,
                               protocol_fee: Decimal,
                               repay_value: Uint128,
                               collateral_value: Uint128): bool =
    decimal_mul_uint(caller_fee + protocol_fee, repay_value) > collateral_value

  // Decimal mul_ratio: multiply_ratio(n, num, den)
  pure def multiply_ratio(n: Uint128, num: Uint128, den: Uint128): Uint128 =
    if (den == 0) 0 else (n * num) / den

  // Manager fee — yield only. Lean proves this never charges on loss.
  // (specs/lean/MembraneSpec/Arithmetic/FeeSplit.lean is indicative.)
  pure def manager_fee_on_yield(realized_yield: int, fee_rate: Decimal): Uint128 =
    if (realized_yield <= 0) 0
    else decimal_mul_uint(fee_rate, realized_yield)
}
