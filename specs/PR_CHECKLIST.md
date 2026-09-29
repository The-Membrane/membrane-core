# PR checklist — spec impact

Paste this into any PR that touches a contract covered by `specs/SPEC_MAP.md`.

```
## Spec impact
- [ ] Did this PR modify code referenced in specs/SPEC_MAP.md?
- [ ] If yes: updated the corresponding .qnt invariant(s) AND bumped `Last synced commit` in SPEC_MAP.md.
- [ ] New execute handler added? Added an `action` in the matching organ.qnt with an invariant per `require!` / `return Err` branch.
- [ ] New state variable added? Added matching `var` in organ.qnt and a SPEC_MAP row.
- [ ] Ran `scripts/run_quint_all.sh` locally (or justified skip in PR description).
- [ ] If you touched arithmetic helpers (calculate_vault_tokens, calculate_base_tokens, isqrt, interest accrual, waterfall): re-ran `cd specs/lean && lake build`.
```

Drift check runs on every pull_request via CI (`scripts/check_drift.sh`). It will fail only when the Rust file the SPEC_MAP references has diverged from the recorded commit AND the PR touches the same lines. Cheap, deterministic, no model-checking in CI.
