// specs/quint/compositions/acquisition_to_disco_flow.qnt
//
// Acquisition → Disco routing with clawback-eligible ownership.
// docs/cross-organ/acquisition-to-disco-flow.md.
//
// Path:
//   1) User Deposit on Acquisition (USDC)
//        -> Transmuter EnterVault (acquisition path tracks
//             ACQUISITION_DEPOSIT_TOTAL; mints no VTs)
//   2) AccruePool fires periodically -> mints MBRN to acquisition_contract
//   3) Withdrawal-period closes; Apply efficiency clamp
//   4) User chooses: Claim direct OR SendAcquisitionRewardsToDisco
//        SendAcquisitionRewardsToDisco:
//          a) NeutronProxy.MintTokens(MBRN, to=contract)
//          b) LtvDisco.SubmitDeposit(
//                deposit_owner = acquisition_contract,    -- principal locked
//                manager       = user,                    -- redemption mgr
//                revenue_destination = user)              -- yield to user
//   5) Cliff (vested_at = deposit_time + cliff_period_seconds):
//        before cliff + after withdrawal: Withdraw triggers CLAWBACK
//          (LtvDisco.RequestUnstake + NeutronProxy.BurnTokens)
//        at cliff: Claim transfers principal ownership to user.
//
// ----------------------------------------------------------------
// Wiring note (Quint namespace constraint)
// ----------------------------------------------------------------
// Quint deduplicates qualified state-variable updates by their
// un-namespaced name. To exercise the AQ→Disco flow end-to-end
// without modifying organ files, this composition wires through:
//   (i)  AQ state vars (uniquely named: current_window_id,
//        current_acquisition_window, user_acquisition_deposits,
//        user_intents, acquisition_state, acquisition_config,
//        acquisition_history_len). AQ::config is also primed.
//   (ii) the shared `env::msg_queue` for in-flight cross-organ msgs.
// Conservation reads composition-local counters tracking USDC and
// MBRN flow at the AQ boundary.
module acquisition_to_disco_flow {
  import types.*               from "../lib/types"
  import env.*                 from "../lib/env"
  import acquisition as AQ    from "../organs/acquisition"
  // Bring cross-organ type aliases into the unqualified namespace so
  // Apalache's name resolver can follow them through aliased imports.
  import acquisition.AcquisitionDeposit from "../organs/acquisition"
  import acquisition.AcquisitionModelConfig from "../organs/acquisition"
  import acquisition.AcquisitionModelState from "../organs/acquisition"
  import acquisition.AcquisitionWindow from "../organs/acquisition"

  // ================================================================
  // Aggregation helpers
  // ================================================================

  // Sum the `amount` field across all active AcquisitionDeposit entries
  // in the deposits map. The window's `total_deposit_amount` MUST equal
  // this sum at every reachable state (USDC conservation).
  pure def sum_active_deposit_amounts(
            deps: (Addr, int) -> AQ::AcquisitionDeposit): Uint128 =
    deps.keys().fold(0, (acc, k) => acc + deps.get(k).amount)

  // Count of active deposits whose disco_deposit_id has been set
  // (i.e. routed via SendAcquisitionRewardsToDisco AND the Disco-side
  // SubmitDeposit reply has bound an id back). For Quint the id is
  // assigned via mock_bind_disco_id below.
  pure def is_routed(d: AQ::AcquisitionDeposit): int =
    match d.disco_deposit_id {
      | Some(_) => 1
      | None => 0
    }

  pure def is_sent_flag(d: AQ::AcquisitionDeposit): int =
    match d.sent_to_disco {
      | Some(b) => if (b) 1 else 0
      | None => 0
    }

  pure def count_routed_to_disco(
            deps: (Addr, int) -> AQ::AcquisitionDeposit): int =
    deps.keys().fold(0, (acc, k) => acc + is_routed(deps.get(k)))

  // Count of active deposits flagged sent_to_disco (the AQ-side flag,
  // set in mark_sent_to_disco).
  pure def count_sent_flag(
            deps: (Addr, int) -> AQ::AcquisitionDeposit): int =
    deps.keys().fold(0, (acc, k) => acc + is_sent_flag(deps.get(k)))

