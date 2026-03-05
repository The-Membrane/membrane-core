# Access Control Vulnerability Patterns
## From 61 Real DeFi Audit Reports

Third most common critical issue. Check ALL state-mutating functions.

---

## Pattern 1: Missing Sender Check

```rust
// VULNERABLE — From Levana audit
pub fn save_swap_instruction(deps: DepsMut, msg: SaveSwapInstructionMsg) -> Result<Response> {
    // NO info: MessageInfo parameter → NO caller check
    SWAP_INSTRUCTIONS.save(deps.storage, &msg.instruction)?;
    Ok(Response::new())
}

// ALSO VULNERABLE — has info but never checks it
pub fn update_config(deps: DepsMut, _info: MessageInfo, new_config: Config) -> Result<Response> {
    CONFIG.save(deps.storage, &new_config)?; // Anyone can update config!
    Ok(())
}
```

**Red flags**:
- Function modifies state but has no `info: MessageInfo` parameter
- Function has `info` but never checks `info.sender`
- Function uses `_info` (underscore prefix = unused)

---

## Pattern 2: Wrong Owner Check

```rust
// VULNERABLE — From Mars v2
pub fn update_account_vault(deps: DepsMut, account_id: String, vault_id: String) -> Result<Response> {
    // No check that caller owns account_id!
    ACCOUNT_VAULTS.save(deps.storage, &account_id, &vault_id)?;
    Ok(Response::new())
}
```

**Key question**: For functions that operate on a resource (position, account, deposit), is the caller verified as the OWNER of that resource?

---

## Pattern 3: Unprotected Initialization

```rust
// VULNERABLE — From Axelar audit
pub fn setup(deps: DepsMut, params: SetupParams) -> Result<Response> {
    // NO check if already initialized
    // NO check on who's calling
    GATEWAY_CONFIG.save(deps.storage, &GatewayConfig {
        owner: params.owner, // Attacker becomes owner
    })?;
    Ok(Response::new())
}
```

**Check**: Are there any setup functions beyond `instantiate`? Can they be called multiple times?

---

## Audit Process

For EVERY execute function, ask:
1. Who should be allowed to call this?
2. Is there a check enforcing that?
3. What's the worst case if an attacker calls this?

**Priority order** (audit first):
1. Functions that transfer assets
2. Functions that update critical parameters
3. Functions that change ownership/roles
4. Functions that pause/unpause
5. Functions that trigger migrations

**Severity**: Anyone can call critical function = **CRITICAL**. Missing auth on non-critical = **MAJOR**.
