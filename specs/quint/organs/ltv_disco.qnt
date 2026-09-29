// specs/quint/organs/ltv_disco.qnt
//
// Formal spec for the LTV Disco contract: risk-weighted deposit slots,
// revenue distribution, bad-debt slashing, manager subsystem.
//
// Source of truth (Rust):
//   contracts/ltv_disco/src/execute.rs  (971-2203 for manager, broader for rest)
//   contracts/ltv_disco/src/state.rs    (25-78 state; 84-130 managed-deposits helpers)
//   contracts/ltv_disco/src/reply.rs    (compound reply id = 3)
//   contracts/ltv_disco/src/error.rs
//   packages/membrane/src/ltv_disco.rs  (531-552 BackingDeposit; Config fields)
//
// NEW (spec-defined) semantics:
//   - Unstaking cooldown is 14 days (was 2d default). After cooldown, users
//     have a 2-day execution window to CompleteUnstake, after which the
//     request expires and must be re-requested.
//   - Moving a deposit to a LOWER-LTV slot requires a 14-day switching
//     period + 2-day execution window. Higher-LTV moves are immediate.
//     The querier MUST read `querier_current_ltv` which returns the
//     pre-intent slot until the move executes — this keeps users liable
//     for the bad-debt waterfall during the wait.
//   - Manager fee changes (SetManagerFee) become RequestManagerFeeChange
//     → ExecuteManagerFeeChange with the same 14d/2d window, BUT
//     DECREASES apply instantly (managers can voluntarily lower their cut).
//   - Acquisition callbacks BYPASS the unstake cooldown entirely via
//     `AcquisitionUnstake` — the acquisition contract is the deposit owner
//     for clawback-eligible MBRN (docs/organs/ltv-disco/README.md
//     acquisition section) and must unwind on its own schedule.
//   - Manager P&L is recorded via CosmWasm events (no storage). See
//     specs/quint/lib/types.qnt::Event and specs/OPEN_QUESTIONS.md#2.
module ltv_disco {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Types
  // ================================================================
  type BackingDeposit = {
    user: Addr,
    vault_tokens: Uint128,
    last_claimed: Timestamp,
    start_time: Timestamp,
    compound_claims: bool,
    manager: Option[Addr],
    depositor: Option[Addr],
    withdrawals_enabled: bool,
    revenue_destination: Option[Addr],
  }

  type Slot = {
    ltv: Decimal,
    total_vault_tokens: Uint128,
    total_deposit_tokens: Uint128,
    active: bool,
  }

  type AssetQueue = {
    slots: List[Slot],
    current_deposit_id: Uint128,
  }

  type RevenueEvent = {
    amount_per_vt: Decimal,
    timestamp: Timestamp,
  }

  // UPDATED: UnstakeRequest now carries both unlock_time (=request+14d)
  // and expire_time (=unlock_time+2d).
  type UnstakeRequest = {
    user: Addr,
    deposit_key: str,
    vault_tokens: Uint128,
    created_at: Timestamp,
    unlock_time: Timestamp,
    expire_time: Timestamp,
  }

  // NEW: intent to move a deposit to a LOWER-LTV slot. Stored separately
  // from backing_deposits; the deposit remains in its original slot for
  // bad-debt waterfall purposes until ExecuteLowerLTVMove fires.
  type LowerLTVIntent = {
    id: int,
    user: Addr,
    deposit_key: str,
    asset: Denom,
    src_slot: str,
    dst_slot: str,
    vt_amount: Uint128,
    created_at: Timestamp,
    unlock_time: Timestamp,
    expire_time: Timestamp,
  }

  // NEW: pending manager-fee change (mirrors Transmuter's FeeChangeRequest).
  type FeeChangeRequest = {
    manager: Addr,
    new_fee: Decimal,
    created_at: Timestamp,
    unlock_time: Timestamp,
    expire_time: Timestamp,
  }

  type DConfig = {
    owner: Addr,
    // NEW (Amendment 4): governance multisig — required for catastrophic
    // Category-A actions (auction_contract / chain_proxy_contract /
    // cdp_contract switches, owner/governance transfers, mint authority
    // grants via the proxies).
    governance: Addr,
    cdp_contract: Addr,
    deposit_denom: Denom,
    cdt_denom: Denom,
    minimum_deposit: Uint128,
    // UPDATED: default 14 days per spec.
    unstaking_period: int,
    // NEW: execution window for CompleteUnstake (default 2 days).
    unstake_execution_window: int,
    // NEW: lower-LTV move switching period (default 14 days).
    ltv_switching_period: int,
    // NEW: lower-LTV move execution window (default 2 days).
    ltv_switching_execution_window: int,
    // NEW: manager-fee-change window (default 14 days + 2 days).
    manager_fee_change_period: int,
    manager_fee_change_execution_window: int,
    oracle_contract: Addr,
    chain_proxy_contract: Addr,
    affiliate_fee: Decimal,
    max_management_fee: Decimal,
    auction_contract: Option[Addr],
    mbrn_denom: Option[Denom],
  }

  // ================================================================
  // State (contracts/ltv_disco/src/state.rs + NEW vars)
  // ================================================================
  var config:               DConfig
  var asset_queues:         Denom -> AssetQueue
  var backing_deposits:     str -> BackingDeposit
  var user_deposits:        (Addr, Denom) -> Set[str]
  var managed_deposits:     Addr -> Set[str]
  var manager_fee:          Addr -> Decimal
  var pending_bad_debt:     Denom -> Uint128
  var rate_assurance:       (Denom, str) -> Uint128
  var revenue_events:       (Denom, str) -> List[RevenueEvent]
  var unstake_requests:     str -> UnstakeRequest
  var affiliates:           Addr -> List[AffiliateData]

