---
name: contract-audit
description: Systematic DeFi/CosmWasm smart contract security auditing powered by patterns from 61 real audit reports. Use this skill whenever the user asks to audit a contract, review contract security, find vulnerabilities, check for exploits, search for common pitfalls, do a security review, or analyze contract code for bugs. Also trigger when the user mentions specific vulnerability types like reentrancy, access control issues, oracle manipulation, liquidation bugs, unbounded loops, bad debt tracking, double refunds, duplicate array exploits, or precision loss — even if they just say "check this contract", "is this safe", "look for issues", or "any bugs in here". Covers Lending/CDP, DEX/AMM, Perpetuals, Liquid Staking, Synthetics, and Bridge protocols.
---

# Contract Audit Skill

Perform systematic security audits of CosmWasm smart contracts. Based on vulnerability patterns from 61 real Oak Security audit reports (2021-2025).

**Token-efficiency rule**: Do NOT read the full reference guide. Read ONLY the specific reference file relevant to what you're currently investigating. Each reference is self-contained and under 150 lines.

## Reference Files (read on-demand, not upfront)

| File | When to read | Size |
|---|---|---|
| `references/access-control-patterns.md` | When you find a suspicious execute function | ~100 lines |
| `references/liquidation-patterns.md` | When auditing liquidation/position closure code | ~120 lines |
| `references/oracle-patterns.md` | When auditing price queries or oracle integration | ~90 lines |
| `references/common-pitfalls.md` | When checking arrays, loops, refunds, arithmetic | ~130 lines |
| `references/DEFI_AUDIT_GUIDE.md` | Deep dive into protocol-specific checklists (Phase 3 only) | ~2700 lines — read ONLY the specific section needed |

---

## Audit Pipeline

### Step 1: Classify (1 minute)

Determine the protocol type — this decides scan priority.

| If the contract... | It's a... | Prioritize |
|---|---|---|
| Manages collateral + debt positions | **Lending/CDP** | Liquidation (40% of criticals), oracle, access control |
| Facilitates token swaps via pools | **DEX/AMM** | Refund logic, swap math, price manipulation |
| Offers leveraged trading | **Perpetuals** | Position tracking, funding rates, bad debt |
| Tokenizes staked assets | **Liquid Staking** | Unbounded collections, delegation, rewards |
| Handles reward distribution | **Staking/Vault** | Reward math, unbounded loops, duplicate claims |

### Step 2: Rapid Scan (5-10 minutes)

Run these searches against the contract directory. Use the Grep tool directly — do not shell out to grep. Adapt paths to the specific contract being audited.

**Scan A — Access Control** (always do this first):
```
Search for: "_info: MessageInfo" or "_info:" in execute functions
Search for: "pub fn execute" or "pub fn " in contract.rs
Search for: "info.sender" to see where auth checks exist
```
Any execute function with `_info` (unused sender) or no `info.sender` check is a finding. If you find something suspicious, read `references/access-control-patterns.md`.

**Scan B — Unbounded Iterations**:
```
Search for: "for " and ".iter()" and ".range(" in src/*.rs
Search for: "MAX_" or "const" with "usize" for caps
```
Any loop without a size cap on the collection is a finding.

**Scan C — State After External Calls (CEI)**:
```
Search for: "WasmMsg::" and "BankMsg::" and "SubMsg"
Then check: is ".save" or ".update" called BEFORE or AFTER these messages?
```

**Scan D — Protocol-Specific** (pick based on classification):

For **Lending/CDP**:
```
Search for: "liquidat" or "seize" or "bad_debt" or "force_close"
Search for: "query_price" or "oracle" or "twap" or "spot_price"
Search for: "overflow-checks" in Cargo.toml
```
If found, read `references/liquidation-patterns.md` and `references/oracle-patterns.md`.

For **Staking/Vault/Reward**:
```
Search for: "Vec<" in function params (duplicate array exploit vector)
Search for: "claim" or "reward" or "distribute" or "fee_event"
Search for: "dedup" or "HashSet" or "unique" (dedup protection)
```
If suspicious, read `references/common-pitfalls.md`.

For **DEX/AMM**:
```
Search for: "refund" or "excess" or "remaining" or "surplus"
Search for: "info.funds" or ".denom" (wrong denomination)
```

### Step 3: Read and Trace (bulk of time)

For each potential issue from Step 2:

1. **Read the function** — don't just flag a grep match. Read 50+ lines of surrounding context.
2. **Trace the flow** — follow the execution path end-to-end.
3. **Check mitigations** — the code might handle the issue elsewhere (reply handlers, callbacks, wrapper functions).
4. **Verify exploitability** — can an external actor trigger this? What's the attack?

Only read a reference file when you need detailed vulnerability patterns for comparison. Don't read all references upfront.

### Step 4: Classify Severity

| Can it cause... | Severity |
|---|---|
| Direct loss of funds | **CRITICAL** |
| Protocol insolvency or permanent DoS | **CRITICAL** |
| Incorrect state or temporary DoS | **MAJOR** |
| Best practice violation or inefficiency | **MINOR** |
| Code quality issue only | **INFORMATIONAL** |

### Step 5: Report

Use this exact format:

```markdown
# Security Audit Report: [Contract Name]

**Scope**: [files audited]
**Commit**: [hash if available]

## Executive Summary
Total Issues: X Critical, Y Major, Z Minor, W Informational
[1-2 sentence overall assessment]

## Summary Table
| ID | Description | Severity | Status |
|----|-------------|----------|--------|
| C-1 | [Title] | Critical | Open |

## Detailed Findings

### C-1. [Descriptive Title]

**Severity**: Critical
**Location**: `file.rs`, `function_name()`, lines X-Y

**Description**: [What is wrong]

**Impact**: [What an attacker can do]

**Vulnerable Code**:
[Show the exact code]

**Recommendation**: [How to fix]

## Positive Security Observations
[List good patterns found — helps calibrate trust in the codebase]
```

---

## Efficiency Guidelines

- **Don't read the full 2700-line guide** — use the focused reference files instead
- **Always run ALL scans (A through D)** — never skip a scan category, even if you've already found critical issues. Missing a vulnerability is worse than spending extra tokens.
- **Don't duplicate grep searches** — if a pattern yields no results, move on
- **Combine related searches** — check access control and CEI simultaneously when reading execute functions
- **Report positive patterns too** — if the code has strong defenses (rate assurance, post-op validation, caps), note them. This is valuable signal.
