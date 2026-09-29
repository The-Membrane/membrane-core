# Apple Migration: Contract Version Registry & User-Driven Upgrades

## Context

Membrane's modular contract architecture (Collateral/Debt/LiqEngine split) enables multiple versions to run in parallel, but there's no formal system for tracking versions, notifying users of upgrades, or migrating positions between versions. This plan designs a **Version Registry contract** and **Export/Import migration pattern** so users can voluntarily upgrade — like updating an app on their phone.

---

## Minimal Upgrade Bundles

Analysis of cross-contract SubMsg chains reveals these atomic upgrade groups:

| Bundle | Contracts | Why Coupled | Custodies Assets? |
|--------|-----------|-------------|-------------------|
| **cdp_core** | Collateral + Debt + LiqEngine | Callback interfaces: `AccrueAndValidate`, `RefreshAndAssess`, `AuthorizedWithdraw`, `LiquidationDebtUpdate`, etc. Shared `position_id` semantics. | Yes (Collateral holds tokens, Debt tracks credit) |
| **oracle** | Oracle | Query-only. Independent IF `PriceResponse` struct unchanged. | No |
| **liq_queue** | Liq-Queue | Receives assets via `AuthorizedTransfer`. Independent IF liquidation message interface unchanged. | Yes (bid assets) |
| **ltv_disco** | LTV Disco | Queried by Collateral for dynamic LTVs. Independent IF query interface stable. | Yes (vault tokens in slots) |
| **independent** | Governance, Staking, Transmuter, System Discounts, Auction, Revenue Distributor, Chain Proxy | No tight coupling to CDP SubMsg chains. | Varies (mostly no) |

**Key insight:** Collateral <-> Debt <-> LiqEngine are the only truly tightly-coupled group. Everything else can upgrade independently as long as message interfaces don't change. Oracle is read-only (queries), so it's independent unless `PriceResponse` changes shape.

---

## Version Registry Contract

### State

```rust
// Map<version_id, VersionEntry>
pub const VERSIONS: Map<u64, VersionEntry> = Map::new("versions");
// Map<bundle_name, BundleDefinition>
pub const BUNDLES: Map<&str, BundleDefinition> = Map::new("bundles");
// Map<(user_addr, bundle_name), version_id>
pub const USER_VERSIONS: Map<(&str, &str), u64> = Map::new("user_versions");
// Map<bundle_name, version_id>
pub const RECOMMENDED: Map<&str, u64> = Map::new("recommended");

pub struct VersionEntry {
    pub version_id: u64,
    pub semver: String,           // "1.0.0"
    pub bundle_name: String,      // "cdp_core"
    pub contracts: Vec<ContractEntry>, // [{name, address, code_id, checksum}]
    pub github_url: Option<String>,
    pub release_notes: String,
    pub registered_at: u64,       // block timestamp
    pub status: VersionStatus,    // Active | Supported | Deprecated | Emergency
}

pub struct BundleDefinition {
    pub name: String,
    pub contract_names: Vec<String>,
    pub description: String,
    pub custodies_assets: bool,
}

pub enum VersionStatus { Active, Supported, Deprecated, Emergency }
```

### Key Messages

**Execute:**
- `RegisterVersion { semver, bundle_name, contracts, github_url, release_notes }` — admin registers new deployment
- `DefineBundle { name, contract_names, description, custodies_assets }` — define upgrade groups
- `SetVersionStatus { version_id, status }` — governance changes status
- `SetRecommended { bundle_name, version_id }` — set recommended version
- `EmergencyDeprecate { version_id, reason }` — governance-only, freezes old version
- `DeclareUserVersion { bundle_name, version_id }` — frontend calls after migration

**Query:**
- `GetVersions { bundle_name, status_filter }` — list versions for a bundle
- `GetRecommended { bundle_name }` — latest recommended
- `DetectUserVersion { user, bundle_name }` — queries position contracts to find which version(s) user has positions in
- `GetContractAddresses { version_id }` — resolve addresses for frontend routing

### Version Detection

Registry doesn't track every user's positions. Instead:
1. **Declared**: Frontend stores user's chosen version via `DeclareUserVersion`
2. **Detected**: `DetectUserVersion` queries each version's Collateral contract for user positions (read-only, no storage)
3. **Default**: New users with no positions get the recommended version

Users can have positions in multiple versions simultaneously (v1 keeps working).

---

## CDP Migration Flow: Export/Import

### New Messages on Collateral & Debt

```rust
// Collateral
ExportPosition { position_id, target_collateral_contract, registry_contract, target_version_id }
ImportPosition { position_id, position_owner, collateral_assets, source_contract }

// Debt
ExportDebt { position_id, target_debt_contract, registry_contract, target_version_id }
ImportDebt { position_id, position_owner, rate_segments, deployed_to, pending_interest, total_interest_accrued, ... , source_contract }
```

### Atomic Migration Transaction (single tx, reverts on any failure)

```
User calls Collateral_v1.ExportPosition(position_id, target=Collateral_v2)
  |
  +- 1. Query Registry: verify target version is Active/Supported
  +- 2. Query Registry: verify target address matches registered version
  +- 3. Call Debt_v1.AccrueAndValidate (ensure position state is current)
  +- 4. Remove position from Collateral_v1 state, update supply caps
  +- 5. SubMsg -> Debt_v1.ExportDebt(position_id, target=Debt_v2)
  |     +- Remove DebtPosition from Debt_v1, update basket credit amounts
  |     +- SubMsg -> Debt_v2.ImportDebt (recreate debt state)
  +- 6. BankMsg: send collateral tokens to Collateral_v2
  +- 7. SubMsg -> Collateral_v2.ImportPosition (recreate collateral state)
```

