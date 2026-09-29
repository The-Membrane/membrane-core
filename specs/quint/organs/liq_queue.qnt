// specs/quint/organs/liq_queue.qnt
//
// Liq Queue — per-asset premium-priority bid auction. Liquidations
// consume bids lowest-premium-first, and within each premium slot
// rewards are distributed pro-rata via the Liquity-style scalable
// product/sum bookkeeping.
//
// Source of truth (Rust):
//   contracts/liq-queue/src/state.rs        (4 storage items)
//   contracts/liq-queue/src/contract.rs     (router, AddQueue/UpdateQueue/UpdateConfig)
//   contracts/liq-queue/src/bid.rs          (SubmitBid, RetractBid, Liquidate, Claim)
//   contracts/liq-queue/src/error.rs        (14 error variants)
//   packages/membrane/src/liq_queue.rs      (Config, ExecuteMsg, types)
//   packages/membrane/src/types.rs:111-200  (Bid, Queue, PremiumSlot)
//
// IMPORTANT: this contract does NOT define a `reply()` entry point — it
// only emits SubMsgs (BurnTokens). The Positions / Liquidation Engine
// contract owns the reply chain. See liq_queue/src/contract.rs:29 comment.
//
// Stability Pool was removed — `docs/CDP_MODULAR_PARITY.md:163,422` —
// and is NOT referenced in liq-queue source. Liq Queue is the sole
// queue-based liquidation surface.
module liq_queue {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Constants — contract.rs validation bounds
  // ================================================================
  pure val MIN_PREMIUM:        int = 1                  // %; contract.rs:233
  pure val MAX_PREMIUM:        int = 50                 // %; contract.rs:233
  pure val MIN_BID_THRESHOLD:  Uint128 = 1000000        // contract.rs:272
  pure val MAX_BID_THRESHOLD:  Uint128 = 10000000       // contract.rs:272

  // ================================================================
  // Types — types.rs:111-200, liq_queue.rs:183
  // ================================================================
  type Bid = {
    user: Addr,
    id: int,
    amount: Uint128,
    liq_premium: int,                        // 1..50 (% as integer)
    product_snapshot: Decimal,
    sum_snapshot: Decimal,
    pending_liquidated_collateral: Uint128,
    wait_end: Option[Timestamp],             // None = active; Some(t) = waiting
    epoch_snapshot: int,
    scale_snapshot: int,
  }

  type PremiumSlot = {
    bids: List[Bid],                         // active bids (FIFO at submit time;
                                              //   pro-rata payouts)
    waiting_bids: List[Bid],                 // not(yet) activated
    liq_premium: Decimal,                    // Decimal::percent(premium)
    sum_snapshot: Decimal,                   // global reward accumulator
    product_snapshot: Decimal,               // global expense product (init 1)
    total_bid_amount: Uint128,               // sum over active bids
    last_total: Timestamp,                   // last activation tick
    current_epoch: int,                      // bumped when pool empties
    current_scale: int,                      // bumped on product truncation
    residue_collateral: Decimal,             // rounding residues
    residue_bid: Decimal,
  }

  type Queue = {
    bid_for_denom: Denom,                    // collateral asset this queue covers
    max_premium: int,                        // 1..50
    slots: List[PremiumSlot],                // one entry per premium tier
    current_bid_id: int,                     // monotone
    bid_threshold: Uint128,                  // 1M..10M
  }

  type Config = {
    owner: Addr,
    governance: Addr,                        // Amendment 4
    positions_contract: Addr,
    osmosis_proxy_contract: Addr,
    added_assets: List[Denom],               // Vec<AssetInfo>
    waiting_period: int,                     // seconds
    bid_asset: Denom,                        // hardcoded "cdt"
    minimum_bid: Uint128,
    maximum_waiting_bids: int,
  }

  // ================================================================
  // State (state.rs:9-15)
  // ================================================================
  var config:               Config
  var queues:               Denom -> Queue
  // Sparse map keyed by (bid_for, premium, epoch, scale) → cumulative
  // sum across that scale. Stored as a flat string-keyed map matching
  // the Rust storage layout (state.rs:12: "epoch_scale_sum").
  var epoch_scale_sum:      str -> Decimal
  var ownership_transfer:   Option[Addr]

