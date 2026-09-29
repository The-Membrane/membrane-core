#[cfg(test)]
pub mod tests {
    //! Rate Scenario Tests — Prints a log of rates in various situations
    //! Run with: cargo test -p cdp rate_scenario_log -- --nocapture

    use std::str::FromStr;
    use cosmwasm_std::Decimal;
    use crate::rates::{adaptive_smooth_rate, adaptive_curve_rate};
    use membrane::types::IRMConfig;

    // Constants for Morpho curve demo (not used in production, kept for reference)
    const CURVE_STEEPNESS: u128 = 4;    // C = 4
    const TARGET_UTILIZATION: u64 = 90; // 90%

    fn default_irm() -> IRMConfig {
        IRMConfig {
            adjustment_speed: Decimal::from_atomics(50u128, 0).unwrap(),   // 50/year
            min_adaptive_rate: Decimal::permille(1),                       // 0.1%
            max_adaptive_rate: Decimal::from_atomics(1u128, 0).unwrap(),  // 100%
        }
    }

    fn curve_steepness() -> Decimal {
        Decimal::from_atomics(CURVE_STEEPNESS, 0).unwrap()
    }

    fn target_utilization() -> Decimal {
        Decimal::percent(TARGET_UTILIZATION)
    }

    fn pct(d: Decimal) -> String {
        let bps = d.atomics().checked_mul(10_000u128.into()).unwrap() / Decimal::one().atomics();
        let whole = bps.u128() / 100;
        let frac = bps.u128() % 100;
        format!("{}.{:02}%", whole, frac)
    }

    // =========================================================================
    //  REGULAR DEBT — Adaptive Smooth Rate
    //  current_adaptive_rate exponentially decays toward destination_rate
    // =========================================================================

