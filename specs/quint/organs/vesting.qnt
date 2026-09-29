// specs/quint/organs/vesting.qnt
//
// MBRN vesting — cliff + linear schedules per recipient, plus a weekly
// "vested transmutation" path that mints new MBRN against burned old MBRN.
//
// Source of truth (Rust):
//   contracts/vesting/src/state.rs              (8 storage items)
//   contracts/vesting/src/contract.rs           (handlers; line cites below)
//   contracts/vesting/src/error.rs              (6 error variants)
//   packages/membrane/src/vesting.rs            (Config, ExecuteMsg, types)
//   packages/membrane/src/types.rs:882-915      (VestingPeriod, Allocation, Recipient)
//
// Solidity-port note: vesting has TWO mint paths:
//   1) Legacy: per-recipient `Allocation` via osmosis_proxy.MintTokens.
//   2) Transmutation: per-week `VestingSchedule` via neutron_proxy.MintTokens,
//      gated by an old-MBRN burn invariant (OLD_MBRN_RECEIVED - MINTED_MBRN ==
//      TOTAL_OLD_MBRN). The Solidity port should keep these as separate
//      functions to preserve the auditability boundary.
module vesting {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Types — packages/membrane/src/types.rs:882-915, vesting.rs:142-157
  // ================================================================
  type VestingPeriod = {
    cliff_days: int,
    linear_days: int,
  }

  type Allocation = {
    amount: Uint128,
    amount_withdrawn: Uint128,
    start_time_of_allocation: Timestamp,
    vesting_period: VestingPeriod,
  }

  type Recipient = {
    recipient: Addr,
    allocation: Option[Allocation],
    claimables: List[(Denom, Uint128)],
  }

  type VestingSchedule = {
    user: Addr,
    week_id: int,
    mbrn_to_mint: Uint128,
    amount_withdrawn: Uint128,
    start_time: Timestamp,
    vesting_period: VestingPeriod,
    transmutation_count: int,
  }

  type Config = {
    owner: Addr,
    governance: Addr,
    total_allocation: Uint128,
    mbrn_denom: Denom,
    osmosis_proxy: Addr,
    staking_contract: Addr,
    neutron_proxy: Option[Addr],
    old_mbrn_denom: Option[Denom],
  }

  // ================================================================
  // State
  // ================================================================
  var config:             Config
  var recipients:         List[Recipient]
  var ownership_transfer: Option[Addr]
  var vesting_schedules:  (Addr, int) -> VestingSchedule
  var old_mbrn_received:  Uint128
  var minted_mbrn:        Uint128
  var total_old_mbrn:     Uint128

