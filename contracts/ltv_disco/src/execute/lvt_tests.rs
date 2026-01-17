use super::*;
use cosmwasm_std::{testing::mock_env, Timestamp};
use membrane::types::Locked;

fn day(days: u64) -> u64 {
    days * ONE_DAY_SECONDS
}

fn brute_force_state(
    base_lvt: u128,
    reference_time: u64,
    base_daily_delta: i128,
    cliffs: &[LVTTimeCliff],
    timestamp: u64,
) -> (i128, i128) {
    let mut current_lvt = base_lvt as i128;
    let mut current_delta = base_daily_delta;
    let mut cliffs_sorted = cliffs.to_vec();
    cliffs_sorted.sort_by_key(|c| c.timestamp);

    if timestamp >= reference_time {
        let mut t = reference_time;
        while t < timestamp {
            let next_time = t + ONE_DAY_SECONDS;
            current_lvt += current_delta;
            t = next_time;
            for cliff in cliffs_sorted.iter().filter(|c| c.timestamp == t) {
                current_delta += cliff.delta_change.i128();
            }
        }
    } else {
        let mut t = reference_time;
        while t > timestamp {
            current_lvt -= current_delta;
            for cliff in cliffs_sorted.iter().filter(|c| c.timestamp == t) {
                current_delta -= cliff.delta_change.i128();
            }
            t -= ONE_DAY_SECONDS;
        }
    }

    (current_lvt, current_delta)
}

fn make_deposit(vault_tokens: u128, locked: Option<Locked>, start_time: u64) -> BackingDeposit {
    BackingDeposit {
        user: Addr::unchecked("user"),
        vault_tokens: Uint128::from(vault_tokens),
        locked_vault_tokens: Uint128::zero(),
        max_borrow_ltv: Decimal::percent(50),
        last_claimed: start_time,
        locked,
        start_time,
        deposit_time: Some(start_time),
        compound_claims: false,
        manager: None,
        depositor: None,
        withdrawals_enabled: true,
        lvt_tracking: DepositLVTTracking {
            base_lvt: Uint128::zero(),
            reference_time: start_time,
            daily_delta: Int128::zero(),
            time_cliffs: vec![],
        },
    }
}

#[test]
fn test_calculate_lvt_at_time_forward_and_reverse() {
    let base_lvt = Uint128::from(100u128);
    let reference_time = 0u64;
    let base_daily_delta = Int128::from(10i128);
    let cliffs = vec![
        LVTTimeCliff {
            timestamp: day(2),
            delta_change: Int128::from(-5i128),
        },
        LVTTimeCliff {
            timestamp: day(4),
            delta_change: Int128::from(20i128),
        },
    ];

    // Forward to day 5:
    // Day 0-2: +10/day => +20 (LVT 120), day 2 cliff -5 => delta 5
    // Day 2-4: +5/day  => +10 (LVT 130), day 4 cliff +20 => delta 25
    // Day 4-5: +25/day => +25 (LVT 155)
    let forward = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        day(5),
    )
    .unwrap();
    assert_eq!(forward, Uint128::from(155u128));

    // Reverse from day 5 to day 1:
    // Start at 155 with delta 25. Day 5-4: -25 => 130, day 4 cliff +20 reversed => delta 5
    // Day 4-2: -5/day => -10 (LVT 120), day 2 cliff -5 reversed => delta 10
    // Day 2-1: -10/day => -10 (LVT 110)
    let reverse_reference_time = day(5);
    let reverse_base_lvt = Uint128::from(155u128);
    let reverse_base_delta = Int128::from(25i128);
    let reverse = calculate_lvt_at_time(
        reverse_base_lvt,
        reverse_reference_time,
        reverse_base_delta,
        &cliffs,
        day(1),
    )
    .unwrap();
    assert_eq!(reverse, Uint128::from(110u128));
}

