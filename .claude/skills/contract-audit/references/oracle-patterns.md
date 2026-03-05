# Oracle Vulnerability Patterns
## From 61 Real DeFi Audit Reports

Second most common critical issue. All DeFi protocols rely on price data.

---

## Pattern 1: Spot Price for Critical Decisions

```rust
// VULNERABLE — From Mars v2
let spot_price = query_astroport_spot_price(pool_addr)?;
let health_factor = calculate_health(spot_price)?;
// Attacker manipulates pool with flash loan → liquidates healthy positions
```

**Rule**: NEVER use spot prices for liquidations, collateral valuation, or health factor. Use TWAP (minimum 15-minute window).

**Where spot is OK**: Display purposes, swap execution with slippage protection.

---

## Pattern 2: Price Inversion

```rust
// VULNERABLE — From Levana audit
fn get_price_from_oracle(oracle: &Oracle) -> Result<Decimal> {
    let price_data = oracle.query_price()?;
    Ok(Decimal::one() / price_data.price) // Returns INVERTED price!
}
```

**Check**: Trace price queries carefully. Is the price base/quote correct?

---

## Pattern 3: Missing Staleness Check

```rust
// VULNERABLE — From IncrementFi
fn set_price(feeder: Addr, asset: String, price: Decimal, timestamp: u64) -> Result<Response> {
    // Feeder can set timestamp = u64::MAX → price never expires
    PRICES.save(deps.storage, &asset, &PriceData { price, timestamp, feeder })?;
    Ok(Response::new())
}
```

**Validation checklist**:
- [ ] Is timestamp checked against current time?
- [ ] Is there a maximum price age?
- [ ] Are prices within reasonable bounds?
- [ ] Is price direction correct?
- [ ] Are multiple sources compared if available?

---

## Pattern 4: Missing Circuit Breakers

No deviation check between consecutive price updates. Attacker gradually manipulates price.

**Correct pattern**:
```rust
fn get_safe_price(asset: String) -> Result<Decimal> {
    let twap_price = query_twap_price(asset, FIFTEEN_MINUTES)?;
    let last_price = LAST_PRICES.load(deps.storage, &asset)?;
    let deviation = (twap_price - last_price).abs() / last_price;
    if deviation > Decimal::percent(10) {
        return Err(ContractError::PriceDeviationTooLarge {});
    }
    LAST_PRICES.save(deps.storage, &asset, &twap_price)?;
    Ok(twap_price)
}
```

**Severity**: Spot price for critical ops = **CRITICAL**. Missing circuit breakers = **MAJOR**.