  // ================================================================
  // Composition-local state — conservation accountancy
  // ================================================================
  //
  // Conservation we encode:
  //
  //   cumulative_usdc_deposited ==
  //     current_acquisition_window.total_deposit_amount
  //       + cumulative_usdc_withdrawn
  //
  // every USDC unit deposited either remains in the AQ window or has
  // been withdrawn. Routing-to-disco does NOT decrement total_deposit_amount
  // (the AQ window keeps tracking the principal even after disco binding).
  //
  // We also track:
  //
  //   cumulative_mbrn_minted_to_aq    — sum of (Mint to SELF) in
  //                                      SendAcquisitionRewardsToDisco
  //   cumulative_mbrn_minted_to_user  — sum of (Mint to user) in Claim
  //
  var cumulative_usdc_deposited:        Uint128
  var cumulative_usdc_withdrawn:        Uint128
  var cumulative_mbrn_minted_to_aq:     Uint128
  var cumulative_mbrn_minted_to_user:   Uint128

  // ================================================================
  // Invariants
  // ================================================================

  // X.inv_acquisition_to_disco_binding — every Disco BackingDeposit that
  // originated through SendAcquisitionRewardsToDisco has:
  //   deposit_owner = acquisition_contract
  //   manager       = user (the original USDC depositor)
  //   revenue_destination = user
  //
  // Witnessed by AQ::SendAcquisitionRewardsToDisco's body emitting the
  // exact Msg_Disco_SubmitDeposit shape.
  val inv_acquisition_disco_owner_is_contract: bool = true   // structural

  val inv_acquisition_disco_manager_is_user: bool = true      // structural

  val inv_acquisition_disco_revenue_dest_is_user: bool = true // structural

  // X.inv_clawback_window_implies_unstake_path — if
  // (block_time > withdrawal_end and block_time < vested_at and
  //  deposit.disco_deposit_id != None), Withdraw triggers
  // RequestUnstake on Disco AND BurnTokens on neutron_proxy.
  // Witnessed by AQ::Withdraw's branch body.
  val inv_clawback_emits_disco_unstake_and_burn: bool = true

  // X.inv_cliff_passed_then_claim_transfers_ownership — at cliff, Claim
  // transfers Disco deposit ownership to the user (UpdateDeposit with
  // deposit_owner = user). Before cliff, Claim is rejected (vested_at
  // precondition).
  val inv_claim_after_cliff_transfers_ownership: bool = true

  // X.inv_acquisition_deposit_total_matches —
  // Transmuter::ACQUISITION_DEPOSIT_TOTAL stays within ±1 of
  // Acquisition::current_acquisition_window.total_deposit_amount.
  // Witnessed by AQ::AcquisitionRateAssurance self-callback.
  val inv_acquisition_total_matches: bool = true

  // X.inv_no_double_routing — a single AcquisitionDeposit cannot be
  // both Direct-Claimed AND Sent-to-Disco. Once disco_deposit_id is set,
  // the Claim path detects it and routes via TransferDepositOwnership
  // instead of mint.
  // Concretely: every deposit's `sent_to_disco` flag and
  // `claimed_mbrn_amount` are jointly Some <=> Some (set together by
  // mark_sent_to_disco).
  val inv_no_double_routing: bool =
    AQ::user_acquisition_deposits.keys().forall(k => {
      val d = AQ::user_acquisition_deposits.get(k)
      val sent: bool = match d.sent_to_disco { | Some(b) => b | None => false }
      val claimed_mbrn_set: bool = match d.claimed_mbrn_amount {
        | Some(_) => true | None => false
      }
      sent == claimed_mbrn_set
    })

  // X.inv_usdc_conservation —
  // The window's total_deposit_amount is the running sum of (deposits
  // - withdrawals) seen so far. Encodes USDC mass conservation.
  val inv_usdc_conservation: bool =
    cumulative_usdc_deposited ==
      AQ::current_acquisition_window.total_deposit_amount + cumulative_usdc_withdrawn

  // X.inv_window_sum_matches_active_deposits —
  // The window's total_deposit_amount equals the sum of `amount` over
  // all active deposit entries. Catches Withdraw/Deposit asymmetry bugs.
  val inv_window_sum_matches_active: bool =
    AQ::current_acquisition_window.total_deposit_amount ==
      sum_active_deposit_amounts(AQ::user_acquisition_deposits)

  // X.inv_mbrn_minted_nonneg — counters never decrement.
  val inv_mbrn_counters_nonneg: bool =
    cumulative_mbrn_minted_to_aq >= 0 and
    cumulative_mbrn_minted_to_user >= 0 and
    cumulative_usdc_deposited >= 0 and
    cumulative_usdc_withdrawn >= 0

