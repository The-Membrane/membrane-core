# Lean proof status

Last build: 2026-05-06 with Lean 4.13.0 + Mathlib pin `d7317655e2826dc1f1de9a0c138db2775c4bb841`.

## Theorem build results — ALL 7 PASS

| File | Status |
|---|---|
| MembraneSpec/Bridge/RustEncoding.lean | ✅ |
| MembraneSpec/Arithmetic/DecimalMulRatio.lean | ✅ |
| MembraneSpec/Arithmetic/InterestAccrual.lean | ✅ |
| MembraneSpec/Arithmetic/IsqrtMonotone.lean | ✅ |
| MembraneSpec/Arithmetic/FeeSplit.lean | ✅ (proof updated for Mathlib 4.13.0) |
| MembraneSpec/Arithmetic/LiqFeeCap.lean | ✅ (proof updated for Mathlib 4.13.0) |
| MembraneSpec/Arithmetic/TrancheWaterfall.lean | ✅ (proof updated for Mathlib 4.13.0) |
| MembraneSpec/Arithmetic/VTRate.lean | ✅ (proof updated for Mathlib 4.13.0) |

## Mathlib drift fixes applied

The 4 originally-broken proofs were repaired by replacing `linarith`/`omega` closers with explicit `Nat.le_trans` chains and folding redundant rewrites into preceding `simp` calls:

- **FeeSplit:41** — `omega` → `exact Nat.le_trans hDiv hRhs.le`
- **VTRate:63** — `omega` → `exact Nat.le_trans h3 h4.le`
- **VTRate:77** — folded a redundant `rw` into the preceding `simp [hs, Nat.mul_div_cancel_left baseAmount h]`
- **LiqFeeCap:46** — `omega` → `exact h3 ▸ h2`
- **LiqFeeCap:96** — `omega` → `exact Nat.le_trans hDivLe hBound`
- **TrancheWaterfall:45/54** — added explicit `unfold juniorLoss` + restructured both branches to use `rw [h_min_j]; omega` and `rw [h_min_j, h_min_s]; omega`

## Reproducing

```bash
cd /Users/EBmic/membrane-core/specs/lean
PATH=$HOME/.elan/bin:$PATH lake exe cache get
PATH=$HOME/.elan/bin:$PATH lake build
```

Expected output: `Build completed successfully.` Only benign unused-variable lints remain.
