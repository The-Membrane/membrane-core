// specs/quint/infrastructure/osmosis_proxy.qnt
//
// Osmosis Proxy — multi-owner gated mint/burn surface for tokenfactory
// denoms, plus DEX swap routing (multihop GAMM/concentrated pools) and
// gauge creation passthrough to Osmosis incentives module.
//
// Source of truth (Rust):
//   contracts/osmosis-proxy/src/state.rs        (7 storage items)
//   contracts/osmosis-proxy/src/contract.rs     (handlers; line cites)
//   contracts/osmosis-proxy/src/error.rs        (error variants)
//   packages/membrane/src/osmosis_proxy.rs:151-168 (Config)
//
// Reply IDs (contract.rs):
//   CREATE_DENOM_REPLY_ID    = 1
//   SWAP_REPLY_ID            = 2
//   USE_BALANCE_SWAP_REPLY_ID = 3
//
// Solidity-port note: TokenFactory bindings translate to OZ ERC20Mintable;
// the Owner.non_token_contract_auth flag becomes a separate role in OZ
// AccessControl. ExecuteSwaps is permissionless on Cosmos but on EVM it
// should require the caller to send the input tokens via approve+transferFrom.
module osmosis_proxy {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Reply IDs (contract.rs)
  // ================================================================
  pure val CREATE_DENOM_REPLY_ID:    int = 1
  pure val SWAP_REPLY_ID:            int = 2
  pure val USE_BALANCE_SWAP_REPLY_ID: int = 3

  // ================================================================
  // Types — packages/membrane/src/types.rs:1023-1034, state.rs:10-34
  // ================================================================
  type Owner = {
    owner: Addr,
    total_minted: Uint128,
    stability_pool_ratio: Option[Decimal],
    non_token_contract_auth: bool,             // gates UpdateConfig + EditOwner
    is_position_contract: bool,
  }

  type TokenInfo = {
    current_supply: Uint128,
    max_supply: Option[Uint128],
    burned_supply: Uint128,
  }

  type PendingTokenInfo = {
    subdenom: str,
    max_supply: Option[Uint128],
  }

  type SwapAmountInRoute = {
    pool_id: int,
    token_out_denom: Denom,
  }

  type SwapRoute = {
    token_in: Denom,
    token_out: Denom,
    route_out: SwapAmountInRoute,
  }

  type SwapInfo = {
    swapper: Addr,
    prev_balances: List[(Denom, Uint128)],
    token_out: Denom,
    max_slippage: Decimal,
  }

  type TransmutationPair = {
    token_in: Denom,
    token_to_mint: Denom,
    mint_ratio: Decimal,
  }

  type SupplySnapshot = {
    timestamp: Timestamp,
    current_supply: Uint128,
    burned_supply: Uint128,
  }

  type Config = {
    owners: List[Owner],
    liquidity_multiplier: Option[Decimal],
    debt_auction: Option[Addr],                 // MBRN mint override
    positions_contract: Option[Addr],
    liquidity_contract: Option[Addr],
    oracle_contract: Option[Addr],
    restrict_mbrn_mints: Option[bool],
  }

  // ================================================================
  // State — contracts/osmosis-proxy/src/state.rs:36-54
  // ================================================================
  var config:               Config
  var tokens:               Denom -> TokenInfo
  var pending_denoms:       Option[PendingTokenInfo]    // single Item
  var swap_routes:          List[SwapRoute]
  var swap_info:            Option[SwapInfo]
  var transmutation_pairs:  List[TransmutationPair]
  var supply_history:       Denom -> List[SupplySnapshot]