    #[test]
    fn rate_scenario_log_regular_debt() {
        let irm = default_irm();

        println!("\n{}", "=".repeat(80));
        println!("  REGULAR DEBT: Adaptive Smooth Rate");
        println!("  speed={}, min={}, max={}",
            irm.adjustment_speed, pct(irm.min_adaptive_rate), pct(irm.max_adaptive_rate));
        println!("{}\n", "=".repeat(80));

        // ── Scenario 1: Rate moving UP ──────────────────────────────────────
        println!("─── Scenario 1: Disco destination JUMPS from 2% → 10% ───");
        println!("  Starting current_adaptive_rate = 2%, destination = 10%");
        println!("  {:>10}  {:>16}  {:>10}", "Time", "current_adaptive_rate", "Δ");
        println!("  {:->10}  {:->16}  {:->10}", "", "", "");

        let mut rat = Decimal::percent(2); // 2%
        let dest = Decimal::percent(10);   // 10%
        let step = 3600u64; // 1 hour steps

        let mut prev = rat;
        for hour in 0..=168 { // 7 days
            if hour == 0 {
                println!("  {:>7}h 0  {:>16}  {:>10}", 0, pct(rat), "—");
            } else {
                rat = adaptive_smooth_rate(rat, dest, step, &irm);
                let label = format!("{}h", hour);
                let delta = if rat >= prev {
                    format!("+{}", pct(rat - prev))
                } else {
                    format!("-{}", pct(prev - rat))
                };
                // Print every 6 hours
                if hour % 6 == 0 || hour == 1 || hour == 168 {
                    println!("  {:>10}  {:>16}  {:>10}", label, pct(rat), delta);
                }
                prev = rat;
            }
        }

        // ── Scenario 2: Rate moving DOWN ────────────────────────────────────
        println!("\n─── Scenario 2: Disco destination DROPS from 10% → 3% ───");
        println!("  Starting current_adaptive_rate = 10%, destination = 3%");
        println!("  {:>10}  {:>16}  {:>10}", "Time", "current_adaptive_rate", "Δ");
        println!("  {:->10}  {:->16}  {:->10}", "", "", "");

        let mut rat = Decimal::percent(10);
        let dest = Decimal::percent(3);

        let mut prev = rat;
        for hour in 0..=168 {
            if hour == 0 {
                println!("  {:>7}h 0  {:>16}  {:>10}", 0, pct(rat), "—");
            } else {
                rat = adaptive_smooth_rate(rat, dest, step, &irm);
                let label = format!("{}h", hour);
                let delta = if rat >= prev {
                    format!("+{}", pct(rat - prev))
                } else {
                    format!("-{}", pct(prev - rat))
                };
                if hour % 6 == 0 || hour == 1 || hour == 168 {
                    println!("  {:>10}  {:>16}  {:>10}", label, pct(rat), delta);
                }
                prev = rat;
            }
        }

        // ── Scenario 3: Stable destination ──────────────────────────────────
        println!("\n─── Scenario 3: Destination = current (5%), no movement ───");
        let mut rat = Decimal::percent(5);
        let dest = Decimal::percent(5);
        for _ in 0..10 {
            rat = adaptive_smooth_rate(rat, dest, 86400, &irm); // 1 day
        }
        println!("  After 10 days at destination=current: rate = {}", pct(rat));

        // ── Scenario 4: Near floor ──────────────────────────────────────────
        println!("\n─── Scenario 4: Rate near floor — destination = 0.05% ───");
        println!("  Starting current_adaptive_rate = 1%, destination = 0.05%, floor = 0.1%");
        let mut rat = Decimal::percent(1);
        let dest = Decimal::from_str("0.0005").unwrap(); // 0.05%
        for day in 0..=14 {
            if day > 0 {
                rat = adaptive_smooth_rate(rat, dest, 86400, &irm);
            }
            if day % 2 == 0 || day == 14 {
                println!("  Day {:>2}: rate = {}", day, pct(rat));
            }
        }

        // ── Scenario 5: Near ceiling ────────────────────────────────────────
        println!("\n─── Scenario 5: Rate near ceiling — destination = 300% ───");
        println!("  Starting current_adaptive_rate = 50%, destination = 300%, ceiling = 200%");
        let mut rat = Decimal::percent(50);
        let dest = Decimal::from_atomics(3u128, 0).unwrap(); // 300%
        for day in 0..=14 {
            if day > 0 {
                rat = adaptive_smooth_rate(rat, dest, 86400, &irm);
            }
            if day % 2 == 0 || day == 14 {
                println!("  Day {:>2}: rate = {}", day, pct(rat));
            }
        }

        println!();
    }

    // =========================================================================
    //  PEG DEBT — Adaptive Curve Rate (Morpho-style)
    //  Two mechanisms: curve (steepness) + adaptive (current_adaptive_rate shifts)
    // =========================================================================

