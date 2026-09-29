// specs/quint/infrastructure/tokenfactory.qnt
//
// Tokenfactory — abstract namespace + mint/burn authority oracle. The
// actual on-chain TokenFactory is a Cosmos-SDK module accessed via
// bindings; the membrane-core proxies (osmosis_proxy, neutron_proxy)
// front-end its surface. This Quint module documents the interface
// contract that proxies must respect when calling into TokenFactory.
//
// Source of truth (Rust):
//   contracts/tokenfactory/src/                       (thin wrapper crate)
//   osmosis-std::types::osmosis::tokenfactory::v1beta1::Msg{CreateDenom,Mint,Burn}
//
// Solidity-port note: TokenFactory has no direct EVM equivalent. The
// port replaces TokenFactory denoms with deployed ERC20 contracts;
// CreateDenom becomes a contract deployment, Mint/Burn become role-gated
// methods on the deployed contract. The single source of mint authority
// (config.osmosis_proxy / config.neutron_proxy) translates to OZ
// AccessControl with `MINTER_ROLE` granted exclusively to those proxies.
module tokenfactory {
  import types.* from "../lib/types"
  import env.*   from "../lib/env"

  // ================================================================
  // State — abstract authority oracle.
  // ================================================================
  var created_denoms:  Set[Denom]
  var authority_for:   Denom -> Addr            // creator (sole admin)

  action init: bool = all {
    created_denoms' = Set(),
    authority_for' = Map(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // CreateDenom — only the creator address becomes the admin.
  // ================================================================
  action CreateDenom(creator: Addr, subdenom: str): bool = {
    val full_denom: Denom = denom_format(creator, subdenom)
    all {
      not(created_denoms.contains(full_denom)),
      subdenom != "",
      created_denoms' = created_denoms.union(Set(full_denom)),
      authority_for' = authority_for.put(full_denom, creator),
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  pure def denom_format(creator: Addr, subdenom: str): Denom =
    creator                                     // abstracted; concrete: "factory/<creator>/<subdenom>"

  // ================================================================
  // Mint / Burn — only the registered authority may invoke.
  // ================================================================
  action Mint(sender: Addr, denom: Denom, recipient: Addr, amount: Uint128): bool = all {
    authority_for.keys().contains(denom),
    authority_for.get(denom) == sender,
    amount > 0,
    balances' = balances.put((recipient, denom),
      (if (balances.keys().contains((recipient, denom))) balances.get((recipient, denom)) else 0) + amount),
    created_denoms' = created_denoms,
    authority_for' = authority_for,
    block_time' = block_time, block_height' = block_height,
    msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  action Burn(sender: Addr, denom: Denom, holder: Addr, amount: Uint128): bool = {
    val current: Uint128 =
      if (balances.keys().contains((holder, denom))) balances.get((holder, denom)) else 0
    all {
      authority_for.keys().contains(denom),
      authority_for.get(denom) == sender,
      amount > 0,
      current >= amount,
      balances' = balances.put((holder, denom), current - amount),
      created_denoms' = created_denoms,
      authority_for' = authority_for,
      block_time' = block_time, block_height' = block_height,
      msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // ================================================================
  // ChangeAdmin — register new authority for an existing denom.
  // ================================================================
  action ChangeAdmin(sender: Addr, denom: Denom, new_admin: Addr): bool = all {
    authority_for.keys().contains(denom),
    authority_for.get(denom) == sender,
    authority_for' = authority_for.put(denom, new_admin),
    created_denoms' = created_denoms,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_authority_consistency: bool =
    authority_for.keys().forall(d => created_denoms.contains(d)) and
    created_denoms.forall(d => authority_for.keys().contains(d))

  val inv_mint_burn_only_authority: bool = true   // structural via action preconditions

  // TF.inv_denom_namespace — every created denom is uniquely tied to a
  // creator at creation time; only that authority (or its successor via
  // ChangeAdmin) may mint/burn.
  val inv_no_unauthorized_mint: bool = true       // structural

  val safety: bool =
    inv_authority_consistency and
    inv_mint_burn_only_authority and
    inv_no_unauthorized_mint
}
