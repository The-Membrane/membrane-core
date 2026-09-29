// specs/quint/lib/types.qnt
//
// Shared types mirroring cosmwasm_std and packages/membrane/src/types.rs.
// Kept abstract so organ modules can use them without re-defining.
//
// Source of truth: packages/membrane/src/types.rs, packages/membrane/src/*.rs
module types {
  // ---- Option ADT (Quint has no built-in Option) ----
  type Option[a] =
    | Some(a)
    | None

  // ---- Primitive aliases ----
  // Quint does not(have) native 128-bit ints; we use `int` and rely on
  // invariants to keep values non-negative. For overflow reasoning the
  // Lean arithmetic carve-out takes over — see specs/lean/MembraneSpec/Arithmetic.
  type Uint128 = int
  type Uint256 = int
  type Decimal = int          // stored as integer * 10^18 scale
  type Timestamp = int        // block seconds
  type Height = int

  // Addresses are opaque strings in CosmWasm; we stay abstract.
  type Addr = str
  type Denom = str

  // Decimal helpers — scale factor
  pure val DECIMAL_FRACTIONAL: int = 1000000000000000000 // 10^18
  pure val DECIMAL_ZERO: Decimal = 0
  pure val DECIMAL_ONE: Decimal = DECIMAL_FRACTIONAL

  pure def decimal_from_percent(pct: int): Decimal = pct * (DECIMAL_FRACTIONAL / 100)
  pure def decimal_from_ratio(num: int, den: int): Decimal =
    if (den == 0) 0 else (num * DECIMAL_FRACTIONAL) / den
  pure def decimal_mul_uint(d: Decimal, n: Uint128): Uint128 =
    (d * n) / DECIMAL_FRACTIONAL

  // Asset pair (packages/membrane/src/transmuter.rs:250-253)
  type AssetPair = {
    cdt: Denom,
    paired_asset: Denom,
  }

  // LiqAsset (packages/membrane/src/types.rs) — shared across revenue paths
  type LiqAsset = {
    info: Denom,
    amount: Uint128,
  }

  // UserInfo (packages/membrane/src/types.rs)
  type UserInfo = {
    position_id: int,
    position_owner: Addr,
  }

  // Affiliate data (packages/membrane/src/types.rs:712-723)
  type AffiliateData = {
    affiliate_address: Addr,
    affiliate_fee: Decimal,
    label: str,
    time_affiliated: Timestamp,
  }

  pure val AFFILIATE_LIMIT: int = 10

  // A CosmWasm Coin analogue for action parameters (`info.funds`).
  type Coin = { denom: Denom, amount: Uint128 }

  // An abstracted cross-contract message envelope. Organ modules append
  // these to a shared queue; `compositions/membrane.qnt` dispatches them.
  type CrossOrganMsg =
    | Msg_BankSend({ to: Addr, denom: Denom, amount: Uint128 })
    | Msg_Tokenfactory_Mint({ denom: Denom, to: Addr, amount: Uint128 })
    | Msg_Tokenfactory_Burn({ denom: Denom, from: Addr, amount: Uint128 })
    | Msg_CDP_FulfillBadDebt({ asset: Denom, amount: Uint128 })
    | Msg_Disco_AddBadDebt({ asset: Denom, amount: Uint128 })
    | Msg_Disco_AddRevenue({ asset: Denom, amount: Uint128 })
    | Msg_Disco_AddDepositTokenRevenue({ asset: Denom, amount: Uint128 })
    | Msg_Disco_SendMBRNForSale({ asset: Denom, amount: Uint128, buyer: Addr })
    | Msg_Disco_ClearBadDebtFreeze({ asset: Denom, amount: Uint128 })
    | Msg_Transmuter_AssignBadDebt({ collateral_denom: Denom, amount: Uint128 })
    | Msg_Transmuter_AccruePool({ notifier: Addr, delta: int })
    | Msg_Auction_StartMBRNSale({ asset: Denom, amount: Uint128 })
    | Msg_RevDist_AddRevenue({ asset: Denom, amount: Uint128 })
    | Msg_Acquisition_AccruePool({ delta: int })
    | Msg_Reply({ id: int, data: str })

