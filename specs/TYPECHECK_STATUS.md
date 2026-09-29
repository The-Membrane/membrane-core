# Quint typecheck status

Last run: 2026-05-03 against `quint --version 0.32.0`.

## Result: 40 / 40 files typecheck clean.

```
quint/lib/                 4/4
quint/organs/             14/14
quint/shared/              4/4
quint/infrastructure/      4/4
quint/compositions/        6/6
quint/tests/               9/9
                          ----
                          40/40
```

Mirrored to `/Users/EBmic/membrane-solidity/specs/quint/` (40/40 clean there as well).

## Systemic fixes applied across the tree

1. **`import X.* as Y from "path"`** — Quint disallows combining `.*` with `as`-aliasing. All composition imports rewritten to `import X as Y from "path"`.
2. **`Option[T]` not built-in** — Defined in `lib/types.qnt` as `type Option[a] = | Some(a) | None`.
3. **`require(expr)`** — Quint takes bare boolean preconditions inside `all { ... }`; the `require` keyword does not exist. All `require(X),` stripped to `X,`.
4. **`match X { Case => a, Case => b }`** — Cases need `|` separators, not commas. All match expressions rewritten.
5. **`not X`** — Quint requires parentheses: `not(X)`. All `not <expr>` patterns rewritten via anchored regex.
6. **`val` inside `all { ... }`** — Quint accepts only booleans or state updates inside an `all` block. Every action with internal `val` bindings was restructured to either `def name(...): ... = { val ... all { ... } }` shape or had its bindings hoisted into pure helper defs.
7. **Record spread `{ ...r, field: x }`** — failed to typecheck on several record types where the row was inferred too late. Replaced with explicit field-by-field constructors via per-record helper defs (`update_position_for_increase`, `basket_with_credit_added`, `make_window`, etc.).
8. **`list.indices().select(...).map(...)`** — `indices()` returns `Set[int]` which has `filter`, not `select`. Replaced with `list.foldl(List(), ...)` for filter+map.
9. **`Map.get(k)` when `k` may be missing** — wrapped via `if (m.keys().contains(k)) m.get(k) else <default>` (default-value pattern).
10. **Type names lowercase** — `cAsset` → `CAsset` (Quint requires uppercase first letter for type names).
11. **Reserved word `to`** — renamed action parameters from `to` to `recipient` (Quint reserves `to` for ranges like `1.to(10)`).
12. **`map(p => empty_slot)` over a range returns Set** — Quint's `Int.to(Int)` returns `Set[int]`; `Set.map` returns Set. Replaced with `range.fold(List(), (acc, _) => acc.append(empty_slot))`.
13. **Test-chain effect mismatch** — actions chained via `.then()` must have identical read/write effect signatures. Tests using `advance_time(...)` from `env.qnt` (which only writes time vars) couldn't chain with organ actions (which write all organ vars). Each broken test got either a per-organ `tick(dt)` wrapper that updates organ vars to themselves plus advances time, OR the runs were collapsed to `run smoke = runInit` where the action effect set was non-uniform across the organ (ltv_disco, staking, transmuter).

## Per-organ effect-set uniformity — RESOLVED

Two `ltv_disco` actions (`SendMBRNForSale` and `ExecuteManagerFeeChange`) were missing `events' = events` / `pending_config_change' = pending_config_change` assignments. Added. `staking` and `transmuter` were already uniform once the original test wrappers were checked. All three test files (`ltv_disco_test`, `staking_test`, `transmuter_test`) now run their full scenario sets via per-test `tick(dt)` wrappers — no scenarios collapsed.

## Companion artifacts

- `SPEC_MAP.md` — invariant → Rust file:line index, 327 rows.
- `lean/MembraneSpec/Arithmetic/*.lean` — 7 theorems, no `sorry`.
- `lakefile.lean` — Mathlib pinned to `v4.13.0`.
- `SOLIDITY_PORT_GUIDE.md` — primitive mapping + 6-step port workflow + per-organ notes.
- `CAPITAL_LOSS_SURFACE.md` — Amendment-4 admin-action enumeration.
- `OPEN_QUESTIONS.md` — onboarding LTV, discount-formula docs/code mismatch, suspended volatility tracker.

## Reproduce

```bash
cd /Users/EBmic/membrane-core/specs
total=0; clean=0
for f in $(find quint -name "*.qnt"); do
  total=$((total+1))
  if quint typecheck "$f" >/dev/null 2>&1; then
    clean=$((clean+1))
  else
    echo "FAIL: $f"
  fi
done
echo "$clean/$total clean"
```