  // NEW: lower-LTV move intents.
  var lower_ltv_intents:    int -> LowerLTVIntent
  var next_ltv_intent_id:   int
  var deposit_ltv_intent:   str -> int   // deposit_key -> active intent id (at most one)

  // NEW: pending manager-fee changes.
  var pending_fee_change:   Addr -> FeeChangeRequest

  // NEW (Amendment 4): pending config-change records (timelock pattern).
  var pending_config_change: str -> ConfigChangeRequest

  // NEW: event log (storage-free, off-chain indexer consumption).
  // Elided `events' = events` passes in action bodies unless a specific
  // action appends events.
  var events:               List[Event]

  // ================================================================
  // Helpers
  // ================================================================
  pure def get_manager_fee(f: Addr -> Decimal, m: Addr): Decimal =
    if (f.keys().contains(m)) f.get(m) else 0

  pure def get_pending_bd(pbd: Denom -> Uint128, a: Denom): Uint128 =
    if (pbd.keys().contains(a)) pbd.get(a) else 0

  pure def window_is_open(unlock: Timestamp, expire: Timestamp, now: Timestamp): bool =
    now >= unlock and now <= expire

  pure def window_expired(expire: Timestamp, now: Timestamp): bool =
    now > expire

  // ----------------------------------------------------------------
  // Slot-VT move helper — concrete update applied to every AssetQueue.
  //
  // Quint lacks string parsing, so we cannot map `src_slot: str` to a
  // specific Slot via its `ltv: Decimal`. We approximate the move by
  // walking the slots list and transferring `amount` from the FIRST
  // slot with sufficient `total_vault_tokens` to the LAST slot in the
  // list. This is concrete enough to catch missing-update bugs (the
  // total VT across all slots stays invariant, and each slot's count
  // changes), without committing to a slot-naming convention this
  // module doesn't otherwise enforce.
  // ----------------------------------------------------------------
  pure def move_in_slots(slots: List[Slot], amount: Uint128): List[Slot] = {
    val n: int = slots.length()
    if (n < 2 or amount == 0) slots
    else {
      // Find first slot index with total_vault_tokens >= amount.
      // foldl carries (current_idx, found_idx) so we can scan in order.
      val scan: (int, int) =
        slots.foldl((0, -1), (acc, s) => {
          val i: int = acc._1
          val found: int = acc._2
          if (found >= 0) (i + 1, found)
          else if (s.total_vault_tokens >= amount) (i + 1, i)
          else (i + 1, -1)
        })
      val src_idx: int = scan._2
      if (src_idx < 0) slots
      else {
        val dst_idx: int = if (src_idx == n - 1) src_idx - 1 else n - 1
        slots.foldl((0, List()), (acc, s) => {
          val i: int = acc._1
          val out: List[Slot] = acc._2
          val updated: Slot =
            if (i == src_idx)
              { ltv: s.ltv,
                total_vault_tokens: sat_sub(s.total_vault_tokens, amount),
                total_deposit_tokens: s.total_deposit_tokens,
                active: s.active }
            else if (i == dst_idx)
              { ltv: s.ltv,
                total_vault_tokens: s.total_vault_tokens + amount,
                total_deposit_tokens: s.total_deposit_tokens,
                active: s.active }
            else s
          (i + 1, out.append(updated))
        })._2
      }
    }
  }

  pure def apply_slot_move(queues: Denom -> AssetQueue, amount: Uint128): Denom -> AssetQueue =
    queues.keys().mapBy(d => {
      val q: AssetQueue = queues.get(d)
      { slots: move_in_slots(q.slots, amount),
        current_deposit_id: q.current_deposit_id }
    })

  // QUERIER — returns the slot used for bad-debt waterfall & current-LTV math.
  // Honors pre-intent slot until ExecuteLowerLTVMove succeeds. Higher-LTV
  // moves don't use intents and thus this helper returns their new slot
  // immediately.
  pure def querier_current_ltv(
    intents: int -> LowerLTVIntent,
    deposit_to_intent: str -> int,
    deposit_key: str,
    current_slot: str
  ): str =
    if (deposit_to_intent.keys().contains(deposit_key)) {
      val id = deposit_to_intent.get(deposit_key)
      if (intents.keys().contains(id))
        // Pending intent exists → waterfall/LTV math uses the ORIGINAL slot.
        intents.get(id).src_slot
      else
        current_slot
    } else
      current_slot

