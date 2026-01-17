#[cfg(test)]
mod unit_tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env};
    use cosmwasm_std::{Addr, Decimal, Uint128, Timestamp};
    use membrane::mars_vault_token::{Config, VaultCost};
    use membrane::mars_redbank::{Market, InterestRateModel};

    use crate::contract::{get_vault_cost_rate, accrue_cost};
    use crate::state::{COST_ACCRUAL, CostAccrual, VAULT_TOKEN, CONFIG};

    fn setup_mock_config() -> Config {
        Config {
            owner: Addr::unchecked("admin"),
            mars_redbank_addr: Addr::unchecked("mars_redbank"),
            vault_token: "factory/contract/mvault".to_string(),
            deposit_token: "uusdc".to_string(),
            total_deposit_tokens: Uint128::zero(),
            vault_cost: VaultCost {
                static_cost: None,
                yield_ceiling: None,
            },
            transmuter_addr: Addr::unchecked("transmuter"),
            revenue_distributor_addr: Addr::unchecked("revenue_distributor"),
            cdt_denom: "ucdt".to_string(),
            cdp_contract_addr: Addr::unchecked("cdp_contract"),
            vault_cost_index: 0,
            revenue_distributions: vec![],
        }
    }

    fn setup_mock_market() -> Market {
        Market {
            denom: "uusdc".to_string(),
            reserve_factor: Decimal::zero(),
            interest_rate_model: InterestRateModel {
                optimal_utilization_rate: Decimal::zero(),
                base: Decimal::zero(),
                slope_1: Decimal::zero(),
                slope_2: Decimal::zero(),
            },
            liquidity_rate: Decimal::percent(5), // 5% APR
            borrow_rate: Decimal::percent(7),
            borrow_index: Decimal::one(),
            liquidity_index: Decimal::one(),
            indexes_last_updated: 0,
            collateral_total_scaled: Uint128::new(10000),
            debt_total_scaled: Uint128::new(0),
        }
    }

    #[test]
    fn test_get_vault_cost_rate_static_cost() {
        let mut deps = mock_dependencies();
        let mut config = setup_mock_config();
        config.vault_cost.static_cost = Some(Decimal::percent(3));

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        let cost_rate = get_vault_cost_rate(deps.as_ref(), &config).unwrap();
        assert_eq!(cost_rate, Decimal::percent(3));
    }

    #[test]
    fn test_get_vault_cost_rate_yield_ceiling() {
        let mut deps = mock_dependencies();
        let mut config = setup_mock_config();
        config.vault_cost.yield_ceiling = Some(Decimal::percent(3));

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        // Mock the Mars market query
        deps.querier.update_wasm(|_| -> cosmwasm_std::SystemResult<cosmwasm_std::ContractResult<cosmwasm_std::Binary>> {
            cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(cosmwasm_std::to_json_binary(&setup_mock_market()).unwrap()))
        });

        let cost_rate = get_vault_cost_rate(deps.as_ref(), &config).unwrap();
        assert_eq!(cost_rate, Decimal::percent(2)); // 5% - 3% = 2%
    }

    #[test]
    fn test_get_vault_cost_rate_yield_ceiling_no_cost() {
        let mut deps = mock_dependencies();
        let mut config = setup_mock_config();
        config.vault_cost.yield_ceiling = Some(Decimal::percent(6)); // Higher than 5% APR

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        // Mock the Mars market query
        deps.querier.update_wasm(|_| -> cosmwasm_std::SystemResult<cosmwasm_std::ContractResult<cosmwasm_std::Binary>> {
            cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(cosmwasm_std::to_json_binary(&setup_mock_market()).unwrap()))
        });

        let cost_rate = get_vault_cost_rate(deps.as_ref(), &config).unwrap();
        assert_eq!(cost_rate, Decimal::zero()); // max(5% - 6%, 0) = 0%
    }

    #[test]
    fn test_get_vault_cost_rate_no_cost() {
        let mut deps = mock_dependencies();
        let config = setup_mock_config();

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        let cost_rate = get_vault_cost_rate(deps.as_ref(), &config).unwrap();
        assert_eq!(cost_rate, Decimal::zero());
    }

    #[test]
    fn test_accrue_cost_no_time_elapsed() {
        let mut deps = mock_dependencies();
        let mut config = setup_mock_config();
        config.vault_cost.static_cost = Some(Decimal::percent(10));

        CONFIG.save(deps.as_mut().storage, &config).unwrap();
        VAULT_TOKEN.save(deps.as_mut().storage, &Uint128::new(1000)).unwrap();
        COST_ACCRUAL.save(deps.as_mut().storage, &CostAccrual {
            revenue_vault_tokens: Uint128::zero(),
            last_updated: 1000,
            total_cost_collected: Uint128::zero(),
        }).unwrap();

        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(1000); // Same time as last_updated

        let vt_minted = accrue_cost(&mut deps.as_mut(), env, &config).unwrap();
        assert_eq!(vt_minted, Uint128::zero());
    }

    #[test]
    fn test_accrue_cost_with_time_elapsed() {
        let mut deps = mock_dependencies();
        let mut config = setup_mock_config();
        config.vault_cost.static_cost = Some(Decimal::percent(10)); // 10% annual

        CONFIG.save(deps.as_mut().storage, &config).unwrap();
        VAULT_TOKEN.save(deps.as_mut().storage, &Uint128::new(1000)).unwrap();
        COST_ACCRUAL.save(deps.as_mut().storage, &CostAccrual {
            revenue_vault_tokens: Uint128::zero(),
            last_updated: 1000,
            total_cost_collected: Uint128::zero(),
        }).unwrap();

        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(1000 + 365 * 24 * 60 * 60); // 1 year later

        let vt_minted = accrue_cost(&mut deps.as_mut(), env, &config).unwrap();
        
        // With 10% annual cost and 1 year elapsed, should mint 10% of 1000 = 100 vault tokens
        assert_eq!(vt_minted, Uint128::new(100));

        // Check that state was updated
        let cost_accrual = COST_ACCRUAL.load(deps.as_ref().storage).unwrap();
        assert_eq!(cost_accrual.revenue_vault_tokens, Uint128::new(100));

        let vault_token_supply = VAULT_TOKEN.load(deps.as_ref().storage).unwrap();
        assert_eq!(vault_token_supply, Uint128::new(1100)); // 1000 + 100
    }

    #[test]
    fn test_accrue_cost_partial_year() {
        let mut deps = mock_dependencies();
        let mut config = setup_mock_config();
        config.vault_cost.static_cost = Some(Decimal::percent(10)); // 10% annual

        CONFIG.save(deps.as_mut().storage, &config).unwrap();
        VAULT_TOKEN.save(deps.as_mut().storage, &Uint128::new(1000)).unwrap();
        COST_ACCRUAL.save(deps.as_mut().storage, &CostAccrual {
            revenue_vault_tokens: Uint128::zero(),
            last_updated: 1000,
            total_cost_collected: Uint128::zero(),
        }).unwrap();

        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(1000 + 182 * 24 * 60 * 60); // 6 months later

        let vt_minted = accrue_cost(&mut deps.as_mut(), env, &config).unwrap();
        
        // With 10% annual cost and 6 months elapsed, should mint ~5% of 1000 = ~50 vault tokens
        assert_eq!(vt_minted, Uint128::new(49));

        // Check that state was updated
        let cost_accrual = COST_ACCRUAL.load(deps.as_ref().storage).unwrap();
        assert_eq!(cost_accrual.revenue_vault_tokens, Uint128::new(49));

        let vault_token_supply = VAULT_TOKEN.load(deps.as_ref().storage).unwrap();
        assert_eq!(vault_token_supply, Uint128::new(1049)); // 1000 + 49
    }

    #[test]
    fn test_accrue_cost_zero_cost_rate() {
        let mut deps = mock_dependencies();
        let config = setup_mock_config(); // No cost configured

        CONFIG.save(deps.as_mut().storage, &config).unwrap();
        VAULT_TOKEN.save(deps.as_mut().storage, &Uint128::new(1000)).unwrap();
        COST_ACCRUAL.save(deps.as_mut().storage, &CostAccrual {
            revenue_vault_tokens: Uint128::zero(),
            last_updated: 1000,
            total_cost_collected: Uint128::zero(),
        }).unwrap();

        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(1000 + 365 * 24 * 60 * 60); // 1 year later

        let vt_minted = accrue_cost(&mut deps.as_mut(), env, &config).unwrap();
        assert_eq!(vt_minted, Uint128::zero());

        // Check that state was not updated
        let cost_accrual = COST_ACCRUAL.load(deps.as_ref().storage).unwrap();
        assert_eq!(cost_accrual.revenue_vault_tokens, Uint128::zero());

        let vault_token_supply = VAULT_TOKEN.load(deps.as_ref().storage).unwrap();
        assert_eq!(vault_token_supply, Uint128::new(1000)); // Unchanged
    }

    #[test]
    fn test_accrue_cost_cumulative() {
        let mut deps = mock_dependencies();
        let mut config = setup_mock_config();
        config.vault_cost.static_cost = Some(Decimal::percent(10)); // 10% annual

        CONFIG.save(deps.as_mut().storage, &config).unwrap();
        VAULT_TOKEN.save(deps.as_mut().storage, &Uint128::new(1000)).unwrap();
        COST_ACCRUAL.save(deps.as_mut().storage, &CostAccrual {
            revenue_vault_tokens: Uint128::new(50), // Already have some revenue
            last_updated: 1000,
            total_cost_collected: Uint128::zero(),
        }).unwrap();

        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(1000 + 182 * 24 * 60 * 60); // 6 months later

        let vt_minted = accrue_cost(&mut deps.as_mut(), env, &config).unwrap();
        
        // Should mint additional 49 vault tokens (6 months of 10% annual)
        assert_eq!(vt_minted, Uint128::new(49));

        // Check that state was updated cumulatively
        let cost_accrual = COST_ACCRUAL.load(deps.as_ref().storage).unwrap();
        assert_eq!(cost_accrual.revenue_vault_tokens, Uint128::new(99)); // 50 + 49

        let vault_token_supply = VAULT_TOKEN.load(deps.as_ref().storage).unwrap();
        assert_eq!(vault_token_supply, Uint128::new(1049)); // 1000 + 49
    }
}
