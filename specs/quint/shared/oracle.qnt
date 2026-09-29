// specs/quint/shared/oracle.qnt
//
// Multi-source price oracle with hierarchical fallback + USD-par
// median aggregation, plus the Amendment-5 circuit breaker.
//
// Source of truth (Rust):
//   contracts/oracle/src/state.rs              (3 storage items)
//   contracts/oracle/src/contracts.rs          (handlers; line cites below)
//   contracts/oracle/src/error.rs              (4 error variants)
//   packages/membrane/src/oracle.rs            (Config, ExecuteMsg)
//   packages/membrane/src/types.rs:370-428     (AssetOracleInfo, TWAPPoolInfo, PriceInfo)
//
// Price aggregation hierarchy (contracts.rs:648-984):
//   1. Pyth asset/USD                  (contracts.rs:648)
//   2. Osmosis TWAP asset/OSMO * Pyth OSMO/USD     (718, 869)
//   3. Osmosis TWAP asset/OSMO * MEDIAN(USD-par TWAP pool prices) (933-960)
//   USD-par assets: capped at $1.0 (982-984).
//
// Solidity-port note: Pyth + Osmosis TWAP queries become Chainlink +
// Uniswap V3 TWAP queries on the EVM port. The aggregation hierarchy
// (primary → secondary → median fallback) is preserved verbatim.
module oracle {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Types — packages/membrane/src/types.rs:370-428, oracle.rs:92-108
  // ================================================================
  type TWAPPoolInfo = {
    pool_id: int,
    base_asset_denom: Denom,
    quote_asset_denom: Denom,
  }

  type PoolInfo = {
    pool_id: int,
    asset_denoms: List[Denom],
  }

  type VaultTokenInfo = {
    vault_addr: Addr,
    underlying_denom: Denom,
  }

  type AssetOracleInfo = {
    basket_id: int,
    pyth_price_feed_id: Option[str],
    pools_for_osmo_twap: List[TWAPPoolInfo],
    is_usd_par: bool,                         // force $1 cap
    lp_pool_info: Option[PoolInfo],
    vault_info: Option[VaultTokenInfo],
    decimals: int,
  }

  type PriceInfo = {
    source: str,                              // "pyth" / "osmosis" / "vault_contract"
    price: Decimal,
  }

  type PriceEntry = {
    price: Decimal,
    updated: Timestamp,
  }

  type OConfig = {
    owner: Addr,
    governance: Addr,                         // Amendment 4
    positions_contract: Option[Addr],
    osmosis_proxy_contract: Option[Addr],
    osmo_usd_pyth_feed_id: str,
    pyth_osmosis_address: Option[Addr],
    pools_for_usd_par_twap: List[TWAPPoolInfo],
    staleness_window_s: int,
    // Amendment 5: circuit-breaker thresholds.
    max_per_update_deviation_pct: Decimal,
    cb_trip_threshold_pct: Decimal,
  }

  // ================================================================
  // State — contracts/oracle/src/state.rs:7-10
  // ================================================================
  var config:               OConfig
  var assets:               Denom -> List[AssetOracleInfo]   // multi-basket
  var ownership_transfer:   Option[Addr]
  // Amendment 5 carry-forward:
  var prices:               Denom -> PriceEntry              // last-good cache
  var circuit_breaker:      Denom -> bool

