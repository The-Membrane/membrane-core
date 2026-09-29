// specs/quint/organs/staking.qnt
//
// MBRN staking — fee receipt + reward accrual + unbonding queue +
// delegation + lock duration + buyback-and-burn + governance vote-power
// export.
//
// Source of truth (Rust):
//   contracts/staking/src/state.rs              (12 storage items)
//   contracts/staking/src/contract.rs           (handlers; line cites below)
//   contracts/staking/src/error.rs              (10 error variants)
//   packages/membrane/src/staking.rs            (Config, ExecuteMsg, types)
//   packages/membrane/src/types.rs:227-342      (StakeDeposit, Locked, Delegation, FeeEvent)
//
// Solidity-port note: `FeeEvent.fee.amount` stores per-MBRN-staked rate,
// not total. Solidity port should use the same trick to avoid an O(n) loop
// over stakers on every DepositFee.
module staking {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Constants
  // ================================================================
  pure val DEFAULT_INCENTIVE_RATE:        Decimal = 90000000000000000
  pure val DEFAULT_INCENTIVE_DURATION_S:  int = 20736000
  pure val DEFAULT_LOCK_DURATION_CEILING_S: int = 31536000
  pure val DEFAULT_UNSTAKING_PERIOD_S:    int = 345600
  pure val DEFAULT_MAX_COMMISSION_RATE:   Decimal = 100000000000000000
  pure val DEFAULT_VESTING_REV_MULT:      Decimal = 200000000000000000
  pure val MIN_STAKE:                     Uint128 = 1000000

  // ================================================================
  // Types
  // ================================================================
  type Locked = {
    locked_until: Timestamp,
    perpetual_lock_days: Option[int],
    intended_lock_days: Option[int],
  }

  type StakeDeposit = {
    staker: Addr,
    amount: Uint128,
    stake_time: Timestamp,
    unstake_start_time: Option[Timestamp],
    locked: Option[Locked],
    last_accrued: Option[Timestamp],
  }

  type FeeEvent = {
    time_of_event: Timestamp,
    fee_denom: Denom,
    amount_per_mbrn_staked: Decimal,
  }

  type Delegation = {
    delegate: Addr,
    amount: Uint128,
    fluidity: bool,
    voting_power_delegation: bool,
    time_of_delegation: Timestamp,
    last_accrued: Option[Timestamp],
  }

  type DelegationInfo = {
    delegated: List[Delegation],
    delegated_to: List[Delegation],
    commission: Decimal,
  }

  type Totals = {
    stakers: Uint128,
    vesting_contract: Uint128,
  }

  type StakeDistributionLog = {
    rate: Decimal,
    duration_s: int,
    start_time: Timestamp,
  }

  type Config = {
    owner: Addr,
    governance: Addr,
    mbrn_denom: Denom,
    incentive_schedule: StakeDistributionLog,
    lock_duration_ceiling_s: int,
    unstaking_period_s: int,
    max_commission_rate: Decimal,
    keep_raw_cdt: bool,
    vesting_rev_multiplier: Decimal,
    positions_contract: Option[Addr],
    auction_contract: Option[Addr],
    vesting_contract: Option[Addr],
    governance_contract: Option[Addr],
    osmosis_proxy: Option[Addr],
    emissions_voting_contract: Option[Addr],
  }

  // ================================================================
  // State
  // ================================================================
  var config:                  Config
  var staking_totals:          Totals
  var staked:                  Addr -> List[StakeDeposit]
  var delegations:             Addr -> DelegationInfo
  var delegate_claims:         Addr -> (List[(Denom, Uint128)], Uint128)
  var fee_events:              List[FeeEvent]
  var incentive_scheduling:    StakeDistributionLog
  var buyback_and_burn:        bool
  var ownership_transfer:      Option[Addr]

