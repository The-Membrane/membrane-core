// specs/quint/lib/env.qnt
//
// Environment abstractions — block time, contract identity, abstracted
// bank/querier state, and a shared cross-contract message queue.
//
// Organ modules import these and read/write the shared state via the
// `env` namespace. The `compositions/` layer instantiates one `env` per
// run and wires it across organs.
module env {
  import types.* from "./types"

  // Sentinel for the contract's own address (self-calls).
  pure val SELF: Addr = "self"

  // Block environment — advanced monotonically by composition module.
  var block_time: Timestamp
  var block_height: Height

  // Abstracted bank: (holder_addr, denom) -> balance.
  // Organ actions debit/credit via helper actions below.
  var balances: (Addr, Denom) -> Uint128

  // Cross-contract message queue — drained by `deliver_head_of_queue`
  // in compositions/membrane.qnt. Each organ appends messages it emits.
  var msg_queue: List[CrossOrganMsg]

  // Reply callback context — CosmWasm `reply(id, result)` semantics.
  var pending_reply: Option[{ id: int, origin_contract: Addr, data: str }]

  // NOTE on events: each organ module maintains its own `events` var
  // (local list of `Event` from types.qnt). Keeping it local avoids
  // forcing every single organ action in the repo to thread a shared
  // events state through its update. Compositions can merge them for
  // run-level assertions if needed.

  action env_init: bool = all {
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  action advance_time(dt: int): bool = all {
    dt > 0,
    block_time' = block_time + dt,
    block_height' = block_height + 1,
    balances' = balances,
    msg_queue' = msg_queue,
    pending_reply' = pending_reply,
  }

  // Balance helpers — keep balances non-negative.
  pure def balance_of(bs: (Addr, Denom) -> Uint128, who: Addr, d: Denom): Uint128 =
    if (bs.keys().contains((who, d))) bs.get((who, d)) else 0

  action credit(who: Addr, d: Denom, amount: Uint128): bool =
    balances' = balances.set((who, d), balance_of(balances, who, d) + amount)

  action debit(who: Addr, d: Denom, amount: Uint128): bool = all {
    balance_of(balances, who, d) >= amount,
    balances' = balances.set((who, d), balance_of(balances, who, d) - amount),
  }

  action emit(msg: CrossOrganMsg): bool =
    msg_queue' = msg_queue.append(msg)
}