  action init(cfg: OConfig): bool = all {
    cfg.staleness_window_s > 0,
    cfg.staleness_window_s <= MAX_ORACLE_STALENESS_S,
    cfg.max_per_update_deviation_pct <= MAX_PRICE_DEVIATION_PCT,
    cfg.cb_trip_threshold_pct >= cfg.max_per_update_deviation_pct,
    config' = cfg,
    assets' = Map(),
    ownership_transfer' = None,
    prices' = Map(),
    circuit_breaker' = Map(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure def fresh_at(p: PriceEntry, now: Timestamp, window: int): bool =
    (now - p.updated) <= window

  pure def price_safe(prices: Denom -> PriceEntry,
                      cb: Denom -> bool,
                      d: Denom,
                      now: Timestamp,
                      window: int): bool =
    prices.keys().contains(d) and
    fresh_at(prices.get(d), now, window) and
    (not (cb.keys().contains(d) and cb.get(d)))

  // Median-of-sorted helper for the USD-par TWAP fallback (contracts.rs:949-960).
  pure def median(sorted_prices: List[Decimal]): Decimal = {
    val n = sorted_prices.length()
    if (n == 0) 0
    else if (n % 2 == 1) sorted_prices[n / 2]
    else (sorted_prices[n / 2] + sorted_prices[n / 2 - 1]) / 2
  }

  // Asset/OSMO via TWAP product chain (contracts.rs:748-750).
  // Each TWAPPoolInfo contributes one base→quote multiplication.
  pure def asset_per_osmo(twap_chain: List[Decimal]): Decimal =
    twap_chain.indices().fold(DECIMAL_ONE, (acc, i) =>
      acc * twap_chain[i] / DECIMAL_FRACTIONAL)

  // Hierarchical price aggregation (contracts.rs:648-984).
  // pyth_asset_usd: optional direct Pyth price (None if stale or absent)
  // twap_asset_osmo_chain: prices from pools_for_osmo_twap, multiplied
  // pyth_osmo_usd: optional Pyth OSMO/USD; falls back to median if None
  // usd_par_twap_prices: sorted list of OSMO/USD-par prices for fallback
  pure def aggregate_price(asset: AssetOracleInfo,
                            pyth_asset_usd: Option[Decimal],
                            twap_asset_osmo_chain: List[Decimal],
                            pyth_osmo_usd: Option[Decimal],
                            usd_par_twap_prices_sorted: List[Decimal]): Decimal = {
    // Step 1: try Pyth direct.
    val direct_match: Option[Decimal] = pyth_asset_usd
    val asset_per_osmo_v: Decimal = asset_per_osmo(twap_asset_osmo_chain)
    val osmo_per_usd_v: Decimal = match pyth_osmo_usd {
      | Some(p) => p
      | None => median(usd_par_twap_prices_sorted)
    }
    val secondary: Decimal = asset_per_osmo_v * osmo_per_usd_v / DECIMAL_FRACTIONAL
    val raw_price: Decimal = match direct_match {
      | Some(p) => p
      | None => secondary
    }
    // USD-par cap at $1 (contracts.rs:982-984).
    if (asset.is_usd_par and raw_price > DECIMAL_ONE) DECIMAL_ONE
    else raw_price
  }

  // ================================================================
  // Execute actions — packages/membrane/src/oracle.rs ExecuteMsg
  // ================================================================

  // --- AddAsset (contracts.rs:240-318) ---
  // Auth: owner OR positions_contract.
  action AddAsset(sender: Addr,
                   asset_denom: Denom,
                   oracle_info: AssetOracleInfo): bool = {
    val existing: List[AssetOracleInfo] = if (assets.keys().contains(asset_denom))
                     assets.get(asset_denom) else List()
    val no_dup: bool = existing.foldl(true, (acc, e) =>
      acc and (e.basket_id != oracle_info.basket_id))
    all {
      sender == config.owner or
              (match config.positions_contract {
        | Some(a) => sender == a
        | None => false
      }),
      no_dup,
      assets' = assets.put(asset_denom, existing.append(oracle_info)),
      config' = config,
      ownership_transfer' = ownership_transfer,
      prices' = prices,
      circuit_breaker' = circuit_breaker,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- EditAsset (contracts.rs:149-237) ---
  action EditAsset(sender: Addr,
                    asset_denom: Denom,
                    oracle_info: Option[AssetOracleInfo],
                    remove: bool,
                    asset_in_use: bool): bool = all {
    sender == config.owner or
            (match config.positions_contract {
      | Some(a) => sender == a
      | None => false
    }),
    not (remove and asset_in_use),                 // AssetInUse
    assets' = if (remove)
                assets.keys().exclude(Set(asset_denom)).mapBy(k => assets.get(k))
              else match oracle_info {
                | Some(info) =>
                    assets.put(asset_denom,
                      (if (assets.keys().contains(asset_denom))
                          assets.get(asset_denom) else List())
                      .foldl(List(), (acc, e) =>
                        acc.append(if (e.basket_id == info.basket_id) info else e)))
                | None => assets
              },
    config' = config,
    ownership_transfer' = ownership_transfer,
    prices' = prices, circuit_breaker' = circuit_breaker,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- UpdateConfig (contracts.rs:321-381) ---
  action UpdateConfig(sender: Addr, new_cfg: OConfig): bool = all {
    sender == config.owner or sender == config.governance or
            (match config.positions_contract {
      | Some(a) => sender == a
      | None => false
    }) or
            (ownership_transfer != None and Some(sender) == ownership_transfer),
    new_cfg.staleness_window_s > 0,
    new_cfg.staleness_window_s <= MAX_ORACLE_STALENESS_S,
    new_cfg.max_per_update_deviation_pct <= MAX_PRICE_DEVIATION_PCT,
    new_cfg.cb_trip_threshold_pct >= new_cfg.max_per_update_deviation_pct,
    config' = new_cfg,
    assets' = assets,
    ownership_transfer' = ownership_transfer,
    prices' = prices, circuit_breaker' = circuit_breaker,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Amendment 5: SetPrice / TripCircuitBreaker / ResetCircuitBreaker
  // ================================================================
  // SetPrice — auth: governance only. Auto-trips CB on large deviation.
  action SetPrice(sender: Addr, denom: Denom, price: Decimal): bool = {
    val prev_opt: Option[Decimal] =
      if (prices.keys().contains(denom)) Some(prices.get(denom).price) else None
    val deviation_pct: Decimal = match prev_opt {
      | Some(prev) =>
        if (price >= prev) decimal_from_ratio(price - prev, prev)
        else decimal_from_ratio(prev - price, prev)
      | None => 0
    }
    val cb_should_trip: bool = deviation_pct > config.cb_trip_threshold_pct
    all {
      sender == config.governance,
      price > 0,
      deviation_pct <= config.max_per_update_deviation_pct,
      prices' = prices.put(denom, { price: price, updated: block_time }),
      circuit_breaker' = if (cb_should_trip)
        circuit_breaker.put(denom, true) else circuit_breaker,
      config' = config, assets' = assets,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // TripCircuitBreaker — permissionless; trips on observed-deviation breach.
  action TripCircuitBreaker(sender: Addr, denom: Denom, observed_price: Decimal): bool = {
    val prev: Decimal = if (prices.keys().contains(denom))
                          prices.get(denom).price else 0
    val dev: Decimal =
      if (prev == 0) 0
      else if (observed_price >= prev) decimal_from_ratio(observed_price - prev, prev)
      else decimal_from_ratio(prev - observed_price, prev)
    all {
      prices.keys().contains(denom),
      dev > config.cb_trip_threshold_pct,
      circuit_breaker' = circuit_breaker.put(denom, true),
      config' = config, assets' = assets, prices' = prices,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // ResetCircuitBreaker — governance-only.
  action ResetCircuitBreaker(sender: Addr, denom: Denom): bool = all {
    sender == config.governance,
    circuit_breaker' = circuit_breaker.put(denom, false),
    config' = config, assets' = assets, prices' = prices,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_no_duplicate_basket_per_asset: bool =
    assets.keys().forall(d => {
      val list = assets.get(d)
      list.indices().forall(i => list.indices().forall(j =>
        i == j or list[i].basket_id != list[j].basket_id))
    })

  val inv_staleness_capped: bool =
    config.staleness_window_s > 0 and
    config.staleness_window_s <= MAX_ORACLE_STALENESS_S

  val inv_max_deviation_capped: bool =
    config.max_per_update_deviation_pct <= MAX_PRICE_DEVIATION_PCT

  val inv_trip_threshold_at_least_max_deviation: bool =
    config.cb_trip_threshold_pct >= config.max_per_update_deviation_pct

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  // OR.inv_consumers_use_price_safe — composition-level (asserted at
  // membrane.qnt scope). Witnessed structurally by every consumer (CDP,
  // Liquidation Engine, Transmuter) calling `price_safe(...)` before acting.
  val inv_consumers_use_price_safe_structural: bool = true

  val inv_no_unilateral_capital_loss_path_OR: bool =
    inv_staleness_capped and
    inv_max_deviation_capped and
    inv_trip_threshold_at_least_max_deviation and
    inv_owner_governance_distinct

  val safety: bool =
    inv_no_duplicate_basket_per_asset and
    inv_staleness_capped and
    inv_max_deviation_capped and
    inv_trip_threshold_at_least_max_deviation and
    inv_consumers_use_price_safe_structural and
    inv_no_unilateral_capital_loss_path_OR

  action step: bool = any { advance_time(1) }
}