    #[test]
    fn rate_scenario_log_peg_debt() {
        let irm = default_irm();
        let c = curve_steepness();
        let target = target_utilization();

        println!("\n{}", "=".repeat(80));
        println!("  PEG DEBT: Adaptive Curve IRM (Morpho-style) - FOR REFERENCE ONLY");
        println!("  NOTE: Peg debt now uses same rates as regular debt");
        println!("  C={}, target_util={}, speed={}, min={}, max={}",
            c, pct(target),
            irm.adjustment_speed, pct(irm.min_adaptive_rate), pct(irm.max_adaptive_rate));
        println!("{}\n", "=".repeat(80));

        // ── Snapshot: Instantaneous curve at different utilizations ──────────
        println!("─── Instantaneous Curve (current_adaptive_rate = 4%) ───");
        println!("  Shows borrow_rate at various utilization levels, NO time passage");
        println!("  {:>12}  {:>16}  {:>16}", "Utilization", "Borrow Rate", "Curve Mult");
        println!("  {:->12}  {:->16}  {:->16}", "", "", "");

        let rat = Decimal::percent(4); // 4% initial current_adaptive_rate
        for util_pct in [0u64, 10, 20, 30, 40, 50, 60, 70, 75, 80, 85, 88, 90, 92, 95, 98, 100] {
            let util = Decimal::percent(util_pct);
            let (borrow_rate, _) = adaptive_curve_rate(util, rat, 0, &irm, c, target);
            let mult = if !rat.is_zero() {
                format!("{:.4}x", borrow_rate.atomics().u128() as f64 / rat.atomics().u128() as f64)
            } else {
                "—".to_string()
            };
            println!("  {:>11}%  {:>16}  {:>16}", util_pct, pct(borrow_rate), mult);
        }

        // ── Time evolution: utilization stuck BELOW target ──────────────────
        println!("\n─── Time Evolution: Utilization stuck at 50% (below 90% target) ───");
        println!("  current_adaptive_rate adapts DOWNWARD, borrow_rate follows");
        println!("  {:>8}  {:>16}  {:>16}", "Day", "current_adaptive_rate", "Borrow Rate");
        println!("  {:->8}  {:->16}  {:->16}", "", "", "");

        let mut rat = Decimal::percent(4);
        let util = Decimal::percent(50);
        let day = 86400u64;

        for d in 0..=30 {
            let (borrow_rate, new_rat) = adaptive_curve_rate(util, rat, if d == 0 { 0 } else { day }, &irm, c, target);
            if d == 0 || d % 3 == 0 || d == 30 {
                println!("  {:>8}  {:>16}  {:>16}", d, pct(rat), pct(borrow_rate));
            }
            rat = new_rat;
        }

        // ── Time evolution: utilization stuck ABOVE target ──────────────────
        println!("\n─── Time Evolution: Utilization stuck at 98% (above 90% target) ───");
        println!("  current_adaptive_rate adapts UPWARD, borrow_rate amplified by curve");
        println!("  {:>8}  {:>16}  {:>16}", "Day", "current_adaptive_rate", "Borrow Rate");
        println!("  {:->8}  {:->16}  {:->16}", "", "", "");

        let mut rat = Decimal::percent(4);
        let util = Decimal::percent(98);

        for d in 0..=30 {
            let (borrow_rate, new_rat) = adaptive_curve_rate(util, rat, if d == 0 { 0 } else { day }, &irm, c, target);
            if d == 0 || d % 3 == 0 || d == 30 {
                println!("  {:>8}  {:>16}  {:>16}", d, pct(rat), pct(borrow_rate));
            }
            rat = new_rat;
        }

        // ── Time evolution: utilization AT target ───────────────────────────
        println!("\n─── Time Evolution: Utilization at exactly 90% (target) ───");
        println!("  current_adaptive_rate stays flat, borrow_rate = current_adaptive_rate");
        println!("  {:>8}  {:>16}  {:>16}", "Day", "current_adaptive_rate", "Borrow Rate");
        println!("  {:->8}  {:->16}  {:->16}", "", "", "");

        let mut rat = Decimal::percent(4);
        let util = Decimal::percent(90);

        for d in 0..=10 {
            let (borrow_rate, new_rat) = adaptive_curve_rate(util, rat, if d == 0 { 0 } else { day }, &irm, c, target);
            println!("  {:>8}  {:>16}  {:>16}", d, pct(rat), pct(borrow_rate));
            rat = new_rat;
        }

        // ── Scenario: utilization oscillation ───────────────────────────────
        println!("\n─── Oscillation: Utilization swings 70% ↔ 98% daily ───");
        println!("  {:>8}  {:>8}  {:>16}  {:>16}", "Day", "Util", "current_adaptive_rate", "Borrow Rate");
        println!("  {:->8}  {:->8}  {:->16}  {:->16}", "", "", "", "");

        let mut rat = Decimal::percent(4);
        let utils = [70u64, 98]; // alternating

        for d in 0..=20 {
            let u = Decimal::percent(utils[d as usize % 2]);
            let (borrow_rate, new_rat) = adaptive_curve_rate(u, rat, if d == 0 { 0 } else { day }, &irm, c, target);
            if d % 2 == 0 || d == 20 {
                println!("  {:>8}  {:>7}%  {:>16}  {:>16}", d, utils[d as usize % 2], pct(rat), pct(borrow_rate));
            }
            rat = new_rat;
        }

        // ── Scenario: 100% utilization (emergency) ──────────────────────────
        println!("\n─── Emergency: Utilization at 100% for 7 days ───");
        println!("  {:>8}  {:>16}  {:>16}", "Day", "current_adaptive_rate", "Borrow Rate");
        println!("  {:->8}  {:->16}  {:->16}", "", "", "");

        let mut rat = Decimal::percent(4);
        let util = Decimal::percent(100);

        for d in 0..=7 {
            let (borrow_rate, new_rat) = adaptive_curve_rate(util, rat, if d == 0 { 0 } else { day }, &irm, c, target);
            println!("  {:>8}  {:>16}  {:>16}", d, pct(rat), pct(borrow_rate));
            rat = new_rat;
        }

        // ── Scenario: 0% utilization (empty transmuter) ─────────────────────
        println!("\n─── Low Usage: Utilization at 0% for 14 days ───");
        println!("  {:>8}  {:>16}  {:>16}", "Day", "current_adaptive_rate", "Borrow Rate");
        println!("  {:->8}  {:->16}  {:->16}", "", "", "");

        let mut rat = Decimal::percent(4);
        let util = Decimal::percent(0);

        for d in 0..=14 {
            let (borrow_rate, new_rat) = adaptive_curve_rate(util, rat, if d == 0 { 0 } else { day }, &irm, c, target);
            if d % 2 == 0 || d == 14 {
                println!("  {:>8}  {:>16}  {:>16}", d, pct(rat), pct(borrow_rate));
            }
            rat = new_rat;
        }

        println!();
    }