#[test]
fn test_calculate_lvt_at_time_matches_bruteforce() {
    for seed in 1u64..=5 {
        let base_lvt = 1000u128 + seed as u128 * 17;
        let reference_time = 0u64;
        let base_daily_delta = (seed as i128 % 7) - 3;
        let cliffs: Vec<LVTTimeCliff> = (1u64..=10)
            .map(|day_idx| {
                let change = ((seed * 37 + day_idx * 13) % 7) as i128 - 3;
                LVTTimeCliff {
                    timestamp: day(day_idx),
                    delta_change: Int128::from(change),
                }
            })
            .collect();

        for target_day in 0u64..=12 {
            let (expected_lvt, _) = brute_force_state(
                base_lvt,
                reference_time,
                base_daily_delta,
                &cliffs,
                day(target_day),
            );
            let expected = if expected_lvt < 0 {
                Uint128::zero()
            } else {
                Uint128::from(expected_lvt as u128)
            };

            let actual = calculate_lvt_at_time(
                Uint128::from(base_lvt),
                reference_time,
                Int128::from(base_daily_delta),
                &cliffs,
                day(target_day),
            )
            .unwrap();
            assert_eq!(actual, expected);
        }

        let (lvt_at_ref, delta_at_ref) = brute_force_state(
            base_lvt,
            reference_time,
            base_daily_delta,
            &cliffs,
            day(12),
        );
        let reverse_reference = day(12);
        let reverse_base_lvt = if lvt_at_ref < 0 {
            Uint128::zero()
        } else {
            Uint128::from(lvt_at_ref as u128)
        };

        for target_day in 0u64..=12 {
            let (expected_lvt, _) = brute_force_state(
                base_lvt,
                reference_time,
                base_daily_delta,
                &cliffs,
                day(target_day),
            );
            let expected = if expected_lvt < 0 {
                Uint128::zero()
            } else {
                Uint128::from(expected_lvt as u128)
            };

            let actual = calculate_lvt_at_time(
                reverse_base_lvt,
                reverse_reference,
                Int128::from(delta_at_ref),
                &cliffs,
                day(target_day),
            )
            .unwrap();
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn test_calculate_deposit_contribution_regular_lock() {
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1_000_000);

    let locked_until = env.block.time.seconds() + day(10);
    let deposit = make_deposit(
        100,
        Some(Locked {
            locked_until,
            perpetual_lock: None,
            intended_lock_days: Some(10),
        }),
        env.block.time.seconds(),
    );

    let (base_lvt, daily_delta, cliffs) = calculate_deposit_contribution(&deposit, &env, 100)
        .unwrap();
    assert_eq!(base_lvt, Uint128::from(1100u128));
    assert_eq!(daily_delta, Int128::zero());
    assert_eq!(cliffs.len(), 1);
    assert_eq!(cliffs[0].timestamp, locked_until);
    assert_eq!(cliffs[0].delta_change, Int128::from(100i128));
}

#[test]
fn test_calculate_deposit_contribution_locked_non_perpetual() {
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(5_000_000);

    // Test a longer non-perpetual lock (90 days) with different vault_tokens
    let locked_until = env.block.time.seconds() + day(90);
    let vault_tokens = 500u128;
    let deposit = make_deposit(
        vault_tokens,
        Some(Locked {
            locked_until,
            perpetual_lock: None,
            intended_lock_days: Some(90),
        }),
        env.block.time.seconds(),
    );

    let (base_lvt, daily_delta, cliffs) = calculate_deposit_contribution(&deposit, &env, 100)
        .unwrap();
    
    // Expected: base_lvt = vault_tokens * (lock_days + 1) = 500 * (90 + 1) = 45,500
    assert_eq!(base_lvt, Uint128::from(45500u128));
    
    // During lock period: daily_delta should be 0 (lock decay cancels time boost)
    assert_eq!(daily_delta, Int128::zero());
    
    // Should have one cliff at lock expiration
    assert_eq!(cliffs.len(), 1);
    assert_eq!(cliffs[0].timestamp, locked_until);
    // After lock expires, delta changes from 0 to +vault_tokens
    assert_eq!(cliffs[0].delta_change, Int128::from(500i128));
}

#[test]
fn test_calculate_deposit_contribution_perpetual_lock() {
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(2_000_000);

    let locked_until = env.block.time.seconds() + day(30);
    let deposit = make_deposit(
        100,
        Some(Locked {
            locked_until,
            perpetual_lock: Some(30),
            intended_lock_days: Some(30),
        }),
        env.block.time.seconds(),
    );

    let (base_lvt, daily_delta, cliffs) = calculate_deposit_contribution(&deposit, &env, 100)
        .unwrap();
    assert_eq!(base_lvt, Uint128::from(3100u128));
    assert_eq!(daily_delta, Int128::from(100i128));
    assert!(cliffs.is_empty());
}

#[test]
fn test_calculate_lvt_at_time_partial_day_no_change() {
    let base_lvt = Uint128::from(500u128);
    let reference_time = day(2);
    let base_daily_delta = Int128::from(25i128);
    let cliffs: Vec<LVTTimeCliff> = vec![];

    // Less than a full day forward should not change LVT
    let forward = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        reference_time + ONE_DAY_SECONDS - 1,
    )
    .unwrap();
    assert_eq!(forward, base_lvt);

    // Less than a full day backward should not change LVT
    let reverse = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        reference_time - (ONE_DAY_SECONDS - 1),
    )
    .unwrap();
    assert_eq!(reverse, base_lvt);
}

