module exp {
  import types.* from "../lib/types"
  import env.* from "../lib/env"
  import auction as AU from "../organs/auction"
  import ltv_disco as DI from "../organs/ltv_disco"

  pure val AU_CFG: AU::Config = {
    owner: "owner", governance: "gov",
    oracle_contract: "oracle", osmosis_proxy: "ox",
    mbrn_denom: "mbrn", cdt_denom: "cdt", desired_asset: "uosmo",
    positions_contract: "pos", governance_contract: "govc", staking_contract: "stk",
    twap_timeframe_m: 60, initial_discount: 30000000000000000,
    discount_increase_timeframe_s: 60, discount_increase: 10000000000000000,
    send_to_stakers: false, delay_window_m: 60,
    revenue_distributor_contract: None, ltv_disco_contract: Some("disco"),
    debt_contract: Some("dbt"),
  }
  pure val DI_CFG: DI::DConfig = {
    owner: "owner", governance: "gov", cdp_contract: "cdp",
    deposit_denom: "uatom", cdt_denom: "cdt", minimum_deposit: 100,
    unstaking_period: 1209600, unstake_execution_window: 172800,
    ltv_switching_period: 1209600, ltv_switching_execution_window: 172800,
    manager_fee_change_period: 1209600, manager_fee_change_execution_window: 172800,
    oracle_contract: "oracle", chain_proxy_contract: "chain",
    affiliate_fee: 30000000000000000, max_management_fee: 200000000000000000,
    auction_contract: Some("auction"), mbrn_denom: Some("mbrn"),
  }

  // Init AU + DI defaults, then DI + AU identity. AU's config is set by AU::init,
  // DI's config is set by DI::init. Both via the SAME runtime 'config' slot
  // — but DI's value overwrites AU's. AU::config.initial_discount might be
  // unreachable.
  action initAUWrapped: bool = all {
    AU::init(AU_CFG),
    DI::asset_queues' = Map(), DI::backing_deposits' = Map(),
    DI::user_deposits' = Map(), DI::managed_deposits' = Map(),
    DI::manager_fee' = Map(), DI::pending_bad_debt' = Map(),
    DI::rate_assurance' = Map(), DI::revenue_events' = Map(),
    DI::unstake_requests' = Map(), DI::affiliates' = Map(),
    DI::lower_ltv_intents' = Map(), DI::next_ltv_intent_id' = 1,
    DI::deposit_ltv_intent' = Map(), DI::pending_fee_change' = Map(),
    DI::pending_config_change' = Map(), DI::events' = List(),
  }
  action initDIWrapped: bool = all {
    DI::init(DI_CFG),
    AU::fee_auctions' = AU::fee_auctions,
    AU::mbrn_sale' = AU::mbrn_sale,
    AU::ownership_transfer' = AU::ownership_transfer,
  }

  action testInit: bool = initAUWrapped.then(initDIWrapped)

  // Test: after init, what's in 'AU::mbrn_sale'?
  val mbrn_test: bool = match AU::mbrn_sale {
    | Some(_) => false
    | None => true
  }

  // Test: after init, what's in DI::pending_bad_debt size?
  val pbd_test: bool = DI::pending_bad_debt.keys().size() == 0

  action noop: bool = all {
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    AU::fee_auctions' = AU::fee_auctions,
    AU::mbrn_sale' = AU::mbrn_sale,
    AU::ownership_transfer' = AU::ownership_transfer,
    DI::asset_queues' = DI::asset_queues, DI::backing_deposits' = DI::backing_deposits,
    DI::user_deposits' = DI::user_deposits, DI::managed_deposits' = DI::managed_deposits,
    DI::manager_fee' = DI::manager_fee, DI::pending_bad_debt' = DI::pending_bad_debt,
    DI::rate_assurance' = DI::rate_assurance, DI::revenue_events' = DI::revenue_events,
    DI::unstake_requests' = DI::unstake_requests, DI::affiliates' = DI::affiliates,
    DI::lower_ltv_intents' = DI::lower_ltv_intents, DI::next_ltv_intent_id' = DI::next_ltv_intent_id,
    DI::deposit_ltv_intent' = DI::deposit_ltv_intent, DI::pending_fee_change' = DI::pending_fee_change,
    DI::pending_config_change' = DI::pending_config_change, DI::events' = DI::events,
  }
}