    // =========================================================================
    //  COMPARISON TABLE — Side-by-side regular vs peg at the same "stress"
    // =========================================================================

    #[test]
    fn rate_scenario_log_comparison() {
        let irm = default_irm();
        let c = curve_steepness();
        let target = target_utilization();

        println!("\n{}", "=".repeat(90));
        println!("  COMPARISON: Regular Debt (smooth) vs Peg Debt (curve+adaptive)");
        println!("  NOTE: Peg debt now uses same rates as regular debt - this is for reference only");
        println!("  Both start at current_adaptive_rate = 4%");
        println!("{}\n", "=".repeat(90));

        println!("─── Regular: dest=10% vs Peg: util=98% — both upward pressure ───");
        println!("  {:>5}  {:>16}  {:>16}  {:>16}",
            "Day", "Regular rate", "Peg rate@target", "Peg borrow_rate");
        println!("  {:->5}  {:->16}  {:->16}  {:->16}", "", "", "", "");

        let mut reg_rat = Decimal::percent(4);
        let mut peg_rat = Decimal::percent(4);
        let dest = Decimal::percent(10);
        let util = Decimal::percent(98);
        let day = 86400u64;

        for d in 0..=14 {
            let (peg_borrow, peg_new) = adaptive_curve_rate(
                util, peg_rat, if d == 0 { 0 } else { day }, &irm, c, target
            );
            if d > 0 {
                reg_rat = adaptive_smooth_rate(reg_rat, dest, day, &irm);
            }
            println!("  {:>5}  {:>16}  {:>16}  {:>16}",
                d, pct(reg_rat), pct(peg_rat), pct(peg_borrow));
            peg_rat = peg_new;
        }

        println!("\n─── Regular: dest=1% vs Peg: util=50% — both downward pressure ───");
        println!("  {:>5}  {:>16}  {:>16}  {:>16}",
            "Day", "Regular rate", "Peg rate@target", "Peg borrow_rate");
        println!("  {:->5}  {:->16}  {:->16}  {:->16}", "", "", "", "");

        let mut reg_rat = Decimal::percent(4);
        let mut peg_rat = Decimal::percent(4);
        let dest = Decimal::percent(1);
        let util = Decimal::percent(50);

        for d in 0..=14 {
            let (peg_borrow, peg_new) = adaptive_curve_rate(
                util, peg_rat, if d == 0 { 0 } else { day }, &irm, c, target
            );
            if d > 0 {
                reg_rat = adaptive_smooth_rate(reg_rat, dest, day, &irm);
            }
            println!("  {:>5}  {:>16}  {:>16}  {:>16}",
                d, pct(reg_rat), pct(peg_rat), pct(peg_borrow));
            peg_rat = peg_new;
        }

        println!();
    }
    // =========================================================================
    //  DISCO DEPOSIT SCENARIOS — TVL-Standardized Comparative Insurance Model
    //  Shows how rates are calculated using insurance ratios normalized by TVL
    // =========================================================================