#[test]
fn test_calculate_lvt_at_time_cliff_at_reference_time() {
    let base_lvt = Uint128::from(100u128);
    let reference_time = 0u64;
    let base_daily_delta = Int128::from(10i128);
    let cliffs = vec![LVTTimeCliff {
        timestamp: reference_time,
        delta_change: Int128::from(5i128),
    }];

    // At reference_time, LVT should equal base, but delta should include cliff
    let lvt_at_ref = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        reference_time,
    )
    .unwrap();
    assert_eq!(lvt_at_ref, base_lvt);

    // One day forward should use the updated delta (10 + 5)
    let one_day = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        reference_time + ONE_DAY_SECONDS,
    )
    .unwrap();
    assert_eq!(one_day, Uint128::from(115u128));
}

#[test]
fn test_calculate_lvt_at_time_cliff_at_target_time() {
    let base_lvt = Uint128::from(100u128);
    let reference_time = 0u64;
    let base_daily_delta = Int128::from(10i128);
    let cliffs = vec![LVTTimeCliff {
        timestamp: day(2),
        delta_change: Int128::from(20i128),
    }];

    // At day 2, LVT should include only the first 2 days of base delta
    let lvt_at_day_2 = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        day(2),
    )
    .unwrap();
    assert_eq!(lvt_at_day_2, Uint128::from(120u128));

    // At day 3, the cliff should have taken effect
    let lvt_at_day_3 = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        day(3),
    )
    .unwrap();
    assert_eq!(lvt_at_day_3, Uint128::from(150u128));
}

#[test]
fn test_calculate_lvt_at_time_negative_to_zero() {
    let base_lvt = Uint128::from(5u128);
    let reference_time = 0u64;
    let base_daily_delta = Int128::from(-10i128);
    let cliffs: Vec<LVTTimeCliff> = vec![];

    let one_day = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        day(1),
    )
    .unwrap();
    assert_eq!(one_day, Uint128::zero());
}

#[test]
fn test_calculate_lvt_at_time_multiple_cliffs_same_timestamp() {
    let base_lvt = Uint128::from(100u128);
    let reference_time = 0u64;
    let base_daily_delta = Int128::from(10i128);
    let cliffs = vec![
        LVTTimeCliff {
            timestamp: day(2),
            delta_change: Int128::from(5i128),
        },
        LVTTimeCliff {
            timestamp: day(2),
            delta_change: Int128::from(-3i128),
        },
    ];

    let lvt_at_day_3 = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        day(3),
    )
    .unwrap();
    // Day 0-2: +20, then day 2-3 uses delta 12 (10 + 5 - 3)
    assert_eq!(lvt_at_day_3, Uint128::from(132u128));
}

