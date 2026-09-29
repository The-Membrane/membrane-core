// specs/quint/infrastructure/neutron_proxy.qnt
//
// Neutron Proxy — Astroport / Duality routing equivalent of Osmosis Proxy.
// Same multi-owner gated mint/burn surface, multihop swap routing, and
// per-denom supply tracking. Differences from osmosis_proxy:
//   - DEX bindings target Astroport / Duality instead of GAMM/CL pools.
//   - No gauge creation (Neutron's incentive model is different).
//
// Source of truth (Rust):
//   contracts/neutron-proxy/src/state.rs
//   contracts/neutron-proxy/src/contract.rs
//   contracts/neutron-proxy/src/error.rs
//   packages/membrane/src/neutron_proxy.rs
//
// The state shape and authorization model are direct mirrors of
// osmosis_proxy.qnt; types are reproduced here for module independence.
// Solidity-port note: a single ERC20-style proxy can serve both chains,
// but the cross-chain state (denom mappings) needs separate accounting.
module neutron_proxy {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  pure val CREATE_DENOM_REPLY_ID: int = 1
  pure val SWAP_REPLY_ID:         int = 2

  // ================================================================
  // Types — mirror packages/membrane/src/neutron_proxy.rs
  // ================================================================
  type Owner = {
    owner: Addr,
    total_minted: Uint128,
    non_token_contract_auth: bool,
  }

  type TokenInfo = {
    current_supply: Uint128,
    max_supply: Option[Uint128],
    burned_supply: Uint128,
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

  type Config = {
    owners: List[Owner],
    debt_auction: Option[Addr],
    positions_contract: Option[Addr],
    oracle_contract: Option[Addr],
    restrict_mbrn_mints: Option[bool],
    vesting_contract: Option[Addr],            // for AddVestedTransmutation
  }

  // ================================================================
  // State
  // ================================================================
  var config:       Config
  var tokens:       Denom -> TokenInfo
  var swap_routes:  List[SwapRoute]

  action init(cfg: Config): bool = all {
    cfg.owners.length() >= 1,
    config' = cfg,
    tokens' = Map(),
    swap_routes' = List(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure def find_owner(owners: List[Owner], addr: Addr): Option[Owner] =
    owners.foldl(None, (acc, o) =>
      match acc {
        | Some(_) => acc
        | None => if (o.owner == addr) Some(o) else None
      })

  pure def is_owner(cfg: Config, sender: Addr): bool =
    find_owner(cfg.owners, sender) != None

  pure def has_non_token_auth(cfg: Config, sender: Addr): bool =
    match find_owner(cfg.owners, sender) {
      | Some(o) => o.non_token_contract_auth
      | None => false
    }

  // ================================================================
  // Execute actions — same shape as osmosis_proxy
  // ================================================================

  action CreateDenom(sender: Addr,
                      subdenom: str,
                      max_supply: Option[Uint128]): bool = all {
    is_owner(config, sender),
    subdenom != "",
    msg_queue' = msg_queue.append(Msg_Reply({
      id: CREATE_DENOM_REPLY_ID, data: subdenom
    })),
    config' = config, tokens' = tokens, swap_routes' = swap_routes,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  action MintTokens(sender: Addr,
                     denom: Denom,
                     amount: Uint128,
                     mint_to: Addr): bool = {
    val ti = if (tokens.keys().contains(denom)) tokens.get(denom)
             else { current_supply: 0, max_supply: None, burned_supply: 0 }
    val new_supply = ti.current_supply + amount
    val is_debt_auction: bool = match config.debt_auction {
      | Some(a) => sender == a
      | None => false
    }
    all {
      is_owner(config, sender) or
              (match config.vesting_contract {
        | Some(a) => sender == a
        | None => false
      }),
      amount > 0,
      tokens.keys().contains(denom),
      match ti.max_supply {
        | Some(cap) => new_supply <= cap or is_debt_auction
        | None => true
      },
      tokens' = tokens.put(denom, { current_supply: new_supply,
                                     max_supply: ti.max_supply,
                                     burned_supply: ti.burned_supply }),
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Mint({
        denom: denom, to: mint_to, amount: amount
      })),
      config' = config, swap_routes' = swap_routes,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  action BurnTokens(sender: Addr,
                     denom: Denom,
                     amount: Uint128,
                     burn_from: Addr): bool = {
    val ti = if (tokens.keys().contains(denom)) tokens.get(denom)
             else { current_supply: 0, max_supply: None, burned_supply: 0 }
    all {
      is_owner(config, sender),
      amount > 0,
      tokens.keys().contains(denom),
      ti.current_supply >= amount,
      tokens' = tokens.put(denom, {
        current_supply: ti.current_supply - amount,
        burned_supply: ti.burned_supply + amount,
        max_supply: ti.max_supply,
      }),
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Burn({
        denom: denom, from: burn_from, amount: amount
      })),
      config' = config, swap_routes' = swap_routes,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  action ExecuteSwaps(sender: Addr,
                       token_in: Denom,
                       amount_in: Uint128,
                       token_out: Denom,
                       max_slippage: Decimal): bool = all {
    amount_in > 0,
    msg_queue' = msg_queue.append(Msg_Reply({
      id: SWAP_REPLY_ID, data: "execute_swap"
    })),
    config' = config, tokens' = tokens, swap_routes' = swap_routes,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    has_non_token_auth(config, sender),
    new_cfg.owners.length() >= 1,
    config' = new_cfg, tokens' = tokens, swap_routes' = swap_routes,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants — mirror osmosis_proxy
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

  val inv_at_least_one_admin_owner: bool =
    config.owners.indices().exists(i => config.owners[i].non_token_contract_auth)

  val inv_no_unilateral_capital_loss_path_NX: bool =
    inv_owners_nonempty and
    inv_supply_under_cap and
    inv_at_least_one_admin_owner

  val safety: bool =
    inv_owners_nonempty and
    inv_supply_nonneg and
    inv_supply_under_cap and
    inv_at_least_one_admin_owner and
    inv_no_unilateral_capital_loss_path_NX

  action step: bool = any { advance_time(1) }
}
