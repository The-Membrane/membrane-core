# CDP Organ

The CDP (Collateralized Debt Position) organ is the core lending engine of the Membrane protocol. It manages collateral deposits, debt issuance, interest accrual, and liquidations across four tightly integrated contracts.

## Architecture

```
                         User
                          |
            +-------------+-------------+
            |             |             |
        Deposit/      Borrow/       Liquidate
        Withdraw       Repay           |
            |             |             |
      +-----------+ +-----------+ +-------------------+
      | Collateral| |   Debt    | |  Liquidation      |
      | Contract  | | Contract  | |  Engine            |
      +-----------+ +-----------+ +-------------------+
            |             |             |
            +------+------+       +-----+-----+
                   |               |           |
              Oracle Queries   Liq Queue   Sell Wall
                                   |
                              CDT Burn
```

## Contracts

| Contract | Purpose | Key State |
|----------|---------|-----------|
| [Collateral](collateral.md) | Manages deposits, withdrawals, supply caps, LTV parameters, and circuit breakers | BASKET, POSITIONS, VOLATILITY, ASSET_CIRCUIT_BREAKERS |
| [Debt](debt.md) | Issues CDT, accrues interest (variable + fixed), manages rate segments and peg debt | BASKET, POSITIONS, RATES, BASKET_RATE_INDICES |
| [Liquidation Engine](liquidation-engine.md) | Orchestrates liquidations via a 9-step SubMsg chain with delay timers and dynamic fees | LIQUIDATION_TIMERS, LIQUIDATION_STATS |
| [Liq Queue](liq-queue.md) | Stability pool using Liquity-style compounding stake distribution for CDT-to-collateral liquidation | QUEUES, EPOCH_SCALE_SUM |

## Cross-Contract Call Flow

### Deposit

1. User calls `Collateral.Deposit`
2. Collateral updates supply caps via `risk_engine::update_basket_tally(add=true)`
3. Collateral sends fire-and-forget SubMsg to `Debt.NotifyDeposit`
4. Collateral attempts best-effort oracle price refresh

### Borrow (IncreaseDebt)

1. User calls `Debt.IncreaseDebt`
2. Debt accrues interest locally
3. Debt sends SubMsg to `Collateral.RefreshAndAssess` (reply on `COLLATERAL_REFRESH_REPLY_ID=1`)
4. On reply: Debt parses assessment, checks frozen assets, calculates `total_collateral_value`, determines mint amount (from explicit amount or LTV target: `(collateral_value * ltv - existing_debt) / credit_price`), solvency check (`LTV <= avg_max_borrow_ltv AND <= avg_max_ltv`), debt minimum check, creates rate segments, mints CDT via `chain_proxy.MintTokens`

### Withdraw

1. User calls `Collateral.Withdraw`
2. Collateral runs circuit breaker check (10% deviation threshold against 5-price historical average)
3. Collateral computes remaining value and weighted average LTVs: `avg_max_ltv = sum(value_ratio_i * max_LTV_i)`
4. Collateral saves `WithdrawPropagation` and sends SubMsg to `Debt.AccrueAndValidate` (reply on success, `WITHDRAW_REPLY_ID=4`)
5. On success reply: `BankMsg::Send` to user, remove from position, update supply caps

### Liquidation

1. Caller triggers `LiquidationEngine.Liquidate`
2. Engine executes a 9-step SubMsg chain across Debt, Collateral, Liq Queue, and Sell Wall
3. CDT is burned via `osmosis_proxy.BurnTokens`, collateral is distributed to liquidators and fee recipients

## Key Design Decisions

- **Path-independent LTV updates**: Upward LTV changes use exponential convergence so the result depends only on elapsed time, not update frequency.
- **Staged downward LTV**: Decreases to max LTV are delayed by a 7-day period with 40% decay rate, protecting borrowers from sudden parameter changes.
- **Per-asset circuit breakers**: Automatic freeze when oracle price deviates >10% from the 5-price historical average, with automatic unfreeze when deviation normalizes.
- **Liquidation delay**: 8-hour default delay for positions near the liquidation threshold, with immediate liquidation for severely undercollateralized positions.
- **Fixed rate caps**: Combined regular + peg fixed-rate debt is capped at a percentage of total debt to limit protocol risk exposure.
- **Liquity-style liq queue**: Uses product/sum snapshot accounting for O(1) claim calculations regardless of the number of liquidation events.
- **Dual rate indices**: Separate rate indices for regular debt and peg debt, where peg debt accumulates an additional `acquisition_bump_rate * (1/max_LTV)`.