  action init(cfg: Config, sched: StakeDistributionLog): bool = all {
    config' = cfg,
    staking_totals' = { stakers: 0, vesting_contract: 0 },
    staked' = Map(),
    delegations' = Map(),
    delegate_claims' = Map(),
    fee_events' = List(),
    incentive_scheduling' = sched,
    buyback_and_burn' = false,
    ownership_transfer' = None,
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure def total_user_stake(s: Addr -> List[StakeDeposit], u: Addr): Uint128 =
    if (s.keys().contains(u))
      s.get(u).foldl(0, (acc, d) => acc + d.amount)
    else 0

  pure def deposit_withdrawable(d: StakeDeposit, now: Timestamp,
                                 unstaking_period_s: int): bool =
    match d.unstake_start_time {
      | Some(t) => now >= t + unstaking_period_s
      | None => false
    }

  pure def event_after_accrual(d: StakeDeposit, e: FeeEvent): bool =
    match d.last_accrued {
      | Some(t) => e.time_of_event > t
      | None => true
    }

  pure def deposit_claimable(d: StakeDeposit,
                              events: List[FeeEvent],
                              now: Timestamp): Uint128 =
    events.foldl(0, (acc, e) =>
      if (event_after_accrual(d, e) and e.time_of_event <= now)
        acc + decimal_mul_uint(e.amount_per_mbrn_staked, d.amount)
      else acc)

  pure def mark_unstaking(d: StakeDeposit, now: Timestamp): StakeDeposit =
    match d.unstake_start_time {
      | None => { staker: d.staker, amount: d.amount, stake_time: d.stake_time,
                 unstake_start_time: Some(now), locked: d.locked,
                 last_accrued: d.last_accrued }
      | Some(_) => d
    }

  pure def clear_unstaking(d: StakeDeposit): StakeDeposit =
    match d.unstake_start_time {
      | Some(_) => { staker: d.staker, amount: d.amount, stake_time: d.stake_time,
                    unstake_start_time: None, locked: d.locked,
                    last_accrued: d.last_accrued }
      | None => d
    }

  pure def update_last_accrued(d: StakeDeposit, now: Timestamp): StakeDeposit = {
    staker: d.staker, amount: d.amount, stake_time: d.stake_time,
    unstake_start_time: d.unstake_start_time, locked: d.locked,
    last_accrued: Some(now),
  }

  pure def lock_within_ceiling(o: Option[Locked], stake_time: Timestamp,
                                ceiling: int): bool =
    match o {
      | Some(l) => l.locked_until <= stake_time + ceiling
      | None => true
    }

  pure val DEFAULT_DELEGATION_INFO: DelegationInfo = {
    delegated: List(),
    delegated_to: List(),
    commission: 0,
  }

  pure def get_or_default_di(m: Addr -> DelegationInfo,
                              k: Addr): DelegationInfo =
    if (m.keys().contains(k)) m.get(k) else DEFAULT_DELEGATION_INFO

  pure def add_delegation(di: DelegationInfo, d: Delegation): DelegationInfo = {
    delegated: di.delegated.append(d),
    delegated_to: di.delegated_to,
    commission: di.commission,
  }

  pure def add_delegated_to(di: DelegationInfo, d: Delegation): DelegationInfo = {
    delegated: di.delegated,
    delegated_to: di.delegated_to.append(d),
    commission: di.commission,
  }

  // ================================================================
  // Execute actions
  // ================================================================

  // --- Stake (contract.rs:497-549) ---
  action Stake(sender: Addr,
                user: Option[Addr],
                amount: Uint128,
                locked: Option[Locked]): bool = {
    val target_user: Addr = match user {
      | Some(u) => u
      | None => sender
    }
    val cur_list = if (staked.keys().contains(target_user))
                     staked.get(target_user) else List()
    val locked_ok: bool = lock_within_ceiling(locked, block_time,
                                                config.lock_duration_ceiling_s)
    val new_deposit: StakeDeposit = {
      staker: target_user,
      amount: amount,
      stake_time: block_time,
      unstake_start_time: None,
      locked: locked,
      last_accrued: Some(block_time),
    }
    val new_totals: Totals = {
      stakers: staking_totals.stakers + amount,
      vesting_contract: staking_totals.vesting_contract,
    }
    all {
      amount >= MIN_STAKE,
      locked_ok,
      staked' = staked.put(target_user, cur_list.append(new_deposit)),
      staking_totals' = new_totals,
      config' = config,
      delegations' = delegations,
      delegate_claims' = delegate_claims,
      fee_events' = fee_events,
      incentive_scheduling' = incentive_scheduling,
      buyback_and_burn' = buyback_and_burn,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- Unstake (contract.rs:625-871) ---
  action Unstake(sender: Addr,
                  mbrn_amount: Option[Uint128],
                  has_active_governance_lock: bool): bool = {
    val has_stake = staked.keys().contains(sender)
    val list: List[StakeDeposit] = if (has_stake) staked.get(sender) else List()
    val total = if (has_stake) total_user_stake(staked, sender) else 0
    val req_amount: Uint128 = match mbrn_amount {
      | Some(a) => a
      | None => total
    }
    val now = block_time
    val updated_list: List[StakeDeposit] =
      list.foldl(List(), (acc, d) => acc.append(mark_unstaking(d, now)))
    all {
      has_stake,
      not(has_active_governance_lock),
      req_amount > 0,
      req_amount <= total,
      staked' = staked.put(sender, updated_list),
      staking_totals' = staking_totals,
      msg_queue' = msg_queue
        .append(Msg_Tokenfactory_Mint({ denom: config.mbrn_denom, to: sender, amount: 0 })),
      config' = config,
      delegations' = delegations,
      delegate_claims' = delegate_claims,
      fee_events' = fee_events,
      incentive_scheduling' = incentive_scheduling,
      buyback_and_burn' = buyback_and_burn,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Restake (contract.rs:1362-1446) ---
  // Restake clears the `unstake_start_time` flag on deposits but does NOT
  // bump `staking_totals.stakers` — Unstake's phase 1 doesn't decrement it,
  // so Restake mustn't add. Otherwise the totals double-count an
  // unstake-and-restake round-trip.
  action Restake(sender: Addr, mbrn_amount: Uint128): bool = {
    val has_stake = staked.keys().contains(sender)
    val list: List[StakeDeposit] = if (has_stake) staked.get(sender) else List()
    val updated_list: List[StakeDeposit] =
      list.foldl(List(), (acc, d) => acc.append(clear_unstaking(d)))
    all {
      has_stake,
      staked' = staked.put(sender, updated_list),
      staking_totals' = staking_totals,
      config' = config,
      delegations' = delegations,
      delegate_claims' = delegate_claims,
      fee_events' = fee_events,
      incentive_scheduling' = incentive_scheduling,
      buyback_and_burn' = buyback_and_burn,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- ClaimRewards (contract.rs:1451-1565) ---
  action ClaimRewards(sender: Addr,
                       send_to: Option[Addr],
                       restake_interest: bool): bool = {
    val has_stake = staked.keys().contains(sender)
    val list: List[StakeDeposit] = if (has_stake) staked.get(sender) else List()
    val recipient: Addr = match send_to {
      | Some(a) => a
      | None => sender
    }
    val now = block_time
    val updated_list: List[StakeDeposit] =
      list.foldl(List(), (acc, d) => acc.append(update_last_accrued(d, now)))
    all {
      has_stake,
      msg_queue' = msg_queue
        .append(Msg_Tokenfactory_Mint({ denom: config.mbrn_denom, to: recipient, amount: 0 })),
      staked' = staked.put(sender, updated_list),
      config' = config,
      staking_totals' = staking_totals,
      delegations' = delegations,
      delegate_claims' = delegate_claims,
      fee_events' = fee_events,
      incentive_scheduling' = incentive_scheduling,
      buyback_and_burn' = buyback_and_burn,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- DepositFee (contract.rs:1568-1698) ---
  action DepositFee(sender: Addr,
                     fee_denom: Denom,
                     fee_amount: Uint128): bool = {
    val from_positions: bool = match config.positions_contract {
      | Some(a) => sender == a
      | None => false
    }
    val from_auction: bool = match config.auction_contract {
      | Some(a) => sender == a
      | None => false
    }
    val total = staking_totals.stakers +
                decimal_mul_uint(config.vesting_rev_multiplier,
                                  staking_totals.vesting_contract)
    val per_mbrn: Decimal = if (total == 0) 0 else decimal_from_ratio(fee_amount, total)
    val new_event: FeeEvent = {
      time_of_event: block_time,
      fee_denom: fee_denom,
      amount_per_mbrn_staked: per_mbrn,
    }
    all {
      from_positions or from_auction,
      fee_events' = fee_events.append(new_event),
      config' = config,
      staking_totals' = staking_totals,
      staked' = staked,
      delegations' = delegations,
      delegate_claims' = delegate_claims,
      incentive_scheduling' = incentive_scheduling,
      buyback_and_burn' = buyback_and_burn,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- UpdateDelegations (contract.rs:876-1184) ---
  // Concretizes the additive (delegate=true) path. Symmetrically appends a
  // Delegation to:
  //   delegations[sender].delegated_to    → {delegate: delegate_addr, amount}
  //   delegations[delegate_addr].delegated → {delegate: sender,        amount}
  // Commission-only and removal paths are pass-through (out of scope).
  action UpdateDelegations(sender: Addr,
                            delegate_addr: Addr,
                            mbrn_amount: Option[Uint128],
                            delegate_flag: bool,
                            commission: Option[Decimal]): bool = {
    val commission_ok: bool = match commission {
      | Some(c) => c <= config.max_commission_rate
      | None => true
    }
    val amt: Uint128 = match mbrn_amount {
      | Some(a) => a
      | None => 0
    }
    val is_additive: bool = amt >= MIN_STAKE
    val sender_di: DelegationInfo = get_or_default_di(delegations, sender)
    val delegate_di: DelegationInfo = get_or_default_di(delegations, delegate_addr)
    val sender_to_delegate: Delegation = {
      delegate: delegate_addr,
      amount: amt,
      fluidity: false,
      voting_power_delegation: delegate_flag,
      time_of_delegation: block_time,
      last_accrued: Some(block_time),
    }
    val delegate_from_sender: Delegation = {
      delegate: sender,
      amount: amt,
      fluidity: false,
      voting_power_delegation: delegate_flag,
      time_of_delegation: block_time,
      last_accrued: Some(block_time),
    }
    val new_sender_di: DelegationInfo = add_delegated_to(sender_di, sender_to_delegate)
    val new_delegate_di: DelegationInfo = add_delegation(delegate_di, delegate_from_sender)
    val new_delegations: Addr -> DelegationInfo =
      if (is_additive)
        delegations
          .put(sender, new_sender_di)
          .put(delegate_addr, new_delegate_di)
      else delegations
    all {
      delegate_addr != sender,
      commission_ok,
      is_additive,
      staked.keys().contains(sender),
      delegations' = new_delegations,
      config' = config,
      staking_totals' = staking_totals,
      staked' = staked,
      delegate_claims' = delegate_claims,
      fee_events' = fee_events,
      incentive_scheduling' = incentive_scheduling,
      buyback_and_burn' = buyback_and_burn,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- DeclareDelegate (contract.rs:334-390) ---
  // Rust mutates DELEGATE_INFO: a list of (delegate, alias, discord, twitter,
  // url) records keyed by delegate address — purely a registry for off-chain
  // discovery. This Quint module does NOT track DELEGATE_INFO (the
  // delegations Map captures who delegated TO whom, which is a separate
  // concern). The action's only meaningful precondition is that the sender
  // matches the declared delegate; identity-on-state below is therefore
  // spec-correct relative to the modeled state.
  action DeclareDelegate(sender: Addr, delegate_addr: Addr, remove: bool): bool = all {
    sender == delegate_addr,
    delegations' = delegations,
    config' = config, staking_totals' = staking_totals, staked' = staked,
    delegate_claims' = delegate_claims, fee_events' = fee_events,
    incentive_scheduling' = incentive_scheduling, buyback_and_burn' = buyback_and_burn,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- Lock (contract.rs:2515-2588) ---
  // Apply `locked` to the user's currently-unlocked deposits. We
  // approximate the per-deposit fill-and-split behavior of the Rust impl by
  // stamping `locked` onto every currently-unlocked deposit (the model
  // doesn't split deposits — the locked-amount aggregate is captured by the
  // precondition that `amount` does not exceed the user's unlocked stake).
  action Lock(sender: Addr, locked: Locked, amount: Uint128): bool = {
    val perpetual_ok: bool = match locked.perpetual_lock_days {
      | Some(d) => d * 86400 <= config.lock_duration_ceiling_s
      | None => true
    }
    val list: List[StakeDeposit] =
      if (staked.keys().contains(sender)) staked.get(sender) else List()
    val updated_list: List[StakeDeposit] =
      list.foldl(List(), (acc, d) => acc.append({
        staker: d.staker,
        amount: d.amount,
        stake_time: d.stake_time,
        unstake_start_time: d.unstake_start_time,
        locked: match d.locked {
          | Some(_) => d.locked        // already locked, untouched
          | None    => Some(locked)
        },
        last_accrued: d.last_accrued,
      }))
    all {
      staked.keys().contains(sender),
      amount > 0,
      locked.locked_until <= block_time + config.lock_duration_ceiling_s,
      perpetual_ok,
      staked' = staked.put(sender, updated_list),
      config' = config, staking_totals' = staking_totals,
      delegations' = delegations, delegate_claims' = delegate_claims,
      fee_events' = fee_events, incentive_scheduling' = incentive_scheduling,
      buyback_and_burn' = buyback_and_burn, ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- RefreshLock (contract.rs:61-105) ---
  // Reset locked.locked_until on all currently-locked deposits to
  // (block_time + intended_lock_days*86400). When deposit_index is Some(_),
  // the Rust impl refreshes only that index — we approximate the same by
  // refreshing every locked deposit (the index branch would require a
  // bounded list[i] update which is not idiomatic in this organ; bounded
  // indices are validated at composition).
  action RefreshLock(sender: Addr,
                      user: Option[Addr],
                      deposit_index: Option[int]): bool = {
    val target: Addr = match user {
      | Some(a) => a
      | None => sender
    }
    val list: List[StakeDeposit] =
      if (staked.keys().contains(target)) staked.get(target) else List()
    val updated_list: List[StakeDeposit] =
      list.foldl(List(), (acc, d) => acc.append({
        staker: d.staker,
        amount: d.amount,
        stake_time: d.stake_time,
        unstake_start_time: d.unstake_start_time,
        locked: match d.locked {
          | Some(l) => Some({
              locked_until: match l.intended_lock_days {
                | Some(days) => block_time + days * 86400
                | None       => l.locked_until
              },
              perpetual_lock_days: l.perpetual_lock_days,
              intended_lock_days: l.intended_lock_days,
            })
          | None => None
        },
        last_accrued: d.last_accrued,
      }))
    all {
      staked.keys().contains(target),
      staked' = staked.put(target, updated_list),
      config' = config, staking_totals' = staking_totals,
      delegations' = delegations, delegate_claims' = delegate_claims,
      fee_events' = fee_events, incentive_scheduling' = incentive_scheduling,
      buyback_and_burn' = buyback_and_burn, ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- UpdateConfig (contract.rs:393-494) ---
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    sender == config.owner or
            (ownership_transfer != None and Some(sender) == ownership_transfer),
    new_cfg.unstaking_period_s >= MIN_UNSTAKING_PERIOD_S,
    new_cfg.lock_duration_ceiling_s >= 0,
    new_cfg.max_commission_rate <= DECIMAL_ONE,
    config' = new_cfg,
    staking_totals' = staking_totals, staked' = staked,
    delegations' = delegations, delegate_claims' = delegate_claims,
    fee_events' = fee_events, incentive_scheduling' = incentive_scheduling,
    buyback_and_burn' = buyback_and_burn, ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- BuybackAndBurn (contract.rs:1702-1745) ---
  // Rust dispatches a swap SubMsg (CDT->MBRN through the proxy) and saves
  // LAST_MBRN_TOTAL_BALANCE for the BURN_REPLY_ID handler to diff against.
  // LAST_MBRN_TOTAL_BALANCE isn't part of the modeled state, so we only
  // queue the swap-reply placeholder here. The buyback_and_burn flag is the
  // ENABLE-flag, NOT a one-shot trigger — it stays set across calls.
  action BuybackAndBurn(sender: Addr, max_slippage: Option[Decimal]): bool = all {
    buyback_and_burn,
    msg_queue' = msg_queue.append(Msg_Reply({
      id: 0, data: "buyback_swap"
    })),
    config' = config, staking_totals' = staking_totals, staked' = staked,
    delegations' = delegations, delegate_claims' = delegate_claims,
    fee_events' = fee_events, incentive_scheduling' = incentive_scheduling,
    buyback_and_burn' = buyback_and_burn, ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- TrimFeeEvents (contract.rs:2591-2638) ---
  action TrimFeeEvents(sender: Addr, cutoff: Timestamp): bool = {
    val trimmed: List[FeeEvent] =
      fee_events.foldl(List(), (acc, e) =>
        if (e.time_of_event >= cutoff) acc.append(e) else acc)
    all {
      sender == config.owner,
      fee_events' = trimmed,
      config' = config, staking_totals' = staking_totals, staked' = staked,
      delegations' = delegations, delegate_claims' = delegate_claims,
      incentive_scheduling' = incentive_scheduling, buyback_and_burn' = buyback_and_burn,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_totals_match_staked: bool =
    staking_totals.stakers <=
      staked.keys().fold(0, (acc, u) => acc + total_user_stake(staked, u))

  val inv_min_stake_per_deposit: bool =
    staked.keys().forall(u => {
      val list = staked.get(u)
      list.indices().forall(i => list[i].amount >= MIN_STAKE)
    })

  val inv_lock_within_ceiling_inv: bool =
    staked.keys().forall(u => {
      val list = staked.get(u)
      list.indices().forall(i =>
        lock_within_ceiling(list[i].locked, list[i].stake_time,
                              config.lock_duration_ceiling_s))
    })

  val inv_unstaking_period_floor: bool =
    config.unstaking_period_s >= MIN_UNSTAKING_PERIOD_S

  val inv_max_commission_capped: bool =
    config.max_commission_rate <= DECIMAL_ONE

  val inv_delegations_bidirectional: bool =
    delegations.keys().forall(u => {
      val di = delegations.get(u)
      di.delegated.indices().forall(i => {
        val d = di.delegated[i]
        delegations.keys().contains(d.delegate) and
        delegations.get(d.delegate).delegated_to.indices().exists(j =>
          delegations.get(d.delegate).delegated_to[j].delegate == u and
          delegations.get(d.delegate).delegated_to[j].amount == d.amount)
      })
    })

  val inv_fee_event_rates_nonneg: bool =
    fee_events.indices().forall(i => fee_events[i].amount_per_mbrn_staked >= 0)

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_ST: bool =
    inv_unstaking_period_floor and
    inv_max_commission_capped and
    inv_owner_governance_distinct

  val safety: bool =
    inv_totals_match_staked and
    inv_min_stake_per_deposit and
    inv_lock_within_ceiling_inv and
    inv_fee_event_rates_nonneg and
    inv_no_unilateral_capital_loss_path_ST

  action step: bool = all {
    advance_time(1),
    config' = config,
    staking_totals' = staking_totals,
    staked' = staked,
    delegations' = delegations,
    delegate_claims' = delegate_claims,
    fee_events' = fee_events,
    incentive_scheduling' = incentive_scheduling,
    buyback_and_burn' = buyback_and_burn,
    ownership_transfer' = ownership_transfer,
  }
}