#[test]
fn test_non_midnight_reference_time_flooring() {
    let base_lvt = Uint128::from(50u128);
    let reference_time = ONE_DAY_SECONDS + 1_000u64;
    let base_daily_delta = Int128::from(7i128);
    let cliffs: Vec<LVTTimeCliff> = vec![];

    let almost_one_day = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        reference_time + ONE_DAY_SECONDS - 1,
    )
    .unwrap();
    assert_eq!(almost_one_day, base_lvt);

    let exactly_one_day = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        reference_time + ONE_DAY_SECONDS,
    )
    .unwrap();
    assert_eq!(exactly_one_day, Uint128::from(57u128));

    let backwards_almost_one_day = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        reference_time - (ONE_DAY_SECONDS - 1),
    )
    .unwrap();
    assert_eq!(backwards_almost_one_day, base_lvt);
}

#[test]
fn test_large_values_match_bruteforce() {
    let base_lvt = 1_000_000_000_000_000_000u128;
    let reference_time = 0u64;
    let base_daily_delta = 1_000_000_000_000i128;
    let cliffs = vec![
        LVTTimeCliff {
            timestamp: day(30),
            delta_change: Int128::from(-500_000_000_000i128),
        },
        LVTTimeCliff {
            timestamp: day(90),
            delta_change: Int128::from(2_000_000_000_000i128),
        },
    ];
    let target = day(120);

    let (expected_lvt, _) = brute_force_state(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        target,
    );
    let expected = Uint128::from(expected_lvt as u128);

    let actual = calculate_lvt_at_time(
        Uint128::from(base_lvt),
        reference_time,
        Int128::from(base_daily_delta),
        &cliffs,
        target,
    )
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn test_calculate_deposit_contribution_unlocked() {
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(3_000_000);

    let deposit = make_deposit(250, None, env.block.time.seconds());
    let (base_lvt, daily_delta, cliffs) = calculate_deposit_contribution(&deposit, &env, 100)
        .unwrap();
    assert_eq!(base_lvt, Uint128::from(250u128));
    assert_eq!(daily_delta, Int128::from(250i128));
    assert!(cliffs.is_empty());
}

#[test]
fn test_calculate_deposit_contribution_expired_lock() {
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(4_000_000);
    let locked_until = env.block.time.seconds() - day(5);
    let deposit = make_deposit(
        500,
        Some(Locked {
            locked_until,
            perpetual_lock: None,
            intended_lock_days: Some(10),
        }),
        env.block.time.seconds() - day(20),
    );

    let (base_lvt, daily_delta, cliffs) = calculate_deposit_contribution(&deposit, &env, 100)
        .unwrap();
    assert_eq!(base_lvt, Uint128::from(500u128));
    assert_eq!(daily_delta, Int128::from(500i128));
    assert!(cliffs.is_empty());
}

#[test]
fn test_reverse_time_with_midday_reference() {
    let base_lvt = Uint128::from(100u128);
    let reference_time = ONE_DAY_SECONDS + 123;
    let base_daily_delta = Int128::from(9i128);
    let cliffs: Vec<LVTTimeCliff> = vec![LVTTimeCliff {
        timestamp: reference_time + ONE_DAY_SECONDS,
        delta_change: Int128::from(4i128),
    }];

    let backwards_partial = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        reference_time - 100,
    )
    .unwrap();
    assert_eq!(backwards_partial, base_lvt);
}

#[test]
fn test_cliff_after_target_does_not_apply() {
    let base_lvt = Uint128::from(100u128);
    let reference_time = 0u64;
    let base_daily_delta = Int128::from(10i128);
    let cliffs = vec![LVTTimeCliff {
        timestamp: day(5),
        delta_change: Int128::from(100i128),
    }];

    let lvt_at_day_3 = calculate_lvt_at_time(
        base_lvt,
        reference_time,
        base_daily_delta,
        &cliffs,
        day(3),
    )
    .unwrap();
    assert_eq!(lvt_at_day_3, Uint128::from(130u128));
}