  action init(cfg: Config): bool = all {
    cfg.owners.length() >= 1,
    config' = cfg,
    tokens' = Map(),
    pending_denoms' = None,
    swap_routes' = List(),
    swap_info' = None,
    transmutation_pairs' = List(),
    supply_history' = Map(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers — contract.rs:597-607 (validate_authority)
  // ================================================================
  pure def find_owner(owners: List[Owner], addr: Addr): Option[Owner] =
    owners.foldl(None, (acc, o) =>
      match acc {
        | Some(_) => acc
        | None => if (o.owner == addr) Some(o) else None
      })

  pure def is_owner(cfg: Config, sender: Addr): bool = find_owner(cfg.owners, sender) != None

  pure def has_non_token_auth(cfg: Config, sender: Addr): bool = match find_owner(cfg.owners, sender) {
      | Some(o) => o.non_token_contract_auth
      | None => false
    }

  // ================================================================
  // Execute actions
  // ================================================================

  // --- CreateDenom (contract.rs:612-645) ---
  // Auth: any owner. Stages PendingTokenInfo, emits TokenFactory msg.
  action CreateDenom(sender: Addr,
                      subdenom: str,
                      max_supply: Option[Uint128]): bool = all {
    is_owner(config, sender),
    subdenom != "",
    pending_denoms' = Some({ subdenom: subdenom, max_supply: max_supply }),
    msg_queue' = msg_queue.append(Msg_Reply({
      id: CREATE_DENOM_REPLY_ID, data: subdenom
    })),
    config' = config, tokens' = tokens,
    swap_routes' = swap_routes, swap_info' = swap_info,
    transmutation_pairs' = transmutation_pairs,
    supply_history' = supply_history,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- MintTokens (contract.rs:727-833) ---
  // Auth: any owner. MBRN restriction: if `restrict_mbrn_mints && denom contains "mbrn"`,
  // only debt_auction may call (contract.rs:753-761).
  action MintTokens(sender: Addr,
                     denom: Denom,
                     amount: Uint128,
                     mint_to: Addr): bool = {
    val mbrn_path: bool = (match config.restrict_mbrn_mints {
      | Some(b) => b
      | None => false
    }) and contains_mbrn(denom)
    val is_debt_auction: bool = match config.debt_auction {
      | Some(a) => sender == a
      | None => false
    }
    val ti = if (tokens.keys().contains(denom)) tokens.get(denom)
             else { current_supply: 0, max_supply: None, burned_supply: 0 }
    val new_supply = ti.current_supply + amount
    all {
      is_owner(config, sender),
      amount > 0,
      not(mbrn_path) or is_debt_auction,
      tokens.keys().contains(denom),
      match ti.max_supply {
        | Some(cap) => new_supply <= cap or is_debt_auction
        | None => true
      },
      tokens' = tokens.put(denom, { current_supply: new_supply,
                                     max_supply: ti.max_supply,
                                     burned_supply: ti.burned_supply }),
      supply_history' = supply_history.put(denom,
        (if (supply_history.keys().contains(denom)) supply_history.get(denom) else List())
          .append({ timestamp: block_time,
                     current_supply: new_supply,
                     burned_supply: ti.burned_supply })),
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Mint({
        denom: denom, to: mint_to, amount: amount
      })),
      config' = config, pending_denoms' = pending_denoms,
      swap_routes' = swap_routes, swap_info' = swap_info,
      transmutation_pairs' = transmutation_pairs,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  pure def contains_mbrn(d: Denom): bool = d == "mbrn" or d == "umbrn"   // simplified

  // --- BurnTokens (contract.rs:908-973) ---
  action BurnTokens(sender: Addr,
                     denom: Denom,
                     amount: Uint128,
                     burn_from: Addr): bool = {
    val ti = if (tokens.keys().contains(denom)) tokens.get(denom)
             else { current_supply: 0, max_supply: None, burned_supply: 0 }
    val new_supply = ti.current_supply - amount
    val new_burned = ti.burned_supply + amount
    all {
      is_owner(config, sender),
      amount > 0,
      tokens.keys().contains(denom),
      ti.current_supply >= amount,
      tokens' = tokens.put(denom, {
        max_supply: ti.max_supply,
        current_supply: new_supply,
        burned_supply: new_burned,
      }),
      supply_history' = supply_history.put(denom,
        (if (supply_history.keys().contains(denom)) supply_history.get(denom) else List())
          .append({ timestamp: block_time,
                     current_supply: new_supply,
                     burned_supply: new_burned })),
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Burn({
        denom: denom, from: burn_from, amount: amount
      })),
      config' = config, pending_denoms' = pending_denoms,
      swap_routes' = swap_routes, swap_info' = swap_info,
      transmutation_pairs' = transmutation_pairs,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- ChangeAdmin (contract.rs:648-680) ---
  action ChangeAdmin(sender: Addr, denom: Denom, new_admin: Addr): bool = all {
    is_owner(config, sender),
    tokens.keys().contains(denom),
    config' = config, tokens' = tokens,
    pending_denoms' = pending_denoms,
    swap_routes' = swap_routes, swap_info' = swap_info,
    transmutation_pairs' = transmutation_pairs,
    supply_history' = supply_history,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- EditTokenMaxSupply (contract.rs:683-724) ---
  action EditTokenMaxSupply(sender: Addr, denom: Denom, max_supply: Uint128): bool = all {
    is_owner(config, sender),
    tokens.keys().contains(denom),
    val ti = tokens.get(denom)
    tokens' = tokens.put(denom, { ...ti, max_supply: Some(max_supply) }),
    config' = config, pending_denoms' = pending_denoms,
    swap_routes' = swap_routes, swap_info' = swap_info,
    transmutation_pairs' = transmutation_pairs,
    supply_history' = supply_history,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- ExecuteSwaps (contract.rs:191-318) ---
  // Permissionless. Builds multihop swap msgs via BFS over swap_routes.
  // Stores prev_balances + max_slippage in SwapInfo; reply id 2 settles.
  action ExecuteSwaps(sender: Addr,
                       token_in: Denom,
                       amount_in: Uint128,
                       token_out: Denom,
                       max_slippage: Decimal): bool = all {
    amount_in > 0,
    swap_info' = Some({
      swapper: sender,
      prev_balances: List(),                    // populated by implementer
      token_out: token_out,
      max_slippage: max_slippage,
    }),
    msg_queue' = msg_queue.append(Msg_Reply({
      id: SWAP_REPLY_ID, data: "execute_swap"
    })),
    config' = config, tokens' = tokens,
    pending_denoms' = pending_denoms, swap_routes' = swap_routes,
    transmutation_pairs' = transmutation_pairs,
    supply_history' = supply_history,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- TransmuteTokens (contract.rs:321) ---
  action TransmuteTokens(sender: Addr,
                          token_in: Denom,
                          amount: Uint128): bool = {
    val pair_opt: Option[TransmutationPair] =
      transmutation_pairs.foldl(None, (acc, p) =>
        match acc {
          | Some(_) => acc
          | None => if (p.token_in == token_in) Some(p) else None
        })
    all {
      pair_opt != None,
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Mint({
        denom: token_in, to: sender, amount: amount
      })),
      config' = config, tokens' = tokens,
      pending_denoms' = pending_denoms, swap_routes' = swap_routes,
      swap_info' = swap_info,
      transmutation_pairs' = transmutation_pairs,
      supply_history' = supply_history,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  pure def head_or_none(l: List[int]): Option[int] =
    if (l.length() == 0) None else Some(l[0])

  // --- CreateOsmosisGauge (contract.rs:837-841) ---
  // Permissionless passthrough — Osmosis incentives module gates internally.
  action CreateOsmosisGauge(sender: Addr): bool = all {
    msg_queue' = msg_queue.append(Msg_Reply({ id: 0, data: "osmosis_gauge_create" })),
    config' = config, tokens' = tokens,
    pending_denoms' = pending_denoms, swap_routes' = swap_routes,
    swap_info' = swap_info, transmutation_pairs' = transmutation_pairs,
    supply_history' = supply_history,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- AddSwapRoutesFromOracleInfo (contract.rs:137) ---
  action AddSwapRoutesFromOracleInfo(sender: Addr, assets: List[Denom]): bool = all {
    swap_routes' = swap_routes,                 // implementer queries oracle, appends routes
    config' = config, tokens' = tokens,
    pending_denoms' = pending_denoms, swap_info' = swap_info,
    transmutation_pairs' = transmutation_pairs,
    supply_history' = supply_history,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- UpdateConfig (contract.rs:432-553) ---
  // Auth: owner + non_token_contract_auth=true.
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    has_non_token_auth(config, sender),
    new_cfg.owners.length() >= 1,
    config' = new_cfg, tokens' = tokens,
    pending_denoms' = pending_denoms, swap_routes' = swap_routes,
    swap_info' = swap_info, transmutation_pairs' = transmutation_pairs,
    supply_history' = supply_history,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- EditOwner (contract.rs:557-594) ---
  // Auth: owner + non_token_contract_auth=true.
  action EditOwner(sender: Addr,
                    target: Addr,
                    stability_pool_ratio: Option[Decimal],
                    non_token_contract_auth: Option[bool]): bool = {
    val tgt_opt = find_owner(config.owners, target)
    val updated_owners: List[Owner] = config.owners.foldl(List(), (acc, o) =>
      acc.append(
        if (o.owner == target) {
          owner: o.owner,
          total_minted: o.total_minted,
          is_position_contract: o.is_position_contract,
          stability_pool_ratio: match stability_pool_ratio {
            | Some(r) => Some(r)
            | None => o.stability_pool_ratio
          },
          non_token_contract_auth: match non_token_contract_auth {
            | Some(b) => b
            | None => o.non_token_contract_auth
          },
        } else o))
    all {
      has_non_token_auth(config, sender),
      tgt_opt != None,
      config' = { ...config, owners: updated_owners },
      tokens' = tokens, pending_denoms' = pending_denoms,
      swap_routes' = swap_routes, swap_info' = swap_info,
      transmutation_pairs' = transmutation_pairs,
      supply_history' = supply_history,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_owners_nonempty: bool = config.owners.length() >= 1

  val inv_supply_nonneg: bool =
    tokens.keys().forall(d => {
      val ti = tokens.get(d)
      ti.current_supply >= 0 and ti.burned_supply >= 0
    })

  val inv_supply_under_cap: bool =
    tokens.keys().forall(d => {
      val ti = tokens.get(d)
      match ti.max_supply {
      | Some(cap) => ti.current_supply <= cap
      | None => true
    }
    })

  val inv_supply_history_capped: bool =
    supply_history.keys().forall(d => supply_history.get(d).length() <= 365)

  // OX.inv_authority_grant_governance_only — UpdateConfig and EditOwner
  // require non_token_contract_auth which is itself only mutable through
  // those same gates. Bootstrap requirement: at least one owner with
  // non_token_contract_auth=true at instantiate.
  val inv_at_least_one_admin_owner: bool =
    config.owners.indices().exists(i => config.owners[i].non_token_contract_auth)

  val inv_no_unilateral_capital_loss_path_OX: bool =
    inv_owners_nonempty and
    inv_supply_under_cap and
    inv_at_least_one_admin_owner

  val safety: bool =
    inv_owners_nonempty and
    inv_supply_nonneg and
    inv_supply_under_cap and
    inv_supply_history_capped and
    inv_at_least_one_admin_owner and
    inv_no_unilateral_capital_loss_path_OX

  action step: bool = any { advance_time(1) }
}