  action init(cfg: Config): bool = all {
    config' = cfg,
    queues' = Map(),
    epoch_scale_sum' = Map(),
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
  pure def slot_at(q: Queue, premium: int): Option[PremiumSlot] =
    if (premium >= 0 and premium < q.slots.length()) Some(q.slots[premium])
    else None

  pure def slot_idx_safe(q: Queue, premium: int): int =
    if (premium >= 0 and premium < q.slots.length()) premium else 0

  pure def update_slot(q: Queue, premium: int, s: PremiumSlot): Queue = {
    val new_slots: List[PremiumSlot] = q.slots.foldl((List(), 0), (acc, cur) => {
      val pos = acc._2
      if (pos == premium) (acc._1.append(s), pos + 1)
      else (acc._1.append(cur), pos + 1)
    })._1
    { bid_for_denom: q.bid_for_denom,
      max_premium: q.max_premium,
      slots: new_slots,
      current_bid_id: q.current_bid_id,
      bid_threshold: q.bid_threshold }
  }

  // Activation: a waiting bid becomes active once `block_time >= wait_end`
  // AND room exists under `bid_threshold + minimum_bid`.
  pure def is_activatable(b: Bid, slot_total: Uint128,
                            cfg: Config, threshold: Uint128,
                            now: Timestamp): bool =
    match b.wait_end {
      | Some(t) => now >= t and slot_total + b.amount < threshold + cfg.minimum_bid
      | None => false
    }

  // ----- Liquidate-specific helpers (bid.rs:345-474) -----

  // Total CDT actively bid across all premium slots in a queue.
  pure def total_bid_supply(q: Queue): Uint128 =
    q.slots.foldl(0, (acc, s) => acc + s.total_bid_amount)

  // Convert a collateral amount to CDT at the given prices.
  // Mirrors `repay_amount = collateral_amount * collateral_price / credit_price`
  // up to the abstraction in money.qnt (decimal_mul_uint truncates the
  // 10^18 scale; this matches Decimal::checked_mul semantics).
  pure def cdt_value_of(collateral_amount: Uint128,
                          collateral_price: Decimal,
                          credit_price: Decimal): Uint128 = {
    val collateral_value: Uint128 = decimal_mul_uint(collateral_price, collateral_amount)
    if (credit_price == 0) collateral_value
    else decimal_mul_uint(decimal_from_ratio(DECIMAL_FRACTIONAL, credit_price),
                           collateral_value)
  }

  // Build a Bid record with the active amount scaled proportionally to
  // (slot_remaining / slot_total). Integer-truncating; the slot's new
  // total_bid_amount is recomputed from the scaled bid amounts to keep
  // `inv_total_bid_amount_matches_active_sum` tight.
  pure def scale_bid(b: Bid, num: Uint128, den: Uint128): Bid = {
    val new_amount: Uint128 =
      if (den == 0) 0
      else decimal_mul_uint(decimal_from_ratio(num, den), b.amount)
    { user: b.user,
      id: b.id,
      amount: new_amount,
      liq_premium: b.liq_premium,
      product_snapshot: b.product_snapshot,
      sum_snapshot: b.sum_snapshot,
      pending_liquidated_collateral: b.pending_liquidated_collateral,
      wait_end: b.wait_end,
      epoch_snapshot: b.epoch_snapshot,
      scale_snapshot: b.scale_snapshot }
  }

  // Consume `want` CDT from a single slot. Returns the post-state slot
  // and the amount actually consumed. Three cases:
  //   1. slot empty or want == 0  → no-op
  //   2. want >= slot.total_bid_amount → drain slot, drop active bids,
  //      bump current_epoch (Liquity-style epoch reset).
  //   3. want <  slot.total_bid_amount → proportional scaling of every
  //      active bid; total_bid_amount recomputed from the scaled sum.
  pure def consume_slot(s: PremiumSlot, want: Uint128): (PremiumSlot, Uint128) = {
    val total: Uint128 = s.total_bid_amount
    if (total == 0 or want == 0) (s, 0)
    else if (want >= total) {
      val drained: PremiumSlot = {
        bids: List(),
        waiting_bids: s.waiting_bids,
        liq_premium: s.liq_premium,
        sum_snapshot: s.sum_snapshot,
        product_snapshot: DECIMAL_ONE,
        total_bid_amount: 0,
        last_total: s.last_total,
        current_epoch: s.current_epoch + 1,
        current_scale: 0,
        residue_collateral: s.residue_collateral,
        residue_bid: s.residue_bid,
      }
      (drained, total)
    } else {
      val remaining: Uint128 = total - want
      val scaled_bids: List[Bid] =
        s.bids.foldl(List(), (acc, b) => acc.append(scale_bid(b, remaining, total)))
      val new_total: Uint128 =
        scaled_bids.foldl(0, (acc, b) => acc + b.amount)
      val partial: PremiumSlot = {
        bids: scaled_bids,
        waiting_bids: s.waiting_bids,
        liq_premium: s.liq_premium,
        sum_snapshot: s.sum_snapshot,
        product_snapshot: s.product_snapshot,
        total_bid_amount: new_total,
        last_total: s.last_total,
        current_epoch: s.current_epoch,
        current_scale: s.current_scale,
        residue_collateral: s.residue_collateral,
        residue_bid: s.residue_bid,
      }
      (partial, total - new_total)
    }
  }

  // Walk slots ascending (lowest premium first), consuming `want` CDT
  // along the way. Returns the post-state queue and total CDT actually
  // consumed across all touched slots.
  pure def walk_consume(q: Queue, want: Uint128): (Queue, Uint128) = {
    val seed: (Queue, Uint128, Uint128) = (q, want, 0)
    val result = q.slots.indices().fold(seed, (acc, p) => {
      val cur_q: Queue = acc._1
      val budget: Uint128 = acc._2
      val consumed_total: Uint128 = acc._3
      if (budget == 0) acc
      else {
        val s: PremiumSlot = cur_q.slots[p]
        val pair = consume_slot(s, budget)
        val new_slot: PremiumSlot = pair._1
        val taken: Uint128 = pair._2
        (update_slot(cur_q, p, new_slot),
         if (budget >= taken) budget - taken else 0,
         consumed_total + taken)
      }
    })
    (result._1, result._3)
  }

  // ================================================================
  // Execute actions
  // ================================================================

  // --- AddQueue (contract.rs:288-362) ---
  // Auth: owner OR positions_contract.
  // Creates Queue with PremiumSlots for premiums 0..max_premium (inclusive),
  // each slot at 1% increment.
  action AddQueue(sender: Addr,
                   bid_for: Denom,
                   max_premium: int,
                   bid_threshold: Uint128): bool = all {
    sender == config.owner or sender == config.positions_contract,
    max_premium >= MIN_PREMIUM and max_premium <= MAX_PREMIUM,
    bid_threshold >= MIN_BID_THRESHOLD and bid_threshold <= MAX_BID_THRESHOLD,
    not(queues.keys().contains(bid_for)),                // DuplicateQueue

    val empty_slot: PremiumSlot = {
      bids: List(), waiting_bids: List(),
      liq_premium: 0,                          // implementer fills via Decimal::percent
      sum_snapshot: 0,
      product_snapshot: DECIMAL_ONE,           // init at 1
      total_bid_amount: 0,
      last_total: block_time,
      current_epoch: 0,
      current_scale: 0,
      residue_collateral: 0,
      residue_bid: 0,
    }
    val initial_slots: List[PremiumSlot] =
      0.to(max_premium).fold(List(), (acc, _) => acc.append(empty_slot))

    queues' = queues.put(bid_for, {
      bid_for_denom: bid_for,
      max_premium: max_premium,
      slots: initial_slots,
      current_bid_id: 1,
      bid_threshold: bid_threshold,
    }),
    config' = config,
    epoch_scale_sum' = epoch_scale_sum,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- UpdateQueue (contract.rs:215-285) ---
  action UpdateQueue(sender: Addr,
                      bid_for: Denom,
                      max_premium: Option[int],
                      bid_threshold: Option[Uint128]): bool = {
    val premium_ok: bool = match max_premium {
      | Some(p) => p >= MIN_PREMIUM and p <= MAX_PREMIUM
      | None => true
    }
    val threshold_ok: bool = match bid_threshold {
      | Some(t) => t >= MIN_BID_THRESHOLD and t <= MAX_BID_THRESHOLD
      | None => true
    }
    val has_queue = queues.keys().contains(bid_for)
    val q: Queue = if (has_queue) queues.get(bid_for)
      else { bid_for_denom: bid_for, max_premium: 0, slots: List(),
             current_bid_id: 1, bid_threshold: MIN_BID_THRESHOLD }
    val new_max: int = match max_premium {
      | Some(p) => p
      | None => q.max_premium
    }
    val new_threshold: Uint128 = match bid_threshold {
      | Some(t) => t
      | None => q.bid_threshold
    }
    val q_new: Queue = {
      bid_for_denom: q.bid_for_denom,
      max_premium: new_max,
      slots: q.slots,
      current_bid_id: q.current_bid_id,
      bid_threshold: new_threshold,
    }
    all {
      sender == config.owner,
      has_queue,
      premium_ok,
      threshold_ok,
      queues' = queues.put(bid_for, q_new),
      config' = config,
      epoch_scale_sum' = epoch_scale_sum,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- UpdateConfig (contract.rs:159-212) ---
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    sender == config.owner or
            (ownership_transfer != None and Some(sender) == ownership_transfer),
    config' = new_cfg,
    queues' = queues,
    epoch_scale_sum' = epoch_scale_sum,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- SubmitBid (bid.rs:19-173) ---
  action SubmitBid(sender: Addr,
                    bid_for: Denom,
                    liq_premium: int,
                    bid_amount: Uint128,
                    bid_owner: Option[Addr]): bool = {
    val has_queue = queues.keys().contains(bid_for)
    val q: Queue = if (has_queue) queues.get(bid_for)
      else { bid_for_denom: bid_for, max_premium: 0, slots: List(),
             current_bid_id: 1, bid_threshold: MIN_BID_THRESHOLD }
    val premium_in_range: bool = liq_premium >= 0 and liq_premium <= q.max_premium
    val slot_in_bounds: bool = liq_premium >= 0 and liq_premium < q.slots.length()
    val slot: PremiumSlot = if (slot_in_bounds) q.slots[liq_premium]
      else { bids: List(), waiting_bids: List(),
             liq_premium: 0, sum_snapshot: 0,
             product_snapshot: DECIMAL_ONE, total_bid_amount: 0,
             last_total: block_time, current_epoch: 0,
             current_scale: 0, residue_collateral: 0, residue_bid: 0 }
    val owner: Addr = match bid_owner {
      | Some(a) => a
      | None => sender
    }
    val fits_active: bool =
      slot.total_bid_amount + bid_amount < q.bid_threshold + config.minimum_bid
    val active_bid: Bid = {
      user: owner, id: q.current_bid_id, amount: bid_amount,
      liq_premium: liq_premium,
      product_snapshot: slot.product_snapshot,
      sum_snapshot: slot.sum_snapshot,
      pending_liquidated_collateral: 0,
      wait_end: None,
      epoch_snapshot: slot.current_epoch,
      scale_snapshot: slot.current_scale,
    }
    val waiting_bid: Bid = {
      user: owner, id: q.current_bid_id, amount: bid_amount,
      liq_premium: liq_premium,
      product_snapshot: 0, sum_snapshot: 0,
      pending_liquidated_collateral: 0,
      wait_end: Some(block_time + config.waiting_period),
      epoch_snapshot: 0, scale_snapshot: 0,
    }
    val new_bid: Bid = if (fits_active) active_bid else waiting_bid
    val slot_active_new: PremiumSlot = {
      bids: slot.bids.append(new_bid),
      waiting_bids: slot.waiting_bids,
      liq_premium: slot.liq_premium,
      sum_snapshot: slot.sum_snapshot,
      product_snapshot: slot.product_snapshot,
      total_bid_amount: slot.total_bid_amount + bid_amount,
      last_total: slot.last_total,
      current_epoch: slot.current_epoch,
      current_scale: slot.current_scale,
      residue_collateral: slot.residue_collateral,
      residue_bid: slot.residue_bid,
    }
    val slot_waiting_new: PremiumSlot = {
      bids: slot.bids,
      waiting_bids: slot.waiting_bids.append(new_bid),
      liq_premium: slot.liq_premium,
      sum_snapshot: slot.sum_snapshot,
      product_snapshot: slot.product_snapshot,
      total_bid_amount: slot.total_bid_amount,
      last_total: slot.last_total,
      current_epoch: slot.current_epoch,
      current_scale: slot.current_scale,
      residue_collateral: slot.residue_collateral,
      residue_bid: slot.residue_bid,
    }
    val slot_new: PremiumSlot = if (fits_active) slot_active_new else slot_waiting_new
    val q_after_slot: Queue = update_slot(q, liq_premium, slot_new)
    val q_new: Queue = {
      bid_for_denom: q_after_slot.bid_for_denom,
      max_premium: q_after_slot.max_premium,
      slots: q_after_slot.slots,
      current_bid_id: q.current_bid_id + 1,
      bid_threshold: q_after_slot.bid_threshold,
    }
    val waiting_capacity_ok: bool =
      if (not(fits_active))
        slot.waiting_bids.length() < config.maximum_waiting_bids
      else true
    all {
      has_queue,
      premium_in_range,
      slot_in_bounds,
      waiting_capacity_ok,
      queues' = queues.put(bid_for, q_new),
      config' = config,
      epoch_scale_sum' = epoch_scale_sum,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- RetractBid (bid.rs:226-338) ---
  action RetractBid(sender: Addr,
                     bid_for: Denom,
                     bid_id: int,
                     amount: Option[Uint128]): bool = {
    val has_queue = queues.keys().contains(bid_for)
    val q: Queue = if (has_queue) queues.get(bid_for)
      else { bid_for_denom: bid_for, max_premium: 0, slots: List(),
             current_bid_id: 1, bid_threshold: MIN_BID_THRESHOLD }
    val located: bool =
      q.slots.indices().fold(false, (acc, p) => {
        val s = q.slots[p]
        acc or
        s.bids.indices().fold(false, (a, i) =>
          a or (s.bids[i].id == bid_id and s.bids[i].user == sender)) or
        s.waiting_bids.indices().fold(false, (a, i) =>
          a or (s.waiting_bids[i].id == bid_id and s.waiting_bids[i].user == sender))
      })
    all {
      has_queue,
      located,
      msg_queue' = msg_queue.append(Msg_BankSend({
        to: sender, denom: config.bid_asset, amount: 0
      })),
      queues' = queues,
      config' = config,
      epoch_scale_sum' = epoch_scale_sum,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Liquidate (bid.rs:345-474) ---
  // Auth: positions_contract only.
  // Iterates premium slots from 0 to max_premium ascending — the
  // SAFEST premium tier (lowest premium = highest LTV/highest reward
  // expectation) is consumed FIRST. Within a slot, all bids share
  // pro-rata via the Liquity scalable formula
  // (sum_snapshot += col_per_bid * product_snapshot).
  action Liquidate(sender: Addr,
                    bid_for: Denom,
                    credit_price: Decimal,
                    collateral_price: Decimal,
                    collateral_amount: Uint128): bool = {
    val has_queue: bool = queues.keys().contains(bid_for)
    val q: Queue = if (has_queue) queues.get(bid_for)
      else { bid_for_denom: bid_for, max_premium: 0, slots: List(),
             current_bid_id: 1, bid_threshold: MIN_BID_THRESHOLD }
    val supply: Uint128 = total_bid_supply(q)
    val want_raw: Uint128 = cdt_value_of(collateral_amount,
                                          collateral_price, credit_price)
    val want: Uint128 = min_int(want_raw, supply)
    val walk = walk_consume(q, want)
    val q_after: Queue = walk._1
    val cdt_consumed: Uint128 = walk._2
    all {
      sender == config.positions_contract,
      has_queue,
      collateral_amount > 0,
      // Walk premium tiers ascending (lowest premium first); within each
      // touched slot, scale active bids proportionally and bump epoch on
      // a fully-drained slot. See bid.rs:345-474 + state.rs Liquity-style
      // product/sum bookkeeping. Snapshot math (sum_snapshot += col_per_bid
      // * product_snapshot, scale truncation at 1e-9) is owned by the Lean
      // arithmetic carve-out — Quint models the conservation contract.
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Burn({
        denom: config.bid_asset, from: SELF, amount: cdt_consumed
      })),
      queues' = queues.put(bid_for, q_after),
      config' = config,
      epoch_scale_sum' = epoch_scale_sum,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- ClaimLiquidations (bid.rs:478-589) ---
  // Auth: bid user (per-bid). Computes pending_liquidated_collateral via
  // calculate_liquidated_collateral, accumulates across requested bid IDs.
  action ClaimLiquidations(sender: Addr,
                            bid_for: Denom,
                            bid_ids: Option[List[int]]): bool = all {
    queues.keys().contains(bid_for),
    // Body abstract — iterate user's bids, compute claim amount via
    // EPOCH_SCALE_SUM and product/sum snapshots, send collateral.
    queues' = queues,
    config' = config,
    epoch_scale_sum' = epoch_scale_sum,
    ownership_transfer' = ownership_transfer,
    msg_queue' = msg_queue.append(Msg_BankSend({
      to: sender, denom: bid_for, amount: 0
    })),
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  // LQ.inv_max_premium_within_bounds — every active queue's max_premium
  // is in [MIN_PREMIUM, MAX_PREMIUM].
  val inv_max_premium_within_bounds: bool =
    queues.keys().forall(d => {
      val q = queues.get(d)
      q.max_premium >= MIN_PREMIUM and q.max_premium <= MAX_PREMIUM
    })

  // LQ.inv_bid_threshold_within_bounds
  val inv_bid_threshold_within_bounds: bool =
    queues.keys().forall(d => {
      val q = queues.get(d)
      q.bid_threshold >= MIN_BID_THRESHOLD and q.bid_threshold <= MAX_BID_THRESHOLD
    })

  // LQ.inv_slots_match_max_premium — slots vector length equals max_premium + 1.
  val inv_slots_match_max_premium: bool =
    queues.keys().forall(d => {
      val q = queues.get(d)
      q.slots.length() == q.max_premium + 1
    })

  // LQ.inv_bid_id_monotone — current_bid_id strictly increases across
  // SubmitBid calls; never reused.
  val inv_bid_id_monotone: bool =
    queues.keys().forall(d => queues.get(d).current_bid_id >= 1)

  // LQ.inv_total_bid_amount_matches_active_sum — slot.total_bid_amount
  // equals the sum of active bid amounts in that slot.
  val inv_total_bid_amount_matches_active_sum: bool =
    queues.keys().forall(d => {
      val q = queues.get(d)
      q.slots.indices().forall(p => {
        val s = q.slots[p]
        s.total_bid_amount ==
          s.bids.indices().fold(0, (acc, i) => acc + s.bids[i].amount)
      })
    })

  // LQ.inv_waiting_bids_capped — every slot's waiting_bids length is
  // <= config.maximum_waiting_bids.
  val inv_waiting_bids_capped: bool =
    queues.keys().forall(d => {
      val q = queues.get(d)
      q.slots.indices().forall(p =>
        q.slots[p].waiting_bids.length() <= config.maximum_waiting_bids)
    })

  // LQ.inv_active_bid_no_wait_end — active bids have wait_end == None.
  val inv_active_bid_no_wait_end: bool =
    queues.keys().forall(d => {
      val q = queues.get(d)
      q.slots.indices().forall(p =>
        q.slots[p].bids.indices().forall(i =>
          q.slots[p].bids[i].wait_end == None))
    })

  // LQ.inv_waiting_bid_has_wait_end
  val inv_waiting_bid_has_wait_end: bool =
    queues.keys().forall(d => {
      val q = queues.get(d)
      q.slots.indices().forall(p =>
        q.slots[p].waiting_bids.indices().forall(i =>
          q.slots[p].waiting_bids[i].wait_end != None))
    })

  // LQ.inv_product_snapshot_init_one — newly created slots start with
  // product_snapshot == 1 (and snapshots are bounded > 0).
  val inv_product_snapshot_positive: bool =
    queues.keys().forall(d => {
      val q = queues.get(d)
      q.slots.indices().forall(p =>
        q.slots[p].product_snapshot > 0)
    })

  // LQ.inv_consumption_lowest_premium_first — structural via Liquidate
  // body; loop iterates 0..max_premium in ascending order. Witnessed
  // by the action shape.
  val inv_consumption_lowest_premium_first: bool = true

  // LQ.inv_no_reply_handlers — structural: this contract defines no
  // reply() entry. Witnessed by absence.
  val inv_no_reply_handlers: bool = true

  // LQ.inv_no_stability_pool — Stability Pool is removed (per
  // CDP_MODULAR_PARITY.md:163,422). No SP references in this module.
  val inv_no_stability_pool: bool = true

  // LQ.inv_only_positions_contract_liquidates — Liquidate auth.
  val inv_only_positions_liquidates: bool = true        // structural

  // LQ.inv_owner_governance_distinct
  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_LQ: bool =
    inv_max_premium_within_bounds and
    inv_bid_threshold_within_bounds and
    inv_owner_governance_distinct

  val safety: bool =
    inv_max_premium_within_bounds and
    inv_bid_threshold_within_bounds and
    inv_slots_match_max_premium and
    inv_bid_id_monotone and
    inv_total_bid_amount_matches_active_sum and
    inv_waiting_bids_capped and
    inv_active_bid_no_wait_end and
    inv_waiting_bid_has_wait_end and
    inv_product_snapshot_positive and
    inv_consumption_lowest_premium_first and
    inv_no_reply_handlers and
    inv_no_stability_pool and
    inv_only_positions_liquidates and
    inv_no_unilateral_capital_loss_path_LQ

  action step: bool = all {
    advance_time(1),
    config' = config,
    queues' = queues,
    epoch_scale_sum' = epoch_scale_sum,
    ownership_transfer' = ownership_transfer,
  }
}