**Authorization:** `ImportPosition` only accepts calls from addresses registered as collateral contracts in the Version Registry. Same for `ImportDebt`.

**CDT handling:** No CDT tokens transfer. CDT is minted via token factory and held by users. Only the debt *accounting* moves (v1 basket decrements, v2 basket increments).

### Migration Strategy by Contract Type

| Contract | Strategy | Notes |
|----------|----------|-------|
| **Collateral + Debt** | Export/Import | Must be atomic. Transfers state + collateral tokens. |
| **LiqEngine** | Redeploy or in-place migrate | No user state. Just config + stats. |
| **Oracle** | In-place CosmWasm migrate | Stateless queries, no user assets. |
| **LTV Disco** | Export/Import | Transfer slot records + vault tokens. |
| **Liq-Queue** | Export/Import | Transfer bid records + bid assets. |
| **Governance, Staking, etc.** | In-place migrate or redeploy | Collective state, not per-user. |

---

## Safety Mechanisms

### Deprecation Levels

| Status | New Deposits | Existing Positions | Migrations Out | Frontend |
|--------|-------------|-------------------|---------------|----------|
| **Active** | Allowed | Full functionality | Allowed | Default |
| **Supported** | Allowed | Full functionality | Allowed | "Update available" |
| **Deprecated** | **Blocked** | Functional (withdraw/repay OK) | Allowed | Warning banner |
| **Emergency** | **Blocked** | **Frozen** | **Only operation allowed** | Critical alert |

### Enforcement in Contracts

Add `version_registry: Option<Addr>` to Collateral/Debt config. On deposit/borrow:

```rust
if let Some(registry) = config.version_registry {
    let health = query_version_health(deps, registry, OUR_VERSION_ID)?;
    match health.status {
        Deprecated => return Err(VersionDeprecated),
        Emergency => if !is_export_msg(&msg) { return Err(EmergencyFreeze) },
        _ => {} // proceed
    }
}
```

### Preventing Bad Migrations

- Source contract queries Registry to verify target version is Active/Supported before exporting
- Import handlers verify sender is a registered contract in the Registry
- Version entries are append-only (can change status, cannot delete)
- Only Governance can call `EmergencyDeprecate`

---

## Frontend Integration

### Dynamic Contract Resolution

Replace static `config/contracts.json` with Registry-based resolution:

```typescript
// hooks/useVersionRegistry.ts
// - Queries registry for user's current version per bundle
// - Falls back to recommended version for new users
// - Returns hasUpdate boolean, migration path, resolved addresses

// services/versionRegistry.ts
// - getContractAddresses(registryAddr, userAddr, bundleName)
// - 1. Check declared version -> 2. Detect from positions -> 3. Use recommended
```

### Ditto Notifications

Add `useVersionNotifications.ts` alongside existing `useProtocolUpdates.ts`:
- "CDP v2.0.0 available — improved liquidation protection"
- "WARNING: Your CDP version is deprecated. Please upgrade."
- "CRITICAL: Emergency — migrate your position immediately."

### Migration UI

Conditional banner on NeutronMint page when `hasUpdate` is true:
- Shows release notes, migration strategy
- "Migrate Position" button executes `ExportPosition` tx
- Progress indicator through the SubMsg chain

---

## Implementation Phases

### Phase 1: Version Registry Contract (standalone, zero risk)
- Create `contracts/version-registry/` with state, messages, handlers
- Define initial bundles (cdp_core, oracle, liq_queue, ltv_disco)
- Register v1.0.0 for each bundle with current deployed addresses
- Deploy to chain

### Phase 2: Frontend Integration (low risk)
- `hooks/useVersionRegistry.ts` + `services/versionRegistry.ts`
- Add registry address to `config/contracts.json`
- `useVersionNotifications.ts` for Ditto
- "Update available" banner on Mint page
- Keep static address fallback

### Phase 3: Export/Import for CDP Core (needs audit)
- Add `version_registry: Option<Addr>` to Collateral + Debt configs
- Implement Export/Import handlers with full SubMsg chain
- Add deprecation checks to deposit/borrow handlers
- Multi-test: export v1 -> import v2, verify balances + state
- **Security audit required** before deployment

### Phase 4: Export/Import for LTV Disco + LQ (after CDP proven)
- Same pattern, simpler (no cross-contract SubMsg chains for Disco)

### Phase 5: Migration UI
- Migration modal, tx builder, progress tracker

---

## Key Files to Modify

**New contract:**
- `contracts/version-registry/` (new)

**Message definitions (add Export/Import):**
- `packages/membrane/src/collateral.rs`
- `packages/membrane/src/debt.rs`

**Contract logic (add handlers + deprecation checks):**
- `contracts/collateral/src/contract.rs`
- `contracts/debt/src/contract.rs`

**Frontend (membrane-app):**
- `hooks/useVersionRegistry.ts` (new)
- `services/versionRegistry.ts` (new)
- `components/DittoSpeechBox/hooks/useVersionNotifications.ts` (new)
- `components/NeutronMint/NeutronMint.tsx` (add update banner)
- `config/contracts.json` (add registry address)

## Verification

1. **Unit tests:** Version Registry CRUD, bundle management, status transitions, authorization
2. **Multi-test integration:** Full Export v1 -> Import v2 flow with real Collateral + Debt contracts, verify token balances, position state, debt accounting
3. **Deprecation test:** Set version to Deprecated, verify deposits blocked, withdrawals work, exports work
4. **Emergency test:** Set Emergency, verify all ops frozen except ExportPosition
5. **Frontend:** Query registry, resolve addresses, show update notifications, execute migration tx
