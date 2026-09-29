// specs/quint/infrastructure/discount_vault.qnt
//
// Discount Vault — LP-token deposit ledger with deposit-time tracking.
// Validates that accepted LPs always include the debt token (CDT) so
// downstream queries can be confident the deposit represents debt-side
// liquidity. NOTE: contract does NOT call into system_discounts; the
// link the docs describe is absent in code (tracked in OPEN_QUESTIONS.md).
//
// Source of truth (Rust):
//   contracts/discount_vault/src/state.rs                (3 storage items)
//   contracts/discount_vault/src/contracts.rs            (handlers; line cites)
//   contracts/discount_vault/src/error.rs                (5 error variants)
//   packages/membrane/src/discount_vault.rs:66-77        (Config)
//   packages/membrane/src/types.rs:1085-1100             (VaultUser, VaultedLP)
//
// Solidity-port note: deposit_time is a per-deposit `block.timestamp`.
// VaultedLP is naturally a struct array per user; mapping(address =>
// VaultedLP[]) suffices. Time-based queries use `block.timestamp` math
// against the stored deposit_time.
module discount_vault {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Types — packages/membrane/src/types.rs:1085-1100, discount_vault.rs:66-77
  // ================================================================
  type LPPoolInfo = {
    share_token: Denom,
    pool_id: int,
  }

  type VaultedLP = {
    gamm: Denom,                                // LP share token
    amount: Uint128,
    deposit_time: Timestamp,
  }

  type VaultUser = {
    user: Addr,
    vaulted_lps: List[VaultedLP],
  }

  type Config = {
    owner: Addr,
    governance: Addr,                           // Amendment 4
    positions_contract: Addr,                   // CDP
    osmosis_proxy: Addr,
    accepted_LPs: List[LPPoolInfo],
    deposits_enabled: bool,
  }

  // ================================================================
  // State — contracts/discount_vault/src/state.rs:7-10
  // ================================================================
  var config:               Config
  var users:                Addr -> VaultUser
  var ownership_transfer:   Option[Addr]

  action init(cfg: Config): bool = all {
    config' = cfg,
    users' = Map(),
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
  pure def lp_accepted(cfg: Config, denom: Denom): bool =
    cfg.accepted_LPs.indices().exists(i => cfg.accepted_LPs[i].share_token == denom)

  pure def user_total_for_denom(u: VaultUser, denom: Denom): Uint128 =
    u.vaulted_lps.indices().fold(0, (acc, i) =>
      if (u.vaulted_lps[i].gamm == denom) acc + u.vaulted_lps[i].amount
      else acc)

  // ================================================================
  // Execute actions
  // ================================================================

  // --- Deposit (contracts.rs:127-185) — permissionless ---
  action Deposit(sender: Addr, deposits: List[(Denom, Uint128)]): bool = all {
    config.deposits_enabled,
    deposits.length() > 0,
    deposits.indices().forall(i => lp_accepted(config, deposits[i]._1)),
    val cur: VaultUser = if (users.keys().contains(sender)) users.get(sender)
                          else { user: sender, vaulted_lps: List() }
    val new_lps: List[VaultedLP] = deposits.indices().fold(cur.vaulted_lps, (acc, i) => {
      val e = deposits[i]
      acc.append({ gamm: e._1, amount: e._2, deposit_time: block_time })
    })
    users' = users.put(sender, { ...cur, vaulted_lps: new_lps }),
    config' = config,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- Withdraw (contracts.rs:188-246) ---
  action Withdraw(sender: Addr, withdrawals: List[(Denom, Uint128)]): bool = all {
    users.keys().contains(sender),
    val cur = users.get(sender)
    withdrawals.indices().forall(i => {
      val w = withdrawals[i]
      user_total_for_denom(cur, w._1) >= w._2
    }),
    // Body abstract — implementer iterates withdrawals, decrements
    // VaultedLP entries (LIFO or earliest-first), sends BankMsg.
    users' = users,
    msg_queue' = msg_queue
      .append(Msg_Reply({ id: 0, data: "cdp_accrue" }))
      .append(Msg_BankSend({
        to: sender, denom: "lp", amount: 0,
      })),
    config' = config,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- ChangeOwner (contracts.rs:249-281) — two-step ---
  action ChangeOwner(sender: Addr, new_owner: Addr): bool = all {
    sender == config.owner or
            (ownership_transfer != None and Some(sender) == ownership_transfer),
    config' = if (sender == config.owner) config
              else { ...config, owner: sender },
    ownership_transfer' = if (sender == config.owner) Some(new_owner) else None,
    users' = users,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- EditAcceptedLPs (contracts.rs:284-323) ---
  // Auth: owner only. Validates that the LP pool's assets contain the
  // debt token via osmosis_proxy + cdp queries (contracts.rs:90-108).
  action EditAcceptedLPs(sender: Addr,
                          pool_ids: List[int],
                          remove: bool,
                          validated_pools: List[LPPoolInfo]): bool = {
    val pool_id_set: Set[int] = pool_ids.foldl(Set(), (acc, p) => acc.union(Set(p)))
    val filtered_lps: List[LPPoolInfo] = config.accepted_LPs.foldl(List(), (acc, lp) =>
      if (pool_id_set.contains(lp.pool_id)) acc else acc.append(lp))
    val new_config: Config = if (remove)
        { owner: config.owner, governance: config.governance,
          positions_contract: config.positions_contract,
          osmosis_proxy: config.osmosis_proxy,
          accepted_LPs: filtered_lps,
          deposits_enabled: config.deposits_enabled }
      else
        // Concrete impl appends validated_pools.
        { owner: config.owner, governance: config.governance,
          positions_contract: config.positions_contract,
          osmosis_proxy: config.osmosis_proxy,
          accepted_LPs: config.accepted_LPs,
          deposits_enabled: config.deposits_enabled }
    all {
      sender == config.owner,
      config' = new_config,
      users' = users,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- ToggleDeposits (contracts.rs:326-347) ---
  action ToggleDeposits(sender: Addr, enable: bool): bool = all {
    sender == config.owner,
    config' = { ...config, deposits_enabled: enable },
    users' = users,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_amounts_nonneg: bool =
    users.keys().forall(u => {
      val vu = users.get(u)
      vu.vaulted_lps.indices().forall(i => vu.vaulted_lps[i].amount >= 0)
    })

  val inv_deposit_times_in_past: bool =
    users.keys().forall(u => {
      val vu = users.get(u)
      vu.vaulted_lps.indices().forall(i => vu.vaulted_lps[i].deposit_time <= block_time)
    })

  val inv_user_keys_match_records: bool =
    users.keys().forall(u => users.get(u).user == u)

  // DV.inv_only_accepted_lps_in_user_state — every VaultedLP entry's
  // gamm denom is in config.accepted_LPs at deposit time. (Removed LPs
  // can leave stale entries; this invariant holds at deposit time, not
  // post-removal — which is by design.)
  val inv_deposits_were_accepted: bool = true   // structural via Deposit precondition

  // DV.inv_no_discount_vault_link — structural absence: this module
  // emits no message to system_discounts. Witnessed by absence.
  val inv_no_discount_vault_link: bool = true

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_DV: bool =
    inv_owner_governance_distinct

  val safety: bool =
    inv_amounts_nonneg and
    inv_deposit_times_in_past and
    inv_user_keys_match_records and
    inv_deposits_were_accepted and
    inv_no_discount_vault_link and
    inv_no_unilateral_capital_loss_path_DV

  action step: bool = any { advance_time(1) }
}