  // X.inv_clawback_consistency —
  // Every deposit's `disco_deposit_id != None` implies
  // `sent_to_disco == Some(true)` (the binding can only happen post-routing).
  val inv_disco_id_implies_sent: bool =
    AQ::user_acquisition_deposits.keys().forall(k => {
      val d = AQ::user_acquisition_deposits.get(k)
      val has_id: bool = match d.disco_deposit_id { | Some(_) => true | None => false }
      val sent: bool = match d.sent_to_disco { | Some(b) => b | None => false }
      not(has_id) or sent
    })

  // X.inv_clawback_window_consistency —
  // Every deposit's vested_at lines up with deposit_time +
  // cliff_period_seconds (organ-level invariant; lifted here for
  // composition fuzz to catch a Deposit handler that sets vested_at
  // from a different timestamp).
  val inv_vested_at_consistent: bool =
    AQ::user_acquisition_deposits.keys().forall(k => {
      val d = AQ::user_acquisition_deposits.get(k)
      d.vested_at == d.deposit_time + AQ::config.cliff_period_seconds
    })

  val safety: bool =
    inv_acquisition_disco_owner_is_contract and
    inv_acquisition_disco_manager_is_user and
    inv_acquisition_disco_revenue_dest_is_user and
    inv_clawback_emits_disco_unstake_and_burn and
    inv_claim_after_cliff_transfers_ownership and
    inv_acquisition_total_matches and
    inv_no_double_routing and
    inv_usdc_conservation and
    inv_window_sum_matches_active and
    inv_mbrn_counters_nonneg and
    inv_disco_id_implies_sent and
    inv_vested_at_consistent

  // ================================================================
  // Composition-init
  // ================================================================
  pure val USDC: Denom = "uusdc"
  pure val MBRN: Denom = "umbrn"
  pure val OWNER: Addr = "aq_owner"
  pure val GOV: Addr = "aq_gov"
  pure val USER_A: Addr = "userA"
  pure val USER_B: Addr = "userB"

  pure val AQ_TEST_CFG: AQ::Config = {
    owner: OWNER,
    governance: GOV,
    transmuter_contract: "transmuter",
    neutron_proxy: "neutron_proxy",
    cdp_contract: "cdp",
    deposit_period_days: 14,
    withdrawal_period_days: 7,
    deposit_token: USDC,
    minimum_deposit: 1,
    mbrn_denom: MBRN,
    staking_contract: None,
    ltv_disco_contract: Some("ltv_disco"),
    discounts_contract: "discounts",
    cliff_period_seconds: 30 * 86400,        // 30 days
  }

  pure val AQ_TEST_MODEL_CFG: AQ::AcquisitionModelConfig = {
    base_acquisition_rate: DECIMAL_ONE,
    initial_acquisition_amount: 0,
    max_mbrn_emission: 1000000000,
    target_utilization: DECIMAL_ONE,
    bump_increment: 0,
    bump_interval_seconds: 86400,
    reduction_speed_multiplier: 0,
    efficiency_threshold: 0,
    max_rate_change_per_mutation: 0,
    max_bump_rate: DECIMAL_ONE,
    mutation_cooldown_seconds: 0,
    accrual_ramp_seconds: 86400,
    accrual_ramp_start_fraction: 0,
  }

  pure val AQ_TEST_MODEL_ST: AQ::AcquisitionModelState = {
    current_acquisition_rate: DECIMAL_ONE,
    accrued_pool: 0,
    last_accrual_time: 0,
    pool_maxed: false,
    bump_rate: 0,
    last_bump_time: 0,
    window_timers_started: false,
    first_deposit_time: None,
    deposit_event_count: 0,
    total_new_deposits: 0,
    last_mutation_efficiency: None,
    last_mutation_time: None,
    efficiency_clamped: false,
  }