    /// Mock asset with LTV, deposits, and TVL for rate calculation demonstration
    struct MockAsset {
        name: &'static str,
        max_ltv: Decimal,
        deposits: u128, // MBRN deposit tokens (in millions for display)
        tvl: u128,      // Asset TVL in basket (in millions for display)
    }

    /// Calculate destination rate using TVL-standardized comparative insurance formula
    /// Formula:
    ///   - insurance_ratio = deposit_ratio / tvl_ratio
    ///   - Highest insurance ratio asset: base_rate × (1 / max_LTV)
    ///   - Other assets: base_rate × (highest_insurance / this_insurance)
    ///   - No data (fallback): base_rate × (1 / max_LTV)
    fn calc_destination_rate(
        base_rate: Decimal,
        asset: &MockAsset,
        total_deposits: u128,
        total_tvl: u128,
        highest_insurance: Decimal,
        max_comparative_rate: Decimal,
    ) -> Decimal {
        // Calculate this asset's insurance ratio
        let deposit_ratio = if total_deposits == 0 {
            Decimal::zero()
        } else {
            Decimal::from_ratio(asset.deposits, total_deposits)
        };
        
        let tvl_ratio = if total_tvl == 0 {
            Decimal::zero()
        } else {
            Decimal::from_ratio(asset.tvl, total_tvl)
        };
        
        // Fallback if no data
        if tvl_ratio.is_zero() || asset.deposits == 0 {
            return base_rate * (Decimal::one() / asset.max_ltv);
        }
        
        let insurance_ratio = deposit_ratio / tvl_ratio;
        
        if insurance_ratio == highest_insurance {
            // Highest insurance ratio gets base LTV rate
            base_rate * (Decimal::one() / asset.max_ltv)
        } else {
            // Others pay: base_rate × (highest_insurance / asset_insurance)
            let rate = base_rate * (highest_insurance / insurance_ratio);
            // Cap at max_comparative_rate (irm_config.max_adaptive_rate = 100%)
            if rate > max_comparative_rate {
                max_comparative_rate
            } else {
                rate
            }
        }
    }
    
    /// Calculate insurance ratio for an asset
    fn calc_insurance_ratio(asset: &MockAsset, total_deposits: u128, total_tvl: u128) -> Decimal {
        let deposit_ratio = if total_deposits == 0 {
            Decimal::zero()
        } else {
            Decimal::from_ratio(asset.deposits, total_deposits)
        };
        let tvl_ratio = if total_tvl == 0 {
            Decimal::zero()
        } else {
            Decimal::from_ratio(asset.tvl, total_tvl)
        };
        if tvl_ratio.is_zero() {
            Decimal::zero()
        } else {
            deposit_ratio / tvl_ratio
        }
    }