  action init(cfg: Config): bool = all {
    config' = cfg,
    recipients' = List(),
    ownership_transfer' = None,
    vesting_schedules' = Map(),
    old_mbrn_received' = 0,
    minted_mbrn' = 0,
    total_old_mbrn' = 0,
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure val EMPTY_ALLOCATION: Allocation = {
    amount: 0, amount_withdrawn: 0,
    start_time_of_allocation: 0,
    vesting_period: { cliff_days: 0, linear_days: 0 },
  }

  // foldl-based linear scan: returns the first matching index, or -1 if none.
  pure def find_recipient_idx(rs: List[Recipient], a: Addr): int =
    rs.foldl((-1, 0), (acc, r) => {
      val cur_idx = acc._1
      val pos = acc._2
      if (cur_idx >= 0) (cur_idx, pos + 1)
      else if (r.recipient == a) (pos, pos + 1)
      else (-1, pos + 1)
    })._1

  // Vesting schedule formula — applies to BOTH Allocation and VestingSchedule.
  pure def unlocked_amount(total: Uint128, withdrawn: Uint128,
                            start: Timestamp, vp: VestingPeriod,
                            now: Timestamp): Uint128 = {
    val cliff_s: int = vp.cliff_days * 86400
    val linear_s: int = vp.linear_days * 86400
    val elapsed: int = now - start
    if (elapsed < cliff_s) 0
    else if (elapsed - cliff_s < linear_s) {
      val ratio: Decimal = decimal_from_ratio(elapsed - cliff_s, linear_s)
      val unlocked: Uint128 = decimal_mul_uint(ratio, total)
      sat_sub(unlocked, withdrawn)
    }
    else sat_sub(total, withdrawn)
  }

  pure def alloc_amount(o: Option[Allocation]): Uint128 =
    match o {
      | Some(a) => a.amount
      | None => 0
    }

  pure def total_allocated(rs: List[Recipient]): Uint128 =
    rs.foldl(0, (acc, r) => acc + alloc_amount(r.allocation))

  pure def remove_at(rs: List[Recipient], idx: int): List[Recipient] =
    rs.foldl((List(), 0), (acc, r) => {
      val pos = acc._2
      if (pos == idx) (acc._1, pos + 1)
      else (acc._1.append(r), pos + 1)
    })._1

  pure def replace_at(rs: List[Recipient], idx: int, new_r: Recipient): List[Recipient] =
    rs.foldl((List(), 0), (acc, r) => {
      val pos = acc._2
      if (pos == idx) (acc._1.append(new_r), pos + 1)
      else (acc._1.append(r), pos + 1)
    })._1

  // Per-record helper: bump a VestingSchedule's amount_withdrawn by `delta`.
  pure def bump_schedule_withdrawn(s: VestingSchedule, delta: Uint128): VestingSchedule = {
    user: s.user,
    week_id: s.week_id,
    mbrn_to_mint: s.mbrn_to_mint,
    amount_withdrawn: s.amount_withdrawn + delta,
    start_time: s.start_time,
    vesting_period: s.vesting_period,
    transmutation_count: s.transmutation_count,
  }

  // Fold across the user's schedule keys, returning (new_map, total_unlocked).
  // For each key: compute unlocked, bump that schedule's amount_withdrawn,
  // and accumulate into the running total.
  pure def withdraw_vested_fold(
      schedules: (Addr, int) -> VestingSchedule,
      user_keys: Set[(Addr, int)],
      now: Timestamp): ((Addr, int) -> VestingSchedule, Uint128) =
    user_keys.fold((schedules, 0), (acc, k) => {
      val cur_map = acc._1
      val running = acc._2
      val s = cur_map.get(k)
      val unlocked = unlocked_amount(s.mbrn_to_mint, s.amount_withdrawn,
                                      s.start_time, s.vesting_period, now)
      val s_new = bump_schedule_withdrawn(s, unlocked)
      (cur_map.put(k, s_new), running + unlocked)
    })

  // ================================================================
  // Execute actions
  // ================================================================

  // --- AddRecipient (contract.rs:797-836) ---
  action AddRecipient(sender: Addr, recipient: Addr): bool = all {
    sender == config.owner,
    find_recipient_idx(recipients, recipient) == -1,
    recipients' = recipients.append({
      recipient: recipient,
      allocation: None,
      claimables: List(),
    }),
    config' = config,
    ownership_transfer' = ownership_transfer,
    vesting_schedules' = vesting_schedules,
    old_mbrn_received' = old_mbrn_received,
    minted_mbrn' = minted_mbrn,
    total_old_mbrn' = total_old_mbrn,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- RemoveRecipient (contract.rs:839-866) ---
  action RemoveRecipient(sender: Addr, recipient: Addr): bool = {
    val idx = find_recipient_idx(recipients, recipient)
    val cur_alloc = if (idx >= 0) recipients[idx].allocation else None
    all {
      sender == config.owner,
      idx >= 0,
      cur_alloc == None,
      recipients' = remove_at(recipients, idx),
      config' = config,
      ownership_transfer' = ownership_transfer,
      vesting_schedules' = vesting_schedules,
      old_mbrn_received' = old_mbrn_received,
      minted_mbrn' = minted_mbrn,
      total_old_mbrn' = total_old_mbrn,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- AddAllocation (contract.rs:630-793) ---
  action AddAllocation(sender: Addr,
                        recipient: Addr,
                        amount: Uint128,
                        vesting_period: Option[VestingPeriod]): bool = {
    val is_owner_path: bool = match vesting_period {
      | Some(_) => true
      | None => false
    }
    val idx = find_recipient_idx(recipients, recipient)
    val existing = if (idx >= 0) recipients[idx]
                    else { recipient: recipient, allocation: None, claimables: List() }
    val existing_alloc = existing.allocation
    val existing_vp: VestingPeriod = match existing_alloc {
      | Some(a) => a.vesting_period
      | None => { cliff_days: 0, linear_days: 0 }
    }
    val vp: VestingPeriod = match vesting_period {
      | Some(v) => v
      | None => existing_vp
    }
    val new_alloc: Allocation = match existing_alloc {
      | Some(a) => { amount: a.amount + amount,
                    amount_withdrawn: a.amount_withdrawn,
                    start_time_of_allocation: a.start_time_of_allocation,
                    vesting_period: a.vesting_period }
      | None => { amount: amount, amount_withdrawn: 0,
                 start_time_of_allocation: block_time,
                 vesting_period: vp }
    }
    val new_recipient: Recipient = {
      recipient: existing.recipient,
      allocation: Some(new_alloc),
      claimables: existing.claimables,
    }
    all {
      if (is_owner_path) sender == config.owner else true,
      total_allocated(recipients) + amount <= config.total_allocation,
      idx >= 0,
      recipients' = replace_at(recipients, idx, new_recipient),
      config' = config,
      ownership_transfer' = ownership_transfer,
      vesting_schedules' = vesting_schedules,
      old_mbrn_received' = old_mbrn_received,
      minted_mbrn' = minted_mbrn,
      total_old_mbrn' = total_old_mbrn,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- WithdrawUnlocked (contract.rs:507-569) ---
  action WithdrawUnlocked(sender: Addr): bool = {
    val idx = find_recipient_idx(recipients, sender)
    val r = if (idx >= 0) recipients[idx]
             else { recipient: sender, allocation: None, claimables: List() }
    val alloc = match r.allocation {
      | Some(a) => a
      | None => EMPTY_ALLOCATION
    }
    val unlocked: Uint128 = unlocked_amount(alloc.amount, alloc.amount_withdrawn,
                                              alloc.start_time_of_allocation,
                                              alloc.vesting_period, block_time)
    val new_alloc: Allocation = {
      amount: alloc.amount,
      amount_withdrawn: alloc.amount_withdrawn + unlocked,
      start_time_of_allocation: alloc.start_time_of_allocation,
      vesting_period: alloc.vesting_period,
    }
    val new_recipient: Recipient = {
      recipient: r.recipient,
      allocation: Some(new_alloc),
      claimables: r.claimables,
    }
    all {
      idx >= 0,
      r.allocation != None,
      unlocked > 0,
      recipients' = replace_at(recipients, idx, new_recipient),
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Mint({
        denom: config.mbrn_denom, to: sender, amount: unlocked
      })),
      config' = config,
      ownership_transfer' = ownership_transfer,
      vesting_schedules' = vesting_schedules,
      old_mbrn_received' = old_mbrn_received,
      minted_mbrn' = minted_mbrn,
      total_old_mbrn' = total_old_mbrn,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- AddVestedTransmutation (contract.rs:869-956) ---
  action AddVestedTransmutation(sender: Addr,
                                 recipient: Addr,
                                 amount_to_mint: Uint128,
                                 vesting_period: VestingPeriod): bool = {
    val auth_ok: bool = match config.neutron_proxy {
      | Some(a) => sender == a
      | None => false
    }
    val week_id: int = block_time / 604800
    val key = (recipient, week_id)
    val has_existing = vesting_schedules.keys().contains(key)
    val existing_schedule: VestingSchedule = if (has_existing)
        vesting_schedules.get(key)
      else
        { user: recipient, week_id: week_id,
          mbrn_to_mint: 0, amount_withdrawn: 0,
          start_time: block_time, vesting_period: vesting_period,
          transmutation_count: 0 }
    val schedule_new: VestingSchedule = if (has_existing)
        { user: existing_schedule.user,
          week_id: existing_schedule.week_id,
          mbrn_to_mint: existing_schedule.mbrn_to_mint + amount_to_mint,
          amount_withdrawn: existing_schedule.amount_withdrawn,
          start_time: existing_schedule.start_time,
          vesting_period: existing_schedule.vesting_period,
          transmutation_count: existing_schedule.transmutation_count + 1 }
      else
        { user: recipient, week_id: week_id,
          mbrn_to_mint: amount_to_mint, amount_withdrawn: 0,
          start_time: block_time, vesting_period: vesting_period,
          transmutation_count: 1 }
    all {
      auth_ok,
      vesting_schedules' = vesting_schedules.put(key, schedule_new),
      old_mbrn_received' = old_mbrn_received + amount_to_mint,
      total_old_mbrn' = total_old_mbrn + amount_to_mint,
      minted_mbrn' = minted_mbrn,
      config' = config, recipients' = recipients,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- WithdrawVestedUnlocked (contract.rs:978-1084) ---
  action WithdrawVestedUnlocked(sender: Addr): bool = {
    val proxy_set: bool = match config.neutron_proxy {
      | Some(_) => true
      | None => false
    }
    val user_keys = vesting_schedules.keys().filter(k => k._1 == sender)
    val folded = withdraw_vested_fold(vesting_schedules, user_keys, block_time)
    val new_schedules = folded._1
    val total_unlocked = folded._2
    all {
      proxy_set,
      user_keys.size() > 0,
      total_unlocked > 0,
      total_unlocked <= total_old_mbrn,
      vesting_schedules' = new_schedules,
      minted_mbrn' = minted_mbrn + total_unlocked,
      total_old_mbrn' = sat_sub(total_old_mbrn, total_unlocked),
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Mint({
        denom: config.mbrn_denom, to: sender, amount: total_unlocked
      })),
      config' = config, recipients' = recipients,
      ownership_transfer' = ownership_transfer,
      old_mbrn_received' = old_mbrn_received,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- ClaimFeesforContract (contract.rs:321-405) — permissionless ---
  // Pure forwarder: dispatches a Staking::ClaimRewards WasmMsg with this
  // contract's address as the recipient, and returns. The cross-organ
  // CrossOrganMsg union has no Msg_Staking_Claim variant; we keep the
  // Msg_Reply placeholder since the mutation it represents (claim accounting)
  // happens entirely inside the staking organ.
  action ClaimFeesforContract(sender: Addr): bool = all {
    msg_queue' = msg_queue.append(Msg_Reply({ id: 0, data: "claim_staking_rewards" })),
    config' = config, recipients' = recipients,
    ownership_transfer' = ownership_transfer,
    vesting_schedules' = vesting_schedules,
    old_mbrn_received' = old_mbrn_received,
    minted_mbrn' = minted_mbrn,
    total_old_mbrn' = total_old_mbrn,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- ClaimFeesforRecipient (contract.rs:263-318) ---
  action ClaimFeesforRecipient(sender: Addr): bool = all {
    find_recipient_idx(recipients, sender) >= 0,
    msg_queue' = msg_queue.append(Msg_BankSend({
      to: sender, denom: "cdt", amount: 0
    })),
    config' = config, recipients' = recipients,
    ownership_transfer' = ownership_transfer,
    vesting_schedules' = vesting_schedules,
    old_mbrn_received' = old_mbrn_received,
    minted_mbrn' = minted_mbrn,
    total_old_mbrn' = total_old_mbrn,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- SubmitProposal (contract.rs:183-222) — recipient-only forwarder ---
  // Forwards Governance::SubmitProposal under the recipient's identity.
  // No local state mutation in Rust. The local recipient-membership gate
  // is the only meaningful precondition; identity-on-state below is
  // intentional.
  action SubmitProposal(sender: Addr,
                         title: str,
                         description: str): bool = all {
    find_recipient_idx(recipients, sender) >= 0,
    msg_queue' = msg_queue.append(Msg_Reply({ id: 0, data: "submit_proposal" })),
    config' = config, recipients' = recipients,
    ownership_transfer' = ownership_transfer,
    vesting_schedules' = vesting_schedules,
    old_mbrn_received' = old_mbrn_received,
    minted_mbrn' = minted_mbrn,
    total_old_mbrn' = total_old_mbrn,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- CastVote (contract.rs:225-259) — recipient-only forwarder ---
  // Same pattern as SubmitProposal: forwards Governance::CastVote with
  // recipient. No local mutations; recipient gate is the entire body.
  action CastVote(sender: Addr, proposal_id: int): bool = all {
    find_recipient_idx(recipients, sender) >= 0,
    msg_queue' = msg_queue.append(Msg_Reply({ id: 0, data: "cast_vote" })),
    config' = config, recipients' = recipients,
    ownership_transfer' = ownership_transfer,
    vesting_schedules' = vesting_schedules,
    old_mbrn_received' = old_mbrn_received,
    minted_mbrn' = minted_mbrn,
    total_old_mbrn' = total_old_mbrn,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- UpdateConfig (contract.rs:446-503) ---
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    sender == config.owner or
            (ownership_transfer != None and Some(sender) == ownership_transfer),
    config' = new_cfg,
    recipients' = recipients,
    ownership_transfer' = ownership_transfer,
    vesting_schedules' = vesting_schedules,
    old_mbrn_received' = old_mbrn_received,
    minted_mbrn' = minted_mbrn,
    total_old_mbrn' = total_old_mbrn,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_total_allocation_under_cap: bool =
    total_allocated(recipients) <= config.total_allocation

  val inv_amount_withdrawn_under_amount: bool =
    recipients.indices().forall(i => match recipients[i].allocation {
      | Some(a) => a.amount_withdrawn <= a.amount
      | None => true
    })

  val inv_schedule_withdrawn_under_total: bool =
    vesting_schedules.keys().forall(k => {
      val s = vesting_schedules.get(k)
      s.amount_withdrawn <= s.mbrn_to_mint
    })

  val inv_transmutation_balance: bool =
    old_mbrn_received >= minted_mbrn and
    sat_sub(old_mbrn_received, minted_mbrn) == total_old_mbrn

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_VS: bool =
    inv_total_allocation_under_cap and
    inv_owner_governance_distinct

  val safety: bool =
    inv_total_allocation_under_cap and
    inv_amount_withdrawn_under_amount and
    inv_schedule_withdrawn_under_total and
    inv_transmutation_balance and
    inv_no_unilateral_capital_loss_path_VS

  action step: bool = all {
    advance_time(1),
    config' = config,
    recipients' = recipients,
    ownership_transfer' = ownership_transfer,
    vesting_schedules' = vesting_schedules,
    old_mbrn_received' = old_mbrn_received,
    minted_mbrn' = minted_mbrn,
    total_old_mbrn' = total_old_mbrn,
  }
}