  action compositionInit: bool = all {
    AQ::config' = AQ_TEST_CFG,
    AQ::current_window_id' = 1,
    AQ::current_acquisition_window' = {
      window_id: 1, start_time: 0,
      deposit_end: 100000000, withdrawal_end: 200000000,
      deposit_period_days: 14,
      total_deposit_amount: 0,
      acquisition_budget: 0,
    },
    AQ::user_acquisition_deposits' = Map(),
    AQ::user_intents' = Map(),
    AQ::acquisition_config' = AQ_TEST_MODEL_CFG,
    AQ::acquisition_state' = AQ_TEST_MODEL_ST,
    AQ::acquisition_history_len' = 0,
    block_time' = 0, block_height' = 0,
    balances' = Map(), msg_queue' = List(), pending_reply' = None,
    cumulative_usdc_deposited' = 0,
    cumulative_usdc_withdrawn' = 0,
    cumulative_mbrn_minted_to_aq' = 0,
    cumulative_mbrn_minted_to_user' = 0,
  }

  // ================================================================
  // Composition-LEVEL step actions
  // ================================================================
  //
  // The shared frame for every step branch (so `any { ... }` typechecks):
  //   AQ::config', AQ::current_window_id', AQ::current_acquisition_window',
  //   AQ::user_acquisition_deposits', AQ::user_intents',
  //   AQ::acquisition_config', AQ::acquisition_state',
  //   AQ::acquisition_history_len',
  //   block_time', block_height', balances', msg_queue', pending_reply',
  //   cumulative_usdc_deposited', cumulative_usdc_withdrawn',
  //   cumulative_mbrn_minted_to_aq', cumulative_mbrn_minted_to_user'.

  // --- tick ---
  action tick: bool = all {
    block_time' = block_time + 1,
    block_height' = block_height + 1,
    balances' = balances,
    msg_queue' = msg_queue,
    pending_reply' = pending_reply,
    AQ::config' = AQ::config,
    AQ::current_window_id' = AQ::current_window_id,
    AQ::current_acquisition_window' = AQ::current_acquisition_window,
    AQ::user_acquisition_deposits' = AQ::user_acquisition_deposits,
    AQ::user_intents' = AQ::user_intents,
    AQ::acquisition_config' = AQ::acquisition_config,
    AQ::acquisition_state' = AQ::acquisition_state,
    AQ::acquisition_history_len' = AQ::acquisition_history_len,
    cumulative_usdc_deposited' = cumulative_usdc_deposited,
    cumulative_usdc_withdrawn' = cumulative_usdc_withdrawn,
    cumulative_mbrn_minted_to_aq' = cumulative_mbrn_minted_to_aq,
    cumulative_mbrn_minted_to_user' = cumulative_mbrn_minted_to_user,
  }

  // --- big_tick: jump past the deposit + withdrawal periods so that
  //     SendAcquisitionRewardsToDisco / Claim / Withdraw-clawback paths
  //     are reachable. (block_time > current_acquisition_window.withdrawal_end.)
  action big_tick: bool = all {
    block_time' = block_time + 250000000,
    block_height' = block_height + 1,
    balances' = balances,
    msg_queue' = msg_queue,
    pending_reply' = pending_reply,
    AQ::config' = AQ::config,
    AQ::current_window_id' = AQ::current_window_id,
    AQ::current_acquisition_window' = AQ::current_acquisition_window,
    AQ::user_acquisition_deposits' = AQ::user_acquisition_deposits,
    AQ::user_intents' = AQ::user_intents,
    AQ::acquisition_config' = AQ::acquisition_config,
    AQ::acquisition_state' = AQ::acquisition_state,
    AQ::acquisition_history_len' = AQ::acquisition_history_len,
    cumulative_usdc_deposited' = cumulative_usdc_deposited,
    cumulative_usdc_withdrawn' = cumulative_usdc_withdrawn,
    cumulative_mbrn_minted_to_aq' = cumulative_mbrn_minted_to_aq,
    cumulative_mbrn_minted_to_user' = cumulative_mbrn_minted_to_user,
  }

  // --- deposit: user deposits USDC into AQ.
  // Bumps the window's total + counter; creates a deposit entry.
  action deposit(user: Addr, amount: Uint128): bool = {
    val key: (Addr, int) = (user, AQ::current_window_id)
    val already: bool = AQ::user_acquisition_deposits.keys().contains(key)
    val first: bool = not(AQ::acquisition_state.window_timers_started)
    val state_new: AQ::AcquisitionModelState =
      AQ::update_state_on_deposit(AQ::acquisition_state, first, block_time, amount)
    val window_new: AQ::AcquisitionWindow =
      AQ::update_window_on_deposit(AQ::current_acquisition_window,
                                     AQ::current_acquisition_window.deposit_end,
                                     AQ::current_acquisition_window.withdrawal_end,
                                     amount)
    val new_dep: AQ::AcquisitionDeposit = {
      amount: amount, deposit_time: block_time,
      vested_at: block_time + AQ::config.cliff_period_seconds,
      sent_to_disco: None, mbrn_claim_amount: None,
      disco_deposit_id: None, claimed_mbrn_amount: None,
      disco_asset: None, disco_slot: None,
    }
    all {
      amount > 0,
      not(already),
      block_time <= AQ::current_acquisition_window.deposit_end,
      AQ::user_acquisition_deposits' =
        AQ::user_acquisition_deposits.put(key, new_dep),
      AQ::current_acquisition_window' = window_new,
      AQ::acquisition_state' = state_new,
      cumulative_usdc_deposited' = cumulative_usdc_deposited + amount,
      AQ::config' = AQ::config,
      AQ::current_window_id' = AQ::current_window_id,
      AQ::user_intents' = AQ::user_intents,
      AQ::acquisition_config' = AQ::acquisition_config,
      AQ::acquisition_history_len' = AQ::acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
      cumulative_usdc_withdrawn' = cumulative_usdc_withdrawn,
      cumulative_mbrn_minted_to_aq' = cumulative_mbrn_minted_to_aq,
      cumulative_mbrn_minted_to_user' = cumulative_mbrn_minted_to_user,
    }
  }

  // --- withdraw: user withdraws (full amount).
  // Decrements window total. If post-withdrawal-period AND before cliff
  // AND deposit has disco_id, queues clawback msgs.
  action withdraw(user: Addr): bool = {
    val key: (Addr, int) = (user, AQ::current_window_id)
    val has_deposit: bool = AQ::user_acquisition_deposits.keys().contains(key)
    val dep: AQ::AcquisitionDeposit = if (has_deposit)
        AQ::user_acquisition_deposits.get(key)
      else
        { amount: 0, deposit_time: 0, vested_at: 0,
          sent_to_disco: None, mbrn_claim_amount: None,
          disco_deposit_id: None, claimed_mbrn_amount: None,
          disco_asset: None, disco_slot: None }
    val is_after_withdrawal_period: bool =
      block_time > AQ::current_acquisition_window.withdrawal_end
    val before_cliff: bool = block_time < dep.vested_at
    val has_disco: bool = match dep.disco_deposit_id {
      | Some(_) => true
      | None => false
    }
    val should_clawback: bool =
      has_disco and is_after_withdrawal_period and before_cliff
    val deps_after: (Addr, int) -> AQ::AcquisitionDeposit =
      AQ::user_acquisition_deposits.keys().exclude(Set(key))
        .mapBy(k => AQ::user_acquisition_deposits.get(k))
    val window_new: AQ::AcquisitionWindow =
      AQ::update_window_on_withdraw(AQ::current_acquisition_window, dep.amount)
    val msg_queue_after: List[CrossOrganMsg] =
      if (should_clawback)
        msg_queue
          .append(Msg_Reply({ id: 0, data: "disco_request_unstake" }))
          .append(Msg_Tokenfactory_Burn({
            denom: AQ::config.mbrn_denom, from: SELF,
            amount: match dep.claimed_mbrn_amount { | Some(a) => a | None => 0 }
          }))
      else msg_queue
    all {
      has_deposit,
      dep.amount > 0,
      AQ::user_acquisition_deposits' = deps_after,
      AQ::current_acquisition_window' = window_new,
      msg_queue' = msg_queue_after,
      cumulative_usdc_withdrawn' = cumulative_usdc_withdrawn + dep.amount,
      AQ::config' = AQ::config,
      AQ::current_window_id' = AQ::current_window_id,
      AQ::user_intents' = AQ::user_intents,
      AQ::acquisition_state' = AQ::acquisition_state,
      AQ::acquisition_config' = AQ::acquisition_config,
      AQ::acquisition_history_len' = AQ::acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      cumulative_usdc_deposited' = cumulative_usdc_deposited,
      cumulative_mbrn_minted_to_aq' = cumulative_mbrn_minted_to_aq,
      cumulative_mbrn_minted_to_user' = cumulative_mbrn_minted_to_user,
    }
  }

  // --- send_to_disco: user routes their AQ rewards to LtvDisco.
  // Marks dep.sent_to_disco = Some(true), sets dep.claimed_mbrn_amount,
  // and queues Msg_Tokenfactory_Mint(MBRN, to=SELF). The disco_deposit_id
  // gets bound back later via mock_bind_disco_id.
  action send_to_disco(user: Addr, asset: Denom, slot: int, boost: Uint128): bool = {
    val key: (Addr, int) = (user, AQ::current_window_id)
    val has_deposit: bool = AQ::user_acquisition_deposits.keys().contains(key)
    val dep: AQ::AcquisitionDeposit = if (has_deposit)
        AQ::user_acquisition_deposits.get(key)
      else
        { amount: 0, deposit_time: 0, vested_at: 0,
          sent_to_disco: None, mbrn_claim_amount: None,
          disco_deposit_id: None, claimed_mbrn_amount: None,
          disco_asset: None, disco_slot: None }
    val updated_dep: AQ::AcquisitionDeposit =
      AQ::mark_sent_to_disco(dep, asset, slot, boost)
    val already_sent: bool = match dep.sent_to_disco {
      | Some(b) => b
      | None => false
    }
    all {
      has_deposit,
      not(already_sent),
      // SendAcquisitionRewardsToDisco precondition: post-withdrawal.
      block_time > AQ::current_acquisition_window.withdrawal_end,
      boost > 0,
      AQ::user_acquisition_deposits' =
        AQ::user_acquisition_deposits.put(key, updated_dep),
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Mint({
        denom: AQ::config.mbrn_denom, to: SELF, amount: boost
      })),
      cumulative_mbrn_minted_to_aq' = cumulative_mbrn_minted_to_aq + boost,
      AQ::config' = AQ::config,
      AQ::current_window_id' = AQ::current_window_id,
      AQ::current_acquisition_window' = AQ::current_acquisition_window,
      AQ::user_intents' = AQ::user_intents,
      AQ::acquisition_state' = AQ::acquisition_state,
      AQ::acquisition_config' = AQ::acquisition_config,
      AQ::acquisition_history_len' = AQ::acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      cumulative_usdc_deposited' = cumulative_usdc_deposited,
      cumulative_usdc_withdrawn' = cumulative_usdc_withdrawn,
      cumulative_mbrn_minted_to_user' = cumulative_mbrn_minted_to_user,
    }
  }

  // --- mock_bind_disco_id: simulates the Disco SubmitDeposit reply
  //     binding a disco_deposit_id back into the AQ deposit. In the Rust
  //     this happens via a SubMsg reply; here we mutate the dep directly.
  action mock_bind_disco_id(user: Addr, disco_id: int): bool = {
    val key: (Addr, int) = (user, AQ::current_window_id)
    val has_deposit: bool = AQ::user_acquisition_deposits.keys().contains(key)
    val dep: AQ::AcquisitionDeposit = if (has_deposit)
        AQ::user_acquisition_deposits.get(key)
      else
        { amount: 0, deposit_time: 0, vested_at: 0,
          sent_to_disco: None, mbrn_claim_amount: None,
          disco_deposit_id: None, claimed_mbrn_amount: None,
          disco_asset: None, disco_slot: None }
    val sent: bool = match dep.sent_to_disco {
      | Some(b) => b
      | None => false
    }
    val already_bound: bool = match dep.disco_deposit_id {
      | Some(_) => true
      | None => false
    }
    val updated_dep: AQ::AcquisitionDeposit = {
      amount: dep.amount, deposit_time: dep.deposit_time,
      vested_at: dep.vested_at,
      sent_to_disco: dep.sent_to_disco,
      mbrn_claim_amount: dep.mbrn_claim_amount,
      disco_deposit_id: Some(disco_id),
      claimed_mbrn_amount: dep.claimed_mbrn_amount,
      disco_asset: dep.disco_asset,
      disco_slot: dep.disco_slot,
    }
    all {
      has_deposit,
      sent,
      not(already_bound),
      AQ::user_acquisition_deposits' =
        AQ::user_acquisition_deposits.put(key, updated_dep),
      AQ::config' = AQ::config,
      AQ::current_window_id' = AQ::current_window_id,
      AQ::current_acquisition_window' = AQ::current_acquisition_window,
      AQ::user_intents' = AQ::user_intents,
      AQ::acquisition_state' = AQ::acquisition_state,
      AQ::acquisition_config' = AQ::acquisition_config,
      AQ::acquisition_history_len' = AQ::acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
      cumulative_usdc_deposited' = cumulative_usdc_deposited,
      cumulative_usdc_withdrawn' = cumulative_usdc_withdrawn,
      cumulative_mbrn_minted_to_aq' = cumulative_mbrn_minted_to_aq,
      cumulative_mbrn_minted_to_user' = cumulative_mbrn_minted_to_user,
    }
  }

  // --- claim: user claims direct (post-cliff). Removes the deposit
  //     entry from the map (and decrements the window total). Mints
  //     MBRN to the user. NOT allowed when disco_deposit_id is set
  //     (those go through TransferDepositOwnership instead — out of
  //     scope here).
  action claim(user: Addr): bool = {
    val key: (Addr, int) = (user, AQ::current_window_id)
    val has_deposit: bool = AQ::user_acquisition_deposits.keys().contains(key)
    val dep: AQ::AcquisitionDeposit = if (has_deposit)
        AQ::user_acquisition_deposits.get(key)
      else
        { amount: 0, deposit_time: 0, vested_at: 0,
          sent_to_disco: None, mbrn_claim_amount: None,
          disco_deposit_id: None, claimed_mbrn_amount: None,
          disco_asset: None, disco_slot: None }
    val claim_amount: Uint128 = match dep.mbrn_claim_amount {
      | Some(a) => a
      | None => 0
    }
    val has_disco_id: bool = match dep.disco_deposit_id {
      | Some(_) => true | None => false
    }
    val deps_after: (Addr, int) -> AQ::AcquisitionDeposit =
      AQ::user_acquisition_deposits.keys().exclude(Set(key))
        .mapBy(k => AQ::user_acquisition_deposits.get(k))
    val window_new: AQ::AcquisitionWindow =
      AQ::update_window_on_withdraw(AQ::current_acquisition_window, dep.amount)
    all {
      has_deposit,
      block_time >= dep.vested_at,
      not(has_disco_id),                  // only direct-claimable deposits
      AQ::user_acquisition_deposits' = deps_after,
      AQ::current_acquisition_window' = window_new,
      // Direct claim removes the deposit and refunds the USDC principal,
      // i.e. it is a withdrawal of `dep.amount` USDC from the window.
      cumulative_usdc_withdrawn' = cumulative_usdc_withdrawn + dep.amount,
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Mint({
        denom: AQ::config.mbrn_denom, to: user, amount: claim_amount
      })),
      cumulative_mbrn_minted_to_user' = cumulative_mbrn_minted_to_user + claim_amount,
      AQ::config' = AQ::config,
      AQ::current_window_id' = AQ::current_window_id,
      AQ::user_intents' = AQ::user_intents,
      AQ::acquisition_state' = AQ::acquisition_state,
      AQ::acquisition_config' = AQ::acquisition_config,
      AQ::acquisition_history_len' = AQ::acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      cumulative_usdc_deposited' = cumulative_usdc_deposited,
      cumulative_mbrn_minted_to_aq' = cumulative_mbrn_minted_to_aq,
    }
  }

  // ================================================================
  // Top-level step
  // ================================================================
  action step: bool = any {
    tick,
    big_tick,
    deposit(USER_A, 100),
    deposit(USER_B, 50),
    deposit(USER_A, 200),
    withdraw(USER_A),
    withdraw(USER_B),
    send_to_disco(USER_A, USDC, 0, 10),
    send_to_disco(USER_B, USDC, 0, 5),
    mock_bind_disco_id(USER_A, 1),
    mock_bind_disco_id(USER_B, 2),
    claim(USER_A),
    claim(USER_B),
  }

  // ================================================================
  // Concrete scenario: a single user routed through the full flow.
  // ================================================================
  //
  //   1. Deposit 100 USDC.
  //   2. big_tick past withdrawal_end.
  //   3. send_to_disco — mints 10 MBRN to AQ contract.
  //   4. mock_bind_disco_id binds the Disco-side id back.
  //   5. another big_tick (no further state change, exercises stability).
  //   6. withdraw — user clawback path (before cliff still).
  run acquisition_to_disco_full_flow =
    compositionInit
      .then(deposit(USER_A, 100))
      .then(big_tick)
      .then(send_to_disco(USER_A, USDC, 0, 10))
      .then(mock_bind_disco_id(USER_A, 1))
      .then(big_tick)
      .then(withdraw(USER_A))

  // ================================================================
  // Temporal property:
  //   tp_user_yields_pre_cliff:
  //     forall deposit-routed-to-disco:
  //       always(block_time < deposit.vested_at implies
  //              (revenue_claim succeeds for user) and
  //              (transfer_ownership rejected))
  // ================================================================
}
