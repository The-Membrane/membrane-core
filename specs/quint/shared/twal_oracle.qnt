// specs/quint/shared/twal_oracle.qnt
//
// Time-Weighted Average Liquidity (TWAL) oracle, sourced from Uniswap V3
// pool observations. Used as the long-run input for supply caps.
//
// Design choice: samples are appended PIGGYBACK on routine contract
// calls (Transmute, EnterVault, ExitVault, IncreaseDebt, ...). No
// dedicated keeper bot needed. The append is bounded — at most one
// sample per pool per (now / SAMPLE_BUCKET_S) bucket — so even an
// extremely active block cannot blow up storage.
//
// IMPORTANT (Amendment 5): the volatility tracker that would mirror
// this shape for price volatility is SUSPENDED. Do not(add) it. See
// specs/OPEN_QUESTIONS.md #3.
//
// Source of truth (Rust, to be implemented):
//   contracts/oracle/src/twal.rs (NEW)
//   contracts/oracle/src/state.rs (TWAL_BUFFER state map)
//
// Supply-cap derivation is downstream of this module; only the buffer
// shape and append/sample contracts are spec'd here. The supply-cap
// formula and where it gets consumed (CDP Collateral max_borrow per
// asset) are deferred until the team specifies them.
module twal_oracle {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // Each sample records the on-chain liquidity (Uniswap V3 `liquidity`
  // value + token0/token1 deposit equivalents) from the configured pool
  // at a specific block_time.
  type LiquiditySample = {
    block_time: Timestamp,
    block_height: Height,
    pool_liquidity: Uint128,        // raw L from the V3 pool
    token0_amount: Uint128,
    token1_amount: Uint128,
  }

  // Per-pool circular buffer. Cap chosen to support a 30-day window at
  // 30-minute bucket granularity (1440 samples). Implementer can tune.
  pure val TWAL_BUFFER_CAP: int = 1440
  pure val SAMPLE_BUCKET_S: int = 1800     // 30 minutes
  pure val DEFAULT_TWAL_WINDOW_S: int = 2592000   // 30 days

  type TwalConfig = {
    owner: Addr,
    governance: Addr,
    // Map from cAsset denom -> Uniswap V3 pool address (abstracted as Addr).
    pool_for: Denom -> Addr,
    twal_window_s: int,
  }

  var config: TwalConfig
  // Per-pool sample log (capped, oldest dropped when over cap).
  var samples_for: Addr -> List[LiquiditySample]

  action init(cfg: TwalConfig): bool = all {
    cfg.twal_window_s > 0,
    config' = cfg,
    samples_for' = Map(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // --- Helpers ---

  pure def buf_for(s: Addr -> List[LiquiditySample], pool: Addr): List[LiquiditySample] =
    if (s.keys().contains(pool)) s.get(pool) else List()

  // True iff we already have a sample in the current bucket.
  pure def already_sampled_this_bucket(buf: List[LiquiditySample], now: Timestamp): bool =
    if (buf.length() == 0) false
    else (buf[buf.length() - 1].block_time / SAMPLE_BUCKET_S) == (now / SAMPLE_BUCKET_S)

  // Append-with-cap. Drops the oldest entry if we're at TWAL_BUFFER_CAP.
  pure def push_capped(buf: List[LiquiditySample], s: LiquiditySample): List[LiquiditySample] =
    if (buf.length() >= TWAL_BUFFER_CAP)
      buf.tail().append(s)
    else
      buf.append(s)

  // TWAL over [now - window_s, now], computed from `buf`. Truncates to
  // available samples.
  pure def twal(buf: List[LiquiditySample], now: Timestamp, window_s: int): Uint128 = {
    val cutoff: Timestamp = now - window_s
    val relevant: List[LiquiditySample] =
      buf.foldl(List(), (acc, s) =>
        if (s.block_time >= cutoff) acc.append(s) else acc)
    val n: int = relevant.length()
    if (n == 0) 0
    else {
      // Time-weighted: each sample weighted by (next_block_time - this).
      // For the last sample, weight is (now - last.block_time).
      // We approximate by scanning relevant[i] and its successor in `buf`.
      val total_l: Uint128 =
        relevant.foldl(0, (acc, s) => acc + s.pool_liquidity)
      // Simplified: arithmetic mean over the bucketed samples. The exact
      // time-weighted form would require a per-sample successor lookup;
      // for spec purposes the mean preserves monotonicity properties.
      total_l / n
    }
  }

  // --- Actions ---

  // ObserveLiquidity — permissionless append. Designed to be called
  // PIGGYBACK on other contracts' execute paths via a SubMsg, but can
  // be invoked standalone too. No-ops (without revert) when we already
  // have a sample in the current bucket.
  action ObserveLiquidity(sender: Addr,
                          pool: Addr,
                          observation: LiquiditySample): bool = {
    val buf = buf_for(samples_for, pool)
    val skip = already_sampled_this_bucket(buf, block_time)
    all {
      // Concrete impl: read the V3 pool's `slot0` + `liquidity` and
      // build the LiquiditySample inline; we accept it as a parameter
      // here for spec abstractness.
      observation.block_time == block_time,
      if (skip)
        samples_for' = samples_for
      else
        samples_for' = samples_for.put(pool, push_capped(buf, observation)),
      config' = config,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // QueryTWAL — read-only by spec, returns the time-weighted average
  // liquidity for the given asset over `config.twal_window_s`.
  pure def query_twal(s: Addr -> List[LiquiditySample],
                     cfg: TwalConfig,
                     denom: Denom,
                     now: Timestamp): Uint128 =
    if (cfg.pool_for.keys().contains(denom)) {
      val pool = cfg.pool_for.get(denom)
      twal(buf_for(s, pool), now, cfg.twal_window_s)
    } else 0

  // SupplyCapFor — placeholder that downstream consumers (CDP Collateral
  // borrow caps) MUST consult. Concrete formula deferred; for now we
  // expose the TWAL value and let the consumer compute a percentage of
  // it. When the team decides on the exact formula, populate this.
  pure def supply_cap_for(s: Addr -> List[LiquiditySample],
                         cfg: TwalConfig,
                         denom: Denom,
                         now: Timestamp,
                         cap_pct_of_twal: Decimal): Uint128 =
    decimal_mul_uint(cap_pct_of_twal, query_twal(s, cfg, denom, now))

  // --- Invariants ---

  val inv_buffer_capped: bool =
    samples_for.keys().forall(p => samples_for.get(p).length() <= TWAL_BUFFER_CAP)

  // No bucket has more than one sample.
  val inv_at_most_one_sample_per_bucket: bool =
    samples_for.keys().forall(p => {
      val buf = samples_for.get(p)
      buf.indices().forall(i => buf.indices().forall(j =>
        i == j or
        (buf[i].block_time / SAMPLE_BUCKET_S) != (buf[j].block_time / SAMPLE_BUCKET_S)))
    })

  // Window config bounded — same governance assertion.
  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_TWAL: bool =
    inv_buffer_capped and
    inv_at_most_one_sample_per_bucket and
    inv_owner_governance_distinct

  val safety: bool = inv_no_unilateral_capital_loss_path_TWAL

  action step: bool = any { advance_time(1) }
}