  // Saturating-subtract with non-negative floor.
  pure def sat_sub(a: int, b: int): int = if (a >= b) a - b else 0

  pure def min_int(a: int, b: int): int = if (a <= b) a else b
  pure def max_int(a: int, b: int): int = if (a >= b) a else b

  // ================================================================
  // CosmWasm Events — off-chain indexer schema
  // ================================================================
  //
  // These are the *only* authoritative surface for manager P&L and
  // switching-intent history. No on-chain storage of these facts (see
  // specs/OPEN_QUESTIONS.md #2). Indexers parse these events by name
  // and reconstruct manager leaderboards, fee histories, and switch
  // audit logs off-chain.
  //
  // Rust emission shape (per CosmWasm convention):
  //   Response::new()
  //     .add_attribute("action", <event_name>)
  //     .add_attribute(<key>, <value as string>)
  //     ...
  // Event names below are the value of the "action" attribute.
  type Event =
    // manager_bad_debt_absorbed: a bad-debt slashing reduced capital that
    // was managed by `manager` on behalf of `user`. Emitted from the
    // organ whose tranche/slot was slashed.
    | Ev_ManagerBadDebtAbsorbed({
        manager: Addr,
        user: Addr,
        organ: str,                 // "transmuter" | "ltv_disco"
        asset: Denom,
        loss_amount: Uint128,
        timestamp: Timestamp,
      })
    // manager_fee_earned: manager received a fee payout at claim/exit.
    | Ev_ManagerFeeEarned({
        manager: Addr,
        user: Addr,
        organ: str,
        asset: Denom,               // e.g. CDT for Disco claims, CDT for Transmuter exit yield
        fee_amount: Uint128,
        yield_base: Uint128,        // realized yield on which the fee was computed (for audit)
        timestamp: Timestamp,
      })
    // manager_user_added / _removed: binding transitions. Emitted from
    // SetTransmuterManager (Transmuter) or UpdateDeposit/MoveDeposit manager
    // reassignments (Disco).
    | Ev_ManagerUserAdded({ manager: Addr, user: Addr, timestamp: Timestamp })
    | Ev_ManagerUserRemoved({ manager: Addr, user: Addr, timestamp: Timestamp })
    // switch_intent_created / executed / cancelled / expired: visibility
    // for the 14d+2d intent flow (Transmuter junior exits, Disco lower-LTV
    // moves, Disco unstake requests).
    | Ev_SwitchIntentCreated({
        organ: str, kind: str, intent_id: int, user: Addr,
        unlock_time: Timestamp, expire_time: Timestamp,
      })
    | Ev_SwitchIntentExecuted({
        organ: str, kind: str, intent_id: int, user: Addr, executor: Addr,
        timestamp: Timestamp,
      })
    | Ev_SwitchIntentCancelled({
        organ: str, kind: str, intent_id: int, user: Addr, timestamp: Timestamp,
      })
    | Ev_SwitchIntentExpired({
        organ: str, kind: str, intent_id: int, user: Addr, timestamp: Timestamp,
      })
    // manager_fee_request_created / executed / cancelled: request/execute
    // lifecycle for the 14d+2d manager-fee-change path. Fee DECREASES are
    // instant (no request event) — an Ev_ManagerFeeChanged is emitted
    // directly with `instant: true`.
    | Ev_ManagerFeeRequestCreated({
        organ: str, manager: Addr, new_fee: Decimal, current_fee: Decimal,
        unlock_time: Timestamp, expire_time: Timestamp,
      })
    | Ev_ManagerFeeChanged({
        organ: str, manager: Addr, old_fee: Decimal, new_fee: Decimal,
        instant: bool,              // true iff this was a decrease applied immediately
        executor: Addr,             // anyone who fired ExecuteManagerFeeChange (or manager for instant)
        timestamp: Timestamp,
      })
    | Ev_ManagerFeeRequestCancelled({
        organ: str, manager: Addr, timestamp: Timestamp,
      })
}