    #[test]
    fn rate_scenario_log_disco_deposits() {
        let base_rate = Decimal::percent(1); // 1% base interest rate
        let max_comparative = Decimal::one(); // 100% cap (irm_config.max_adaptive_rate)

        println!("\n{}", "=".repeat(100));
        println!("  DISCO DEPOSIT-BASED RATES — TVL-Standardized Comparative Insurance Model");
        println!("  base_interest_rate = {}, max_rate_cap = {}", pct(base_rate), pct(max_comparative));
        println!("{}", "=".repeat(100));
        println!();
        println!("  Formula:");
        println!("    insurance_ratio = deposit_ratio / tvl_ratio");
        println!("    - Highest insurance ratio asset: base_rate × (1 / max_LTV)");
        println!("    - Other assets: base_rate × (highest_insurance / this_insurance)");
        println!("    - No deposits (fallback): base_rate × (1 / max_LTV)");
        println!();
        println!("  Key insight: A small asset with proportionally good insurance won't be");
        println!("  penalized vs a large asset with the same ratio!");
        println!();

        // ── Scenario 1: Proportionally equal (same insurance ratio) ────────────
        println!("─── Scenario 1: Proportionally Equal Insurance (all ratio ≈ 1.0) ───");
        println!("  Each asset has deposits proportional to its TVL share");
        let assets = vec![
            MockAsset { name: "ATOM",   max_ltv: Decimal::percent(70), deposits: 500, tvl: 500 }, // 50%/50% = 1.0
            MockAsset { name: "OSMO",   max_ltv: Decimal::percent(60), deposits: 300, tvl: 300 }, // 30%/30% = 1.0
            MockAsset { name: "stATOM", max_ltv: Decimal::percent(75), deposits: 200, tvl: 200 }, // 20%/20% = 1.0
        ];
        print_tvl_deposit_scenario(&assets, base_rate, max_comparative);

        // ── Scenario 2: Over-insured small asset ────────────────────────────────
        println!("\n─── Scenario 2: Over-insured Small Asset ───");
        println!("  Small asset (stATOM) has disproportionately HIGH deposits");
        let assets = vec![
            MockAsset { name: "ATOM",   max_ltv: Decimal::percent(70), deposits: 400, tvl: 500 }, // 40%/50% = 0.8
            MockAsset { name: "OSMO",   max_ltv: Decimal::percent(60), deposits: 200, tvl: 300 }, // 20%/30% = 0.67
            MockAsset { name: "stATOM", max_ltv: Decimal::percent(75), deposits: 400, tvl: 200 }, // 40%/20% = 2.0 (BEST!)
        ];
        print_tvl_deposit_scenario(&assets, base_rate, max_comparative);

        // ── Scenario 3: Under-insured large asset ───────────────────────────────
        println!("\n─── Scenario 3: Under-insured Large Asset ───");
        println!("  Large asset (ATOM) has disproportionately LOW deposits");
        let assets = vec![
            MockAsset { name: "ATOM",   max_ltv: Decimal::percent(70), deposits: 100, tvl: 600 }, // 10%/60% = 0.17 (WORST)
            MockAsset { name: "OSMO",   max_ltv: Decimal::percent(60), deposits: 400, tvl: 300 }, // 40%/30% = 1.33 (BEST)
            MockAsset { name: "TIA",    max_ltv: Decimal::percent(65), deposits: 500, tvl: 100 }, // 50%/10% = 5.0 -- wait this should be best
        ];
        print_tvl_deposit_scenario(&assets, base_rate, max_comparative);

        // ── Scenario 4: New asset with no deposits ──────────────────────────────
        println!("\n─── Scenario 4: New Asset with No Deposits (falls back to LTV) ───");
        let assets = vec![
            MockAsset { name: "ATOM",      max_ltv: Decimal::percent(70), deposits: 600, tvl: 500 },
            MockAsset { name: "OSMO",      max_ltv: Decimal::percent(60), deposits: 400, tvl: 400 },
            MockAsset { name: "NEW_TOKEN", max_ltv: Decimal::percent(50), deposits: 0, tvl: 100 }, // No deposits!
        ];
        print_tvl_deposit_scenario(&assets, base_rate, max_comparative);

        // ── Scenario 5: Old formula problem case ────────────────────────────────
        println!("\n─── Scenario 5: Old Formula Problem Case (now fixed!) ───");
        println!("  Old formula: small asset with low deposits got spiked rates");
        println!("  New formula: normalizes by TVL share - proportional deposits = fair rates");
        let assets = vec![
            MockAsset { name: "ATOM",  max_ltv: Decimal::percent(70), deposits: 1000, tvl: 900 }, // 91%/90% = 1.01
            MockAsset { name: "SMALL", max_ltv: Decimal::percent(60), deposits: 100, tvl: 100 },  // 9%/10% = 0.9
        ];
        print_tvl_deposit_scenario(&assets, base_rate, max_comparative);
        println!("  ^ In old formula, SMALL would pay 10x rate! Now it pays only ~1.12x");

        // ── Scenario 6: Rate smoothing over time ────────────────────────────────
        println!("\n─── Scenario 6: Insurance Ratios Change Over Time ───");
        println!("  Shows how rates converge when deposit/TVL ratios shift");
        println!();

        let irm = default_irm();
        let day = 86400u64;

        // Initial: ATOM has better insurance
        let mut atom_rat = Decimal::percent(1) * (Decimal::one() / Decimal::percent(70)); // 1.43%
        let mut osmo_rat = Decimal::percent(1) * Decimal::from_ratio(12u128, 10u128); // 1.2%

        println!("  Initial: ATOM insurance=1.2, OSMO insurance=1.0 (ATOM best)");
        println!("  Then: OSMO deposits surge, OSMO insurance rises to 1.5 (OSMO becomes best)");
        println!();
        println!("  {:>5}  {:>18}  {:>18}", "Day", "ATOM rate", "OSMO rate");
        println!("  {:->5}  {:->18}  {:->18}", "", "", "");

        // New destinations after OSMO surge
        let atom_dest = Decimal::percent(1) * Decimal::from_ratio(15u128, 12u128); // 1.25% (1.5/1.2)
        let osmo_dest = Decimal::percent(1) * (Decimal::one() / Decimal::percent(60)); // 1.67% (best gets LTV rate)

        for d in 0..=14 {
            println!("  {:>5}  {:>18}  {:>18}", d, pct(atom_rat), pct(osmo_rat));
            if d < 14 {
                atom_rat = adaptive_smooth_rate(atom_rat, atom_dest, day, &irm);
                osmo_rat = adaptive_smooth_rate(osmo_rat, osmo_dest, day, &irm);
            }
        }

        println!();
    }

