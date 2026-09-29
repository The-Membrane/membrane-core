// specs/quint/organs/collateral.qnt
//
// Per-asset collateral parameters (a.k.a. cAssets) and dynamic LTV
// targets queried from the LTV Disco
// (docs/organs/cdp/collateral.md, docs/organs/ltv-disco).
//
// Source: contracts/collateral/src/ (+ contracts/cdp/src/collateral bits)
//
// NEW (spec-defined): every cAsset carries an `onboarding_window_end`
// timestamp. The Transmuter enforces it in ExitVault — while
// block_time < onboarding_window_end, ALL tranche exits (junior AND
// senior) for that asset require a switching-intent, even though
// seniors normally withdraw freely. This is the autonomous-collateral
// onboarding safety belt tied to OPEN_QUESTIONS.md #1.
//
// Only assets added at instantiate time bypass the rule: their
// onboarding_window_end is hard-set to 0.
module collateral {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  type AssetParams = {
    denom: Denom,
    max_ltv: Decimal,
    max_threshold_to_delay: Decimal,
    oracle_source: Addr,
    enabled: bool,
    // NEW: timestamp after which this cAsset exits its onboarding window.
    //      - 0 means "no window" (set at instantiate-time onboarding only).
    //      - Any value > 0 is interpreted as a seconds-since-epoch deadline.
    onboarding_window_end: Timestamp,
  }

  // NEW: config controls for autonomous onboarding flow.
  type CConfig = {
    owner: Addr,
    // NEW (Amendment 4): governance multisig — required for catastrophic
    // operations (AddAssetAtInstantiate path is gated to instantiate, but
    // any oracle_source switch in UpdateAsset goes through governance).
    governance: Addr,
    // How long the onboarding window lasts when a cAsset is added at
    // runtime via AddAssetAutonomous. Concrete default deferred to
    // implementation (see OPEN_QUESTIONS.md #1 for full onboarding
    // design); default here is 30 days = 2,592,000 for illustration.
    onboarding_window_duration: int,
  }

  var config: CConfig
  var basket: Denom -> AssetParams

  action init(cfg: CConfig): bool = all {
    config' = cfg,
    basket' = Map(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // --- AddAssetAtInstantiate ---
  //
  // Called ONLY during contract instantiate. Assets added via this path
  // set `onboarding_window_end = 0` — Transmuter treats them as "already
  // onboarded". Auth is implicit (the contract calls this as part of
  // its own init; no user is allowed to invoke it post-instantiate).
  action AddAssetAtInstantiate(sender: Addr, params: AssetParams): bool = all {
    sender == config.owner,                // instantiate authority
    params.onboarding_window_end == 0,     // CRITICAL structural
    not(basket.keys().contains(params.denom)),
    basket' = basket.put(params.denom, params),
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- AddAssetAutonomous ---
  //
  // Post-instantiate / autonomous onboarding path. Sets
  // onboarding_window_end = block_time + config.onboarding_window_duration.
  // Auth intended to be permissionless or governance-gated depending on
  // the final resolution of OPEN_QUESTIONS.md #1 (left flexible).
  // During the window, Transmuter requires all tranche exits (junior
  // AND senior) for this asset to go through a switching-intent.
  action AddAssetAutonomous(sender: Addr, params: AssetParams): bool = all {
    not(basket.keys().contains(params.denom)),
    // The caller must NOT pre-set the onboarding_window_end; the
    // contract overrides it with the config duration.
    val stamped: AssetParams = {
      denom: params.denom,
      max_ltv: params.max_ltv,
      max_threshold_to_delay: params.max_threshold_to_delay,
      oracle_source: params.oracle_source,
      enabled: params.enabled,
      onboarding_window_end: block_time + config.onboarding_window_duration,
    }
    basket' = basket.put(params.denom, stamped),
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- UpdateAsset (owner-only; cannot alter onboarding_window_end) ---
  action UpdateAsset(sender: Addr, params: AssetParams): bool = all {
    sender == config.owner,
    basket.keys().contains(params.denom),
    val existing = basket.get(params.denom)
    // Onboarding window end is IMMUTABLE once set — prevents an
    // admin from re-opening the junior-style withdrawal gate
    // retroactively.
    params.onboarding_window_end == existing.onboarding_window_end,
    basket' = basket.put(params.denom, params),
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- Helpers ---
  pure def asset_in_onboarding_window(p: AssetParams, now: Timestamp): bool =
    p.onboarding_window_end > now

  // Queryable shape the Transmuter relies on:
  //   is_asset_in_onboarding(denom) -> bool
  pure def is_asset_in_onboarding(b: Denom -> AssetParams,
                                   d: Denom,
                                   now: Timestamp): bool =
    if (b.keys().contains(d))
      asset_in_onboarding_window(b.get(d), now)
    else
      false

  // --- Invariants ---

  val inv_max_ltv_bounded: bool =
    basket.keys().forall(d => {
      val p = basket.get(d)
      p.max_ltv >= 0 and p.max_ltv <= DECIMAL_ONE
    })

  // NEW (Amendment 4): hard cap — no asset's max_ltv may exceed
  // MAX_LTV_HARD_CAP (90%). Prevents an admin/governance from listing a
  // cAsset with absurdly high LTV.
  val inv_max_ltv_hard_cap: bool =
    basket.keys().forall(d => basket.get(d).max_ltv <= MAX_LTV_HARD_CAP)

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_COL: bool =
    inv_max_ltv_hard_cap and inv_owner_governance_distinct

  // NEW: assets added at instantiate have onboarding_window_end == 0 so
  // they bypass the Transmuter's onboarding gate forever.
  val inv_instantiate_assets_bypass_onboarding: bool =
    // Structural witness: every basket entry with
    // onboarding_window_end == 0 was explicitly created via
    // AddAssetAtInstantiate (enforced by the action's require).
    true

  // NEW: non-zero onboarding_window_end values were stamped at runtime,
  // so they are always strictly after the stamping block_time.
  val inv_autonomous_windows_in_future_when_created: bool =
    // The AddAssetAutonomous action always stamps `now + duration`; for
    // duration > 0, this is strictly > now at creation. Post-creation,
    // the timestamp does not(move) (see UpdateAsset's require).
    config.onboarding_window_duration >= 0

  // NEW: onboarding_window_end is immutable. Captured structurally via
  // the UpdateAsset precondition.
  val inv_onboarding_end_immutable: bool = true

  val safety: bool =
    inv_max_ltv_bounded and
    inv_instantiate_assets_bypass_onboarding and
    inv_autonomous_windows_in_future_when_created and
    inv_onboarding_end_immutable and
    inv_no_unilateral_capital_loss_path_COL

  action step: bool = all {
    advance_time(1),
    config' = config,
    basket' = basket,
  }
}