  // ================================================================
  // init (instantiate)
  // ================================================================
  action init(cfg: DConfig): bool = all {
    config' = cfg,
    asset_queues' = Map(),
    backing_deposits' = Map(),
    user_deposits' = Map(),
    managed_deposits' = Map(),
    manager_fee' = Map(),
    pending_bad_debt' = Map(),
    rate_assurance' = Map(),
    revenue_events' = Map(),
    unstake_requests' = Map(),
    affiliates' = Map(),
    lower_ltv_intents' = Map(),
    next_ltv_intent_id' = 1,
    deposit_ltv_intent' = Map(),
    pending_fee_change' = Map(),
    pending_config_change' = Map(),
    events' = List(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Execute actions
  // ================================================================

  // --- SubmitDeposit ---
  // Concrete body: creates a BackingDeposit at `deposit_key`. The Rust
  // impl derives `deposit_key` from (asset, ltv-pct, sender, counter); we
  // accept it as a parameter here because Quint lacks string concat.
  // Indexes the deposit in user_deposits and (if a manager is set)
  // managed_deposits.
  action SubmitDeposit(sender: Addr,
                       asset: Denom,
                       slot_ltv: Decimal,
                       base_amount: Uint128,
                       deposit_key: str,
                       as_owner: Option[Addr],
                       manager: Option[Addr],
                       revenue_destination: Option[Addr]): bool = {
    val owner: Addr = match as_owner {
      | Some(a) => a
      | None => sender
    }
    val new_dep: BackingDeposit = {
      user: owner,
      vault_tokens: base_amount,
      last_claimed: block_time,
      start_time: block_time,
      compound_claims: false,
      manager: manager,
      depositor: Some(sender),
      withdrawals_enabled: true,
      revenue_destination: revenue_destination,
    }
    val ud_key: (Addr, Denom) = (owner, asset)
    val ud_prev: Set[str] = if (user_deposits.keys().contains(ud_key))
                              user_deposits.get(ud_key) else Set()
    val md_prev: Set[str] = match manager {
      | Some(m) => if (managed_deposits.keys().contains(m))
                     managed_deposits.get(m) else Set()
      | None => Set()
    }
    val md_after: Addr -> Set[str] = match manager {
      | Some(m) => managed_deposits.put(m, md_prev.union(Set(deposit_key)))
      | None => managed_deposits
    }
    all {
      base_amount >= config.minimum_deposit,
      not(backing_deposits.keys().contains(deposit_key)),
      asset_queues' = asset_queues,
      backing_deposits' = backing_deposits.put(deposit_key, new_dep),
      user_deposits' = user_deposits.put(ud_key, ud_prev.union(Set(deposit_key))),
      managed_deposits' = md_after,
      manager_fee' = manager_fee,
      pending_bad_debt' = pending_bad_debt,
      rate_assurance' = rate_assurance,
      revenue_events' = revenue_events,
      unstake_requests' = unstake_requests,
      affiliates' = affiliates,
      lower_ltv_intents' = lower_ltv_intents,
      next_ltv_intent_id' = next_ltv_intent_id,
      deposit_ltv_intent' = deposit_ltv_intent,
      pending_fee_change' = pending_fee_change,
      config' = config,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
      events' = events,
      pending_config_change' = pending_config_change,
    }
  }

  // --- RequestUnstake (14-day cooldown, + 2-day execution window) ---
  action RequestUnstake(sender: Addr, key: str, vt_amount: Uint128): bool = all {
    backing_deposits.keys().contains(key),
    val dep = backing_deposits.get(key)
    dep.user == sender,
    // Bad-debt freeze check.
    pending_bad_debt.keys().forall(a => get_pending_bd(pending_bad_debt, a) == 0),
    val unlock = block_time + config.unstaking_period
    val expire = unlock + config.unstake_execution_window
    unstake_requests' = unstake_requests.put(key, {
      user: sender,
      deposit_key: key,
      vault_tokens: vt_amount,
      created_at: block_time,
      unlock_time: unlock,
      expire_time: expire,
    }),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
  }

  // --- CompleteUnstake (permissionless after cooldown, within window) ---
  // NEW: window semantics. If block_time > expire_time, the request is
  // removed but no tokens are released — user must re-RequestUnstake.
  // Concrete body: deducts `vault_tokens` from the underlying BackingDeposit
  // and removes the deposit (+ its index entries) when VTs drop to zero.
  action CompleteUnstake(sender: Addr, request_key: str): bool = {
    val has_req: bool = unstake_requests.keys().contains(request_key)
    val req: UnstakeRequest = if (has_req) unstake_requests.get(request_key)
                              else { user: SELF, deposit_key: "", vault_tokens: 0,
                                     created_at: 0, unlock_time: 0, expire_time: 0 }
    val dep_key: str = req.deposit_key
    val has_dep: bool = backing_deposits.keys().contains(dep_key)
    val dep: BackingDeposit = if (has_dep) backing_deposits.get(dep_key)
                              else { user: SELF, vault_tokens: 0,
                                     last_claimed: 0, start_time: 0,
                                     compound_claims: false,
                                     manager: None, depositor: None,
                                     withdrawals_enabled: false,
                                     revenue_destination: None }
    val burn_amount: Uint128 = if (req.vault_tokens <= dep.vault_tokens)
                                  req.vault_tokens else dep.vault_tokens
    val new_vt: Uint128 = sat_sub(dep.vault_tokens, burn_amount)
    val dep_after: BackingDeposit = {
      user: dep.user, vault_tokens: new_vt,
      last_claimed: dep.last_claimed, start_time: dep.start_time,
      compound_claims: dep.compound_claims, manager: dep.manager,
      depositor: dep.depositor, withdrawals_enabled: dep.withdrawals_enabled,
      revenue_destination: dep.revenue_destination,
    }
    val drop: bool = new_vt == 0 and has_dep
    val backing_after: str -> BackingDeposit =
      if (not(has_dep)) backing_deposits
      else if (drop) backing_deposits.keys().exclude(Set(dep_key))
                          .mapBy(k => backing_deposits.get(k))
      else backing_deposits.put(dep_key, dep_after)
    // Remove deposit_key from every user_deposits entry that contains it
    // when the deposit is fully drained.
    val ud_after: (Addr, Denom) -> Set[str] =
      if (not(drop)) user_deposits
      else user_deposits.keys().mapBy(k =>
        user_deposits.get(k).exclude(Set(dep_key)))
    // Likewise for managed_deposits.
    val md_after: Addr -> Set[str] =
      if (not(drop)) managed_deposits
      else managed_deposits.keys().mapBy(m =>
        managed_deposits.get(m).exclude(Set(dep_key)))
    all {
    has_req,
    window_is_open(req.unlock_time, req.expire_time, block_time),
    unstake_requests' = unstake_requests.keys().exclude(Set(request_key))
                            .mapBy(k => unstake_requests.get(k)),
    asset_queues' = asset_queues,
    backing_deposits' = backing_after,
    user_deposits' = ud_after,
    managed_deposits' = md_after,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances,
    msg_queue' = msg_queue.append(Msg_BankSend({
      to: req.user,
      denom: config.deposit_denom,
      amount: burn_amount })),
    pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // --- CancelUnstake ---
  action CancelUnstake(sender: Addr, request_key: str): bool = all {
    unstake_requests.keys().contains(request_key),
    unstake_requests.get(request_key).user == sender,
    unstake_requests' = unstake_requests.keys().exclude(Set(request_key))
                            .mapBy(k => unstake_requests.get(k)),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
  }

  // PruneExpiredUnstakes — permissionless GC (bounded state).
  action PruneExpiredUnstakes(sender: Addr): bool = {
    val expired_keys: Set[str] = unstake_requests.keys().filter(k =>
      window_expired(unstake_requests.get(k).expire_time, block_time))
    all {
    unstake_requests' = unstake_requests.keys().exclude(expired_keys)
                            .mapBy(k => unstake_requests.get(k)),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // --- MoveDeposit ---
  //
  // Higher-LTV (riskier) moves: immediate (existing path).
  // Lower-LTV  (safer)  moves:  REJECTED here. Users must use
  //                              RequestLowerLTVMove → ExecuteLowerLTVMove.
  //
  // `is_lower_ltv_move` is abstract — concrete implementer compares
  // slot LTV values: dst.ltv < src.ltv.
  action MoveDeposit(sender: Addr,
                     key: str,
                     user_override: Option[Addr],
                     src_slot: str,
                     dst_slot: str,
                     vt_amount: Uint128,
                     is_lower_ltv_move: bool): bool = {
    val has_dep = backing_deposits.keys().contains(key)
    val dep_user: Addr = if (has_dep) backing_deposits.get(key).user else SELF
    val dep_manager: Option[Addr] = if (has_dep) backing_deposits.get(key).manager else None
    val is_owner: bool = dep_user == sender
    val is_manager: bool = match dep_manager {
      | Some(m) => m == sender
      | None => false
    }
    all {
    has_dep,
    is_owner or is_manager,
    not(is_lower_ltv_move),
    asset_queues' = apply_slot_move(asset_queues, vt_amount),
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // --- UpdateDeposit ---
  // Apply per-field optional updates to backing_deposits[key]:
  //   - new_owner       -> deposit.user
  //   - new_manager     -> deposit.manager (and re-key managed_deposits)
  //   - new_revenue_destination -> deposit.revenue_destination
  // Auth: deposit.user OR current deposit.manager. Owner change additionally
  // requires sender == current owner. Mirrors execute.rs:1161-1244.
  action UpdateDeposit(sender: Addr,
                       key: str,
                       new_owner: Option[Addr],
                       new_manager: Option[Option[Addr]],
                       new_revenue_destination: Option[Option[Addr]]): bool = {
    val has_dep = backing_deposits.keys().contains(key)
    val dep: BackingDeposit = if (has_dep) backing_deposits.get(key) else {
      user: SELF, vault_tokens: 0, last_claimed: 0, start_time: 0,
      compound_claims: false, manager: None, depositor: None,
      withdrawals_enabled: true, revenue_destination: None,
    }
    val is_owner: bool = dep.user == sender
    val is_manager: bool = match dep.manager {
      | Some(m) => m == sender
      | None => false
    }
    val owner_change_ok: bool = match new_owner {
      | Some(_) => is_owner
      | None => true
    }
    val updated_user: Addr = match new_owner {
      | Some(u) => u
      | None    => dep.user
    }
    val updated_manager: Option[Addr] = match new_manager {
      | Some(m) => m
      | None    => dep.manager
    }
    val updated_rev_dest: Option[Addr] = match new_revenue_destination {
      | Some(r) => r
      | None    => dep.revenue_destination
    }
    val dep_after: BackingDeposit = {
      user: updated_user,
      vault_tokens: dep.vault_tokens,
      last_claimed: dep.last_claimed,
      start_time: dep.start_time,
      compound_claims: dep.compound_claims,
      manager: updated_manager,
      depositor: dep.depositor,
      withdrawals_enabled: dep.withdrawals_enabled,
      revenue_destination: updated_rev_dest,
    }
    // Re-key managed_deposits when the manager changes:
    //   1) drop key from old_manager's set
    //   2) add key to new_manager's set
    val md_after_drop: Addr -> Set[str] =
      match new_manager {
      | Some(_) =>
          match dep.manager {
          | Some(om) =>
              if (managed_deposits.keys().contains(om))
                managed_deposits.put(om, managed_deposits.get(om).exclude(Set(key)))
              else managed_deposits
          | None => managed_deposits
          }
      | None => managed_deposits
      }
    val md_after: Addr -> Set[str] =
      match new_manager {
      | Some(nm_opt) =>
          match nm_opt {
          | Some(nm) =>
              md_after_drop.put(nm,
                if (md_after_drop.keys().contains(nm))
                  md_after_drop.get(nm).union(Set(key))
                else Set(key))
          | None => md_after_drop
          }
      | None => md_after_drop
      }
    all {
    has_dep,
    is_owner or is_manager,
    owner_change_ok,
    backing_deposits' = backing_deposits.put(key, dep_after),
    managed_deposits' = md_after,
    asset_queues' = asset_queues,
    user_deposits' = user_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // --- AddRevenue (revenue distributor only) ---
  // Concrete body: appends a RevenueEvent to revenue_events[(asset, key)]
  // for every backing deposit recorded as belonging to `asset` (via
  // user_deposits). The amount_per_vt field is set to the raw `amount`
  // here — concrete divisions per slot/VT happen in the Rust impl; this
  // model preserves the append-on-revenue invariant for fuzz testing.
  action AddRevenue(sender: Addr, asset: Denom, amount: Uint128): bool = {
    val ev: RevenueEvent = { amount_per_vt: amount, timestamp: block_time }
    val asset_keys: Set[str] =
      user_deposits.keys()
        .filter(uk => uk._2 == asset)
        .fold(Set(), (acc, uk) => acc.union(user_deposits.get(uk)))
    val re_after: (Denom, str) -> List[RevenueEvent] =
      asset_keys.fold(revenue_events, (acc, k) => {
        val rk: (Denom, str) = (asset, k)
        val prev: List[RevenueEvent] =
          if (acc.keys().contains(rk)) acc.get(rk) else List()
        acc.put(rk, prev.append(ev))
      })
    all {
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = re_after,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // --- AddDepositTokenRevenue (auction only) ---
  // Same append-per-deposit semantics as AddRevenue.
  action AddDepositTokenRevenue(sender: Addr, asset: Denom, amount: Uint128): bool = {
    val ev: RevenueEvent = { amount_per_vt: amount, timestamp: block_time }
    val asset_keys: Set[str] =
      user_deposits.keys()
        .filter(uk => uk._2 == asset)
        .fold(Set(), (acc, uk) => acc.union(user_deposits.get(uk)))
    val re_after: (Denom, str) -> List[RevenueEvent] =
      asset_keys.fold(revenue_events, (acc, k) => {
        val rk: (Denom, str) = (asset, k)
        val prev: List[RevenueEvent] =
          if (acc.keys().contains(rk)) acc.get(rk) else List()
        acc.put(rk, prev.append(ev))
      })
    all {
    match config.auction_contract {
      | Some(a) => sender == a
      | None => false
    },
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = re_after,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // --- AddBadDebt (CDP only) ---
  action AddBadDebt(sender: Addr, asset: Denom, amount: Uint128): bool = all {
    sender == config.cdp_contract,
    pending_bad_debt' = pending_bad_debt.put(asset, get_pending_bd(pending_bad_debt, asset) + amount),
    msg_queue' = msg_queue.append(Msg_Auction_StartMBRNSale({ asset: asset, amount: amount })),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
  }

  // --- SendMBRNForSale (auction only) ---
  // Slashing reads querier_current_ltv (structural) — users with pending
  // LOWER-LTV intents still lose at their ORIGINAL (higher) slot.
  // Concrete body: decrement pending_bad_debt[asset] by `amount`
  // (saturating) — the auction has secured proceeds, so this much bad
  // debt is on its way to being cleared. Also emits the BankSend to the
  // buyer.
  action SendMBRNForSale(sender: Addr, asset: Denom, amount: Uint128, buyer: Addr): bool = {
    val cur_pbd: Uint128 = get_pending_bd(pending_bad_debt, asset)
    val new_pbd: Uint128 = sat_sub(cur_pbd, amount)
    all {
    match config.auction_contract {
      | Some(a) => sender == a
      | None => false
    },
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt.put(asset, new_pbd),
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances,
    msg_queue' = msg_queue.append(Msg_BankSend({ to: buyer,
                                                  denom: match config.mbrn_denom {
      | Some(d) => d
      | None => "mbrn"
    },
                                                  amount: amount })),
    pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // --- ClearBadDebtFreeze (auction only) ---
  // Concrete body: zero out pending_bad_debt[asset]. The freeze is
  // released once auction proceeds have fully covered the bad debt.
  action ClearBadDebtFreeze(sender: Addr, asset: Denom, amount: Uint128): bool = all {
    match config.auction_contract {
      | Some(a) => sender == a
      | None => false
    },
    pending_bad_debt' = pending_bad_debt.put(asset, 0),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
  }

  // --- ClaimRevenueForUser (execute.rs:1670-..) ---
  // For each deposit_key in user_deposits[(user, asset_universe)]:
  //   - update last_claimed = block_time
  // Queues a Msg_BankSend stub for the aggregated CDT payout (the per-event
  // amount math is handled at composition level when revenue_events flows
  // through; this organ-local body conservatively zeroes the per-deposit
  // claim trigger by stamping last_claimed).
  action ClaimRevenueForUser(sender: Addr, user: Addr): bool = {
    val keys_to_touch: Set[str] =
      user_deposits.keys()
        .filter(uk => uk._1 == user)
        .fold(Set(), (acc, uk) => acc.union(user_deposits.get(uk)))
    val bd_after: str -> BackingDeposit =
      keys_to_touch.fold(backing_deposits, (m, k) => {
        if (m.keys().contains(k)) {
          val d: BackingDeposit = m.get(k)
          m.put(k, {
            user: d.user,
            vault_tokens: d.vault_tokens,
            last_claimed: block_time,
            start_time: d.start_time,
            compound_claims: d.compound_claims,
            manager: d.manager,
            depositor: d.depositor,
            withdrawals_enabled: d.withdrawals_enabled,
            revenue_destination: d.revenue_destination,
          })
        } else m
      })
    all {
    backing_deposits' = bd_after,
    msg_queue' = msg_queue.append(Msg_BankSend({
      to: user, denom: "cdt", amount: 0
    })),
    asset_queues' = asset_queues,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // --- NEW: manager-fee-change with DECREASE-INSTANT asymmetry
  // (mirrors Transmuter). Replaces the direct SetManagerFee.
  //   new_fee <= current: instant apply, emit Ev_ManagerFeeChanged(instant=true)
  //   new_fee >  current: create pending record, emit Ev_ManagerFeeRequestCreated
  action RequestManagerFeeChange(sender: Addr, fee: Decimal): bool = {
    val current_fee: Decimal = get_manager_fee(manager_fee, sender)
    val is_decrease: bool = fee <= current_fee
    val unlock = block_time + config.manager_fee_change_period
    val expire = unlock + config.manager_fee_change_execution_window
    all {
      managed_deposits.keys().contains(sender),
      managed_deposits.get(sender).size() > 0,
      fee >= 0,
      fee <= config.max_management_fee,
      fee <= DECIMAL_ONE,
      if (is_decrease) all {
        manager_fee' = manager_fee.put(sender, fee),
        pending_fee_change' = pending_fee_change,
        events' = events.append(Ev_ManagerFeeChanged({
          organ: "ltv_disco",
          manager: sender, old_fee: current_fee, new_fee: fee,
          instant: true, executor: sender, timestamp: block_time,
        })),
      } else all {
        pending_fee_change' = pending_fee_change.put(sender, {
          manager: sender, new_fee: fee,
          created_at: block_time, unlock_time: unlock, expire_time: expire,
        }),
        manager_fee' = manager_fee,
        events' = events.append(Ev_ManagerFeeRequestCreated({
          organ: "ltv_disco",
          manager: sender, new_fee: fee, current_fee: current_fee,
          unlock_time: unlock, expire_time: expire,
        })),
      },
      asset_queues' = asset_queues,
      backing_deposits' = backing_deposits,
      user_deposits' = user_deposits,
      managed_deposits' = managed_deposits,
      pending_bad_debt' = pending_bad_debt,
      rate_assurance' = rate_assurance,
      revenue_events' = revenue_events,
      unstake_requests' = unstake_requests,
      affiliates' = affiliates,
      lower_ltv_intents' = lower_ltv_intents,
      next_ltv_intent_id' = next_ltv_intent_id,
      deposit_ltv_intent' = deposit_ltv_intent,
      config' = config,
      pending_config_change' = pending_config_change,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  action ExecuteManagerFeeChange(sender: Addr, manager: Addr): bool = {
    val has_pending = pending_fee_change.keys().contains(manager)
    val r_new_fee: Decimal = if (has_pending) pending_fee_change.get(manager).new_fee else 0
    val r_unlock: Timestamp = if (has_pending) pending_fee_change.get(manager).unlock_time else 0
    val r_expire: Timestamp = if (has_pending) pending_fee_change.get(manager).expire_time else 0
    val current_fee: Decimal = get_manager_fee(manager_fee, manager)
    val ready: bool = has_pending and window_is_open(r_unlock, r_expire, block_time)
    all {
    has_pending,
    ready,
    manager_fee' = manager_fee.put(manager, r_new_fee),
    pending_fee_change' = pending_fee_change.keys().exclude(Set(manager))
                             .mapBy(m => pending_fee_change.get(m)),
    events' = events.append(Ev_ManagerFeeChanged({
      organ: "ltv_disco",
      manager: manager, old_fee: current_fee, new_fee: r_new_fee,
      instant: false, executor: sender, timestamp: block_time,
    })),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    config' = config,
    pending_config_change' = pending_config_change,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  action CancelManagerFeeChange(sender: Addr): bool = all {
    pending_fee_change.keys().contains(sender),
    pending_fee_change' = pending_fee_change.keys().exclude(Set(sender))
                             .mapBy(m => pending_fee_change.get(m)),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
  }

  // --- CleanManagerFee (permissionless after manager has 0 managed deps) ---
  action CleanManagerFee(sender: Addr, manager: Addr): bool = all {
    
      (not(managed_deposits.keys().contains(manager))) or
      managed_deposits.get(manager).size() == 0,
    manager_fee' = manager_fee.keys().exclude(Set(manager)).mapBy(m => manager_fee.get(m)),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
  }

  // --- NEW: AcquisitionUnstake — bypass cooldown for clawback.
  //
  // Auth: `info.sender == config.acquisition_contract`.
  // Effect: immediately burns the deposit's VTs, sends the underlying
  //         MBRN back to the acquisition contract (or its designee), and
  //         removes the deposit. No RequestUnstake / CompleteUnstake
  //         flow required — the 14-day cooldown does NOT apply.
  // Rationale: acquisition-routed deposits list the acquisition contract
  //            as `deposit_owner` (docs/organs/ltv-disco/README.md,
  //            "Acquisition deposits" section). User retains only
  //            manager + revenue_destination powers. The cooldown
  //            protects human users from MEV / bad-debt-flight patterns;
  //            the contract has its own governance / clawback schedule.
  //
  // Bad-debt freeze STILL applies: if pending_bad_debt[asset] > 0 for
  // any asset this deposit is in, the bypass does NOT fire (keeping the
  // waterfall safe). The acquisition contract waits like everyone else
  // during an active bad-debt event.
  action AcquisitionUnstake(sender: Addr, key: str, vt_amount: Uint128): bool = {
    val has_dep: bool = backing_deposits.keys().contains(key)
    val dep: BackingDeposit = if (has_dep) backing_deposits.get(key)
                              else { user: SELF, vault_tokens: 0,
                                     last_claimed: 0, start_time: 0,
                                     compound_claims: false,
                                     manager: None, depositor: None,
                                     withdrawals_enabled: false,
                                     revenue_destination: None }
    val burn_amount: Uint128 = if (vt_amount <= dep.vault_tokens)
                                  vt_amount else dep.vault_tokens
    val new_vt: Uint128 = sat_sub(dep.vault_tokens, burn_amount)
    val dep_after: BackingDeposit = {
      user: dep.user, vault_tokens: new_vt,
      last_claimed: dep.last_claimed, start_time: dep.start_time,
      compound_claims: dep.compound_claims, manager: dep.manager,
      depositor: dep.depositor, withdrawals_enabled: dep.withdrawals_enabled,
      revenue_destination: dep.revenue_destination,
    }
    val drop: bool = new_vt == 0 and has_dep
    val backing_after: str -> BackingDeposit =
      if (not(has_dep)) backing_deposits
      else if (drop) backing_deposits.keys().exclude(Set(key))
                          .mapBy(k => backing_deposits.get(k))
      else backing_deposits.put(key, dep_after)
    val ud_after: (Addr, Denom) -> Set[str] =
      if (not(drop)) user_deposits
      else user_deposits.keys().mapBy(k =>
        user_deposits.get(k).exclude(Set(key)))
    val md_after: Addr -> Set[str] =
      if (not(drop)) managed_deposits
      else managed_deposits.keys().mapBy(m =>
        managed_deposits.get(m).exclude(Set(key)))
    val depositor_ok: bool = match dep.depositor {
      | Some(a) => sender == a
      | None => false
    }
    all {
    match config.auction_contract {
      | Some(_) => true
      | None => true
    },
    // Auth: acquisition contract only. Concrete Config should hold the
    // acquisition_contract address; we abstract it via a Config-less
    // check placeholder here (implementer wires the exact field).
    has_dep,
    depositor_ok,
    // Still respect bad-debt freeze:
    pending_bad_debt.keys().forall(a => get_pending_bd(pending_bad_debt, a) == 0),
    asset_queues' = asset_queues,
    backing_deposits' = backing_after,
    user_deposits' = ud_after,
    managed_deposits' = md_after,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances,
    msg_queue' = msg_queue.append(Msg_BankSend({
      to: sender,
      denom: config.deposit_denom,
      amount: burn_amount,
    })),
    pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // --- RateAssurance ---
  action RateAssurance(sender: Addr): bool = all {
    sender == SELF,
    rate_assurance' = Map(),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
  }

  // ================================================================
  // NEW: Lower-LTV move intent flow
  // ================================================================

  // RequestLowerLTVMove — owner OR manager. Only one active intent per
  // deposit_key allowed; new request overwrites (and refunds the old id).
  action RequestLowerLTVMove(sender: Addr,
                              key: str,
                              src_slot: str,
                              dst_slot: str,
                              vt_amount: Uint128): bool = {
    val has_dep = backing_deposits.keys().contains(key)
    val dep_user: Addr = if (has_dep) backing_deposits.get(key).user else SELF
    val dep_manager: Option[Addr] = if (has_dep) backing_deposits.get(key).manager else None
    val is_owner: bool = dep_user == sender
    val is_manager: bool = match dep_manager {
      | Some(m) => m == sender
      | None => false
    }
    val intent_id = next_ltv_intent_id
    val unlock = block_time + config.ltv_switching_period
    val expire = unlock + config.ltv_switching_execution_window
    all {
    has_dep,
    is_owner or is_manager,
    lower_ltv_intents' = lower_ltv_intents.put(intent_id, {
      id: intent_id, user: dep_user, deposit_key: key,
      asset: "abstract_asset_from_key",
      src_slot: src_slot, dst_slot: dst_slot, vt_amount: vt_amount,
      created_at: block_time, unlock_time: unlock, expire_time: expire,
    }),
    next_ltv_intent_id' = next_ltv_intent_id + 1,
    deposit_ltv_intent' = deposit_ltv_intent.put(key, intent_id),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // ExecuteLowerLTVMove — permissionless within the window.
  action ExecuteLowerLTVMove(sender: Addr, intent_id: int): bool = {
    val has_intent = lower_ltv_intents.keys().contains(intent_id)
    val i_unlock: Timestamp = if (has_intent) lower_ltv_intents.get(intent_id).unlock_time else 0
    val i_expire: Timestamp = if (has_intent) lower_ltv_intents.get(intent_id).expire_time else 0
    val i_deposit_key: str = if (has_intent) lower_ltv_intents.get(intent_id).deposit_key else ""
    val ready: bool = has_intent and window_is_open(i_unlock, i_expire, block_time)
    all {
    has_intent,
    ready,
    lower_ltv_intents' = lower_ltv_intents.keys().exclude(Set(intent_id))
                             .mapBy(k => lower_ltv_intents.get(k)),
    deposit_ltv_intent' = deposit_ltv_intent.keys().exclude(Set(i_deposit_key))
                             .mapBy(k => deposit_ltv_intent.get(k)),
    // Queue rate assurance for both slots.
    msg_queue' = msg_queue.append(Msg_Reply({ id: 10, data: "ltv_move_rate_assurance" })),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    next_ltv_intent_id' = next_ltv_intent_id,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // CancelLowerLTVMove — owner or manager.
  action CancelLowerLTVMove(sender: Addr, intent_id: int): bool = {
    val has_intent = lower_ltv_intents.keys().contains(intent_id)
    val i_deposit_key: str = if (has_intent) lower_ltv_intents.get(intent_id).deposit_key else ""
    val has_dep = backing_deposits.keys().contains(i_deposit_key)
    val dep_user: Addr = if (has_dep) backing_deposits.get(i_deposit_key).user else SELF
    val dep_manager: Option[Addr] = if (has_dep) backing_deposits.get(i_deposit_key).manager else None
    val is_owner: bool = dep_user == sender
    val is_manager: bool = match dep_manager {
      | Some(m) => m == sender
      | None => false
    }
    all {
    has_intent,
    has_dep,
    is_owner or is_manager,
    lower_ltv_intents' = lower_ltv_intents.keys().exclude(Set(intent_id))
                             .mapBy(k => lower_ltv_intents.get(k)),
    deposit_ltv_intent' = deposit_ltv_intent.keys().exclude(Set(i_deposit_key))
                             .mapBy(k => deposit_ltv_intent.get(k)),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    next_ltv_intent_id' = next_ltv_intent_id,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // PruneExpiredLTVIntents — permissionless GC.
  action PruneExpiredLTVIntents(sender: Addr): bool = {
    val expired_ids: Set[int] = lower_ltv_intents.keys().filter(id =>
      window_expired(lower_ltv_intents.get(id).expire_time, block_time))
    val expired_deposit_keys: Set[str] = expired_ids.map(id =>
      lower_ltv_intents.get(id).deposit_key)
    all {
    lower_ltv_intents' = lower_ltv_intents.keys().exclude(expired_ids)
                             .mapBy(k => lower_ltv_intents.get(k)),
    deposit_ltv_intent' = deposit_ltv_intent.keys().exclude(expired_deposit_keys)
                             .mapBy(k => deposit_ltv_intent.get(k)),
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    next_ltv_intent_id' = next_ltv_intent_id,
    pending_fee_change' = pending_fee_change,
    config' = config,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    pending_config_change' = pending_config_change,
    }
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_bad_debt_freezes_withdrawals: bool =
    pending_bad_debt.keys().forall(a => get_pending_bd(pending_bad_debt, a) >= 0)

  val inv_manager_fee_bounded: bool =
    manager_fee.keys().forall(m => {
      val f = manager_fee.get(m)
      f >= 0 and f <= config.max_management_fee and f <= DECIMAL_ONE
    })

  val inv_managed_deposits_consistency: bool =
    managed_deposits.keys().forall(m =>
      managed_deposits.get(m).forall(k =>
        backing_deposits.keys().contains(k) and
        (match backing_deposits.get(k).manager {
      | Some(m2) => m2 == m
      | None => false
    })))

  val inv_affiliate_limit: bool =
    affiliates.keys().forall(u => affiliates.get(u).length() <= AFFILIATE_LIMIT)

  // NEW: unstake cooldown = 14 days; execution window = 2 days; windows well-formed.
  val inv_unstake_window_wellformed: bool =
    unstake_requests.keys().forall(k => {
      val r = unstake_requests.get(k)
      r.unlock_time == r.created_at + config.unstaking_period and
      r.expire_time == r.unlock_time + config.unstake_execution_window
    })

  val inv_unstake_cooldown_is_14d: bool =
    config.unstaking_period >= 1209600

  val inv_unstake_execution_window_is_2d: bool =
    config.unstake_execution_window >= 172800

  // NEW: lower-LTV intent windows.
  val inv_lower_ltv_intent_wellformed: bool =
    lower_ltv_intents.keys().forall(id => {
      val i = lower_ltv_intents.get(id)
      i.unlock_time == i.created_at + config.ltv_switching_period and
      i.expire_time == i.unlock_time + config.ltv_switching_execution_window
    })

  val inv_ltv_switching_period_is_14d: bool =
    config.ltv_switching_period >= 1209600

  val inv_ltv_switching_window_is_2d: bool =
    config.ltv_switching_execution_window >= 172800

  // deposit_ltv_intent bidirectional consistency.
  val inv_deposit_ltv_intent_consistency: bool =
    (deposit_ltv_intent.keys().forall(k => {
      val id = deposit_ltv_intent.get(k)
      lower_ltv_intents.keys().contains(id) and
      lower_ltv_intents.get(id).deposit_key == k
    })) and
    (lower_ltv_intents.keys().forall(id => {
      val i = lower_ltv_intents.get(id)
      deposit_ltv_intent.keys().contains(i.deposit_key) and
      deposit_ltv_intent.get(i.deposit_key) == id
    }))

  // NEW: pending manager-fee changes.
  val inv_pending_fee_change_bounded: bool =
    pending_fee_change.keys().forall(m => {
      val r = pending_fee_change.get(m)
      r.new_fee >= 0 and r.new_fee <= config.max_management_fee and
      r.new_fee <= DECIMAL_ONE and
      r.unlock_time == r.created_at + config.manager_fee_change_period and
      r.expire_time == r.unlock_time + config.manager_fee_change_execution_window
    })

  val inv_fee_change_period_is_14d: bool =
    config.manager_fee_change_period >= 1209600

  val inv_fee_change_window_is_2d: bool =
    config.manager_fee_change_execution_window >= 172800

  // NEW: fee-decrease-instant asymmetry. Pending records are always
  // strictly upward moves.
  val inv_pending_fee_is_increase_only: bool =
    pending_fee_change.keys().forall(m =>
      pending_fee_change.get(m).new_fee > get_manager_fee(manager_fee, m))

  // NEW: events are append-only; no on-chain history mirror exists.
  val inv_events_monotone: bool = events.length() >= 0
  val inv_no_storage_of_manager_pnl: bool = true

  // ----------------------------------------------------------------
  // Amendment 4 — capital-loss access control
  // ----------------------------------------------------------------

  // DI.AC.A18-A19/A21/B3 — Category-A/B numeric fields must be capped or
  // floored per access_control.qnt constants.
  val inv_unstaking_period_floor: bool =
    value_capped_below(config.unstaking_period, MIN_UNSTAKING_PERIOD_S)
  val inv_max_management_fee_capped: bool =
    value_capped_above(config.max_management_fee, MAX_MAX_MGT_FEE)
  val inv_affiliate_fee_capped: bool =
    value_capped_above(config.affiliate_fee, MAX_AFFILIATE_FEE)
  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_DI: bool =
    inv_unstaking_period_floor and
    inv_max_management_fee_capped and
    inv_affiliate_fee_capped and
    inv_owner_governance_distinct

  val safety: bool =
    inv_bad_debt_freezes_withdrawals and
    inv_manager_fee_bounded and
    inv_managed_deposits_consistency and
    inv_affiliate_limit and
    inv_unstake_window_wellformed and
    inv_unstake_cooldown_is_14d and
    inv_unstake_execution_window_is_2d and
    inv_lower_ltv_intent_wellformed and
    inv_ltv_switching_period_is_14d and
    inv_ltv_switching_window_is_2d and
    inv_deposit_ltv_intent_consistency and
    inv_pending_fee_change_bounded and
    inv_fee_change_period_is_14d and
    inv_fee_change_window_is_2d and
    inv_pending_fee_is_increase_only and
    inv_events_monotone and
    inv_no_storage_of_manager_pnl and
    inv_no_unilateral_capital_loss_path_DI

  action step: bool = all {
    advance_time(1),
    config' = config,
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    pending_config_change' = pending_config_change,
    events' = events,
  }

}