    fn print_tvl_deposit_scenario(assets: &[MockAsset], base_rate: Decimal, max_comparative: Decimal) {
        let total_deposits: u128 = assets.iter().map(|a| a.deposits).sum();
        let total_tvl: u128 = assets.iter().map(|a| a.tvl).sum();
        
        // Calculate all insurance ratios
        let insurance_ratios: Vec<Decimal> = assets.iter()
            .map(|a| calc_insurance_ratio(a, total_deposits, total_tvl))
            .collect();
        
        // Find highest insurance ratio
        let highest_insurance = insurance_ratios.iter()
            .filter(|r| !r.is_zero())
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
            .unwrap_or(Decimal::one());

        println!("  {:>12}  {:>8}  {:>8}  {:>8}  {:>12}  {:>12}  {:>14}",
            "Asset", "max_LTV", "Deps", "TVL", "Dep Ratio", "Insurance", "Dest. Rate");
        println!("  {:->12}  {:->8}  {:->8}  {:->8}  {:->12}  {:->12}  {:->14}", "", "", "", "", "", "", "");

        for (i, asset) in assets.iter().enumerate() {
            let dest_rate = calc_destination_rate(base_rate, asset, total_deposits, total_tvl, highest_insurance, max_comparative);
            let dep_ratio = if total_deposits > 0 { 
                format!("{:.1}%", (asset.deposits as f64 / total_deposits as f64) * 100.0)
            } else { 
                "—".to_string() 
            };
            let ins_str = if insurance_ratios[i].is_zero() {
                "fallback".to_string()
            } else if insurance_ratios[i] == highest_insurance {
                format!("{:.2} (best)", insurance_ratios[i].atomics().u128() as f64 / 1e18)
            } else {
                format!("{:.2}", insurance_ratios[i].atomics().u128() as f64 / 1e18)
            };

            println!("  {:>12}  {:>8}  {:>7}M  {:>7}M  {:>12}  {:>12}  {:>14}",
                asset.name,
                pct(asset.max_ltv),
                asset.deposits,
                asset.tvl,
                dep_ratio,
                ins_str,
                pct(dest_rate));
        }
    }
}
