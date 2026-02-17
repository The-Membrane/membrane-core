#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use crate::helpers::DiscountsContract;

    use membrane::cdp::{BasketPositionsResponse, PositionResponse};
    use membrane::system_discounts::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::stability_pool::ClaimsResponse;
    use membrane::staking::{StakerResponse, RewardsResponse, Config as Staking_Config};
    use membrane::oracle::PriceResponse;
    use membrane::discount_vault::UserResponse as Discount_UserResponse;
    use membrane::types::{Asset, AssetInfo, AssetPool, Basket, Deposit, StakeDistribution, UserInfo, PendingRevenue};

    use cosmwasm_std::{
        to_binary, Addr, Binary, Empty, Response, StdResult, Uint128, Decimal, Coin,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //Discounts Contract
    pub fn discounts_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contracts::execute,
            crate::contracts::instantiate,
            crate::contracts::query,
        );
        Box::new(contract)
    }

    //Mock Positions Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockExecuteMsg { }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct CDP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {
        GetBasketPositions {
            start_after: Option<String>,
            limit: Option<u32>,
            user_info: Option<UserInfo>, 
            user: Option<String>
        },
        GetBasket { },
    }

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> { 
                match msg {
                    CDP_MockQueryMsg::GetBasketPositions { 
                        start_after,
                        user,
                        user_info,
                        limit,
                    } => {
                        Ok(to_binary(&vec![BasketPositionsResponse {
                            user: String::from(""),
                            positions: vec![
                                PositionResponse {
                                position_id: Uint128::new(1),
                                collateral_assets: vec![ ],
                                credit_amount: Uint128::new(500),
                                cAsset_ratios: vec![ ],
                                avg_borrow_LTV: Decimal::zero(),
                                avg_max_LTV: Decimal::zero(),
                                deployed_to: vec![],
                                pending_interest: Uint128::zero(),
                                total_interest_accrued: Uint128::zero(),
                        }]}])?)
                    },
                    CDP_MockQueryMsg::GetBasket { } => {
                        Ok(to_binary(&Basket {
                            basket_id: Uint128::one(),
                            current_position_id: Uint128::one(),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![],
                            lastest_collateral_rates: vec![],
                            credit_asset: Asset { info: AssetInfo::NativeToken { denom: String::from("credit") }, amount: Uint128::zero() },
                            credit_price: PriceResponse { 
                                prices: vec![], 
                                price: Decimal::one(), 
                                decimals: 6
                            },
                            liq_queue: None,
                            base_interest_rate: Decimal::zero(),
                            pending_revenue: PendingRevenue {
                                total_pending: Uint128::zero(),
                                per_asset_rev: vec![],
                            },
                            negative_rates: false,
                            cpc_margin_of_error: Decimal::zero(),
                            multi_asset_supply_caps: vec![],
                            frozen: false,
                            distribute_revenue: true,
                            credit_last_accrued: 0,
                            rates_last_accrued: 0,
                            oracle_set: false,
                            pending_bad_debt: Uint128::zero(),
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    //Mock Oracle Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Oracle_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockQueryMsg {
        Price {
            asset_info: AssetInfo,
            twap_timeframe: u64,
            basket_id: Option<Uint128>,
        },
    }

    pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Price {
                        asset_info,
                        twap_timeframe,
                        basket_id,
                    } => {
                        Ok(to_binary(&vec![PriceResponse {
                            prices: vec![],
                            price: Decimal::one(),
                            decimals: 0,
                        }])?)
                        
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Staking Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Staking_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockQueryMsg {
        Config {},
        UserRewards { user: String },
        UserStake { staker: String },

    }

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Staking_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())                
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Staking_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Staking_MockQueryMsg::Config { } => {
                        Ok(to_binary(&Staking_Config {
                            owner: Addr::unchecked(""),
                            mbrn_denom: String::from("mbrn_denom"),
                            incentive_schedule: StakeDistribution { rate: Decimal::zero(), duration: 0 },
                            max_commission_rate: Decimal::zero(),
                            unstaking_period: 0,
                            keep_raw_cdt: false,
                            lock_duration_ceiling: 365,
                            vesting_rev_multiplier: Decimal::one(),
                            positions_contract: None,
                            auction_contract: None,
                            vesting_contract: None,
                            governance_contract: None,
                            osmosis_proxy: None,
                            emissions_voting_contract: None,
                        })?)
                    },
                    Staking_MockQueryMsg::UserRewards { user } => {
                        Ok(to_binary(&RewardsResponse {
                            claimables: vec![
                                Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: String::from("debit"),
                                    },
                                    amount: Uint128::new(100u128),
                                },
                                Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: String::from("2nddebit"),
                                    },
                                    amount: Uint128::new(100u128),
                                },
                            ],
                            accrued_interest: Uint128::new(10),
                        })?)
                    },
                    Staking_MockQueryMsg::UserStake { staker } => {
                        Ok(to_binary(&StakerResponse {
                            staker,
                            total_staked: Uint128::new(11),
                            deposit_list: vec![],
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Stability Pool Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum SP_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct SP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum SP_MockQueryMsg {
        AssetDeposits { user: String },
        UnclaimedIncentives { user: String },
        UserClaims { user: String },
        AssetPool {},
    }

    pub fn stability_pool_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::AssetDeposits {
                        user: _,
                    } => Ok(to_binary(&vec![Deposit {
                        user: Addr::unchecked("uzzer"),
                        amount: Decimal::percent(222_00),
                        deposit_time: 0,
                        last_accrued: 0,
                        unstake_time: None,
                    }])?),
                    SP_MockQueryMsg::UnclaimedIncentives {
                        user: _,
                    } => Ok(to_binary(&Uint128::new(5))?),
                    SP_MockQueryMsg::UserClaims {
                        user: _,
                    } => Ok(to_binary(&ClaimsResponse {
                        claims: vec![
                            Coin {
                                denom: String::from("juicy_claims"),
                                amount: Uint128::new(4),
                            }
                        ],
                    })?),
                    SP_MockQueryMsg::AssetPool {} => Ok(to_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken { denom: String::from("credit") },
                            amount: Uint128::new(100),
                        },
                        liq_premium: Decimal::percent(10),
                        deposits: vec![Deposit {
                            user: Addr::unchecked("uzzer"),
                            amount: Decimal::percent(222_00),
                            deposit_time: 0,
                            last_accrued: 0,
                            unstake_time: None,
                        }],
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock Discount Vault Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Discount_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Discount_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Discount_MockQueryMsg {
        User { user: String },
    }

    pub fn lp_discount_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Discount_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Discount_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Discount_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Discount_MockQueryMsg::User {
                        user: _,
                    } => Ok(to_binary(&Discount_UserResponse {
                        user: String::from(""),
                        discount_value: Uint128::new(13),
                        deposits: vec![],
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock Transmuter Contract
    use membrane::transmuter::{UserDepositsResponse, UserDeposit, QueryMsg as TransmuterQueryMsg};
    use std::collections::HashMap;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Transmuter_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Transmuter_MockInstantiateMsg {}

    // Store user deposits in a static variable for testing
    thread_local! {
        static USER_DEPOSITS: std::cell::RefCell<HashMap<String, Vec<UserDeposit>>> = std::cell::RefCell::new(HashMap::new());
    }

    pub fn transmuter_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Transmuter_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Transmuter_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: TransmuterQueryMsg| -> StdResult<Binary> {
                match msg {
                    TransmuterQueryMsg::UserDeposits { user } => {
                        let deposits = USER_DEPOSITS.with(|d| {
                            d.borrow().get(&user).cloned().unwrap_or_default()
                        });
                        Ok(to_binary(&UserDepositsResponse { deposits })?)
                    },
                    _ => Ok(to_binary(&"")?),
                }
            },
        );
        Box::new(contract)
    }

    // Helper function to set user deposits for testing
    pub fn set_transmuter_deposits(user: String, deposits: Vec<UserDeposit>) {
        USER_DEPOSITS.with(|d| {
            d.borrow_mut().insert(user, deposits);
        });
    }

    // Helper function to clear all deposits
    pub fn clear_transmuter_deposits() {
        USER_DEPOSITS.with(|d| {
            d.borrow_mut().clear();
        });
    }

    //Mock LTV Disco Contract
    use membrane::ltv_disco::{AllUserDepositsResponse, UserDepositInfo, BackingDeposit, Config as LTVDiscoConfig, QueryMsg as LTVDiscoQueryMsg};
    use membrane::types::Locked;
    use membrane::ltv_disco::DepositLVTTracking;

    thread_local! {
        static LTV_DISCO_DEPOSITS: std::cell::RefCell<HashMap<String, Vec<UserDepositInfo>>> = std::cell::RefCell::new(HashMap::new());
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum LTVDisco_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct LTVDisco_MockInstantiateMsg {}

    pub fn ltv_disco_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: LTVDisco_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: LTVDisco_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LTVDiscoQueryMsg| -> StdResult<Binary> {
                match msg {
                    LTVDiscoQueryMsg::GetAllUserDeposits { user } => {
                        let deposits = LTV_DISCO_DEPOSITS.with(|d| {
                            d.borrow().get(&user).cloned().unwrap_or_default()
                        });
                        Ok(to_binary(&AllUserDepositsResponse { deposits })?)
                    },
                    LTVDiscoQueryMsg::Config {} => {
                        Ok(to_binary(&LTVDiscoConfig {
                            owner: Addr::unchecked("admin"),
                            cdp_contract: Addr::unchecked("cdp"),
                            deposit_denom: membrane::types::DepositDenom { denom: "mbrn".to_string(), vault_info: None },
                            cdt_denom: "cdt".to_string(),
                            minimum_deposit: Uint128::new(1000),
                            max_ltv: Decimal::percent(90),
                            percent_to_disperse: Decimal::percent(50),
                            dispersal_window: 720,
                            activation_window: 24,
                            oracle_contract: Addr::unchecked("oracle"),
                            chain_proxy_contract: Addr::unchecked("proxy"),
                            emissions_voting_contract: None,
                            lock_duration_ceiling: 365,
                            affiliate_fee: Decimal::percent(1),
                            max_management_fee: Decimal::zero(),
                            ltv_delta_minimum: Decimal::percent(1),
                            points_system_contract: None,
                            revenue_distributor: None,
                            auction_contract: None,
                            mbrn_denom: None,
                        })?)
                    },
                    LTVDiscoQueryMsg::VaultTokenConversion { asset: _, ltv: _, max_borrow_ltv: _, vault_tokens } => {
                        // Simple 1:1 conversion for testing
                        Ok(to_binary(&vault_tokens)?)
                    },
                    _ => Ok(to_binary(&"")?),
                }
            },
        );
        Box::new(contract)
    }

    // Helper function to set LTV Disco deposits for testing
    pub fn set_ltv_disco_deposits(user: String, deposits: Vec<UserDepositInfo>) {
        LTV_DISCO_DEPOSITS.with(|d| {
            d.borrow_mut().insert(user, deposits);
        });
    }

    // Helper function to clear all LTV Disco deposits
    pub fn clear_ltv_disco_deposits() {
        LTV_DISCO_DEPOSITS.with(|d| {
            d.borrow_mut().clear();
        });
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            // bank.init_balance(
            //     storage,
            //     &Addr::unchecked("contract3"),
            //     vec![coin(30_000_000_000_000, "mbrn_denom")],
            // )
            // .unwrap(); //contract3 = Builders contract
            // bank.init_balance(
            //     storage,
            //     &Addr::unchecked("coin_God"),
            //     vec![coin(100_000_000, "debit"), coin(100_000_000, "2nddebit")],
            // )
            // .unwrap();
            // bank.init_balance(
            //     storage,
            //     &Addr::unchecked(USER),
            //     vec![coin(99, "error"), coin(101, "credit_fulldenom")],
            // )
            // .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, DiscountsContract) {
        let mut app = mock_app();

        //Instaniate CDP
        let cdp_id = app.store_code(cdp_contract());

        let cdp_contract_addr = app
            .instantiate_contract(
                cdp_id,
                Addr::unchecked(ADMIN),
                &CDP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Oracle
        let oracle_id = app.store_code(oracle_contract());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Staking
        let staking_id = app.store_code(staking_contract());

        let staking_contract_addr = app
            .instantiate_contract(
                staking_id,
                Addr::unchecked(ADMIN),
                &SP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate LP Discount
        let discount_id = app.store_code(lp_discount_contract());

        let discount_contract_addr = app
            .instantiate_contract(
                discount_id,
                Addr::unchecked(ADMIN),
                &Discount_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate SP
        let sp_id = app.store_code(stability_pool_contract());

        let sp_contract_addr = app
            .instantiate_contract(
                sp_id,
                Addr::unchecked(ADMIN),
                &SP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

                

        //Instantiate Liquidity contract
        let discounts_id = app.store_code(discounts_contract());

        let msg = InstantiateMsg {
            owner: None,
            positions_contract: cdp_contract_addr.to_string(),
            oracle_contract: oracle_contract_addr.to_string(),
            staking_contract: staking_contract_addr.to_string(),
            lockdrop_contract: None,
            discount_vault_contract: Some(discount_contract_addr.to_string()),
            ltv_disco_contract: None,
            transmuter_contract: None,
            minimum_time_in_network: 7,
            max_discount: Some(Decimal::percent(50)),
            mbrn_at_max_discount: Some(Uint128::new(100_000_000_000u128)),
            max_boost: Some(Decimal::percent(9)),
            stable_backing_max_discount: None,
            stable_backing_first_month_discount: None,
            stable_backing_remaining_discount: None,
            stable_backing_curve_duration_days: None,
            stable_backing_first_month_days: None,
            stable_backing_discountable_debt_multiplier: None,
            stable_backing_transmuter_balance_multiplier: None,
        };

        let discounts_contract_addr = app
            .instantiate_contract(
                discounts_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "test",
                None,
            )
            .unwrap();

        let discounts_contract = DiscountsContract(discounts_contract_addr);

        (app, discounts_contract)
    }

    mod discounts {

        use membrane::system_discounts::{Config, UpdateConfig, UserDiscountResponse};

        use super::*;
        

        #[test]
        fn get_discount() {
            let (app, discounts_contract) = proper_instantiate();
            
            //Query Liquidity
            let discount: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: String::from("uzzer"),
                    },
                )
                .unwrap();
            assert_eq!(discount.discount.to_string(), String::from("0.042"));
        }

        #[test]
        fn update_config() {
            let (mut app, discounts_contract) = proper_instantiate();

            //Successful UpdateConfig
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: Some(String::from("new_owner")), 
                positions_contract: Some(String::from("new_pos_contract")),                 
                oracle_contract: Some(String::from("new_oracle_contract")), 
                staking_contract: None, 
                lockdrop_contract: Some(String::from("new_lockdrop_contract")),
                ltv_disco_contract: None,
                transmuter_contract: None,
                max_discount: None,
                mbrn_at_max_discount: None,
                max_boost: None,
                discount_vault_contract: Some((String::from("new_discount_vault_contract"), true)), 
                minimum_time_in_network: Some(14),
                static_discount: Some(UserDiscountResponse {
                    user: String::from("user"),
                    discount: Decimal::percent(101),
                }),
                stable_backing_max_discount: None,
                stable_backing_first_month_discount: None,
                stable_backing_remaining_discount: None,
                stable_backing_curve_duration_days: None,
                stable_backing_first_month_days: None,
                stable_backing_discountable_debt_multiplier: None,
                stable_backing_transmuter_balance_multiplier: None,
            });
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Query Static Discount
            let discount: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: String::from("user"),
                    },
                )
                .unwrap();
            assert_eq!(discount.discount.to_string(), String::from("1"));
            
            //Successful UpdateConfig
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: None,  
                positions_contract: None, 
                oracle_contract:None, 
                staking_contract: None, 
                lockdrop_contract: None, 
                discount_vault_contract: None,
                ltv_disco_contract: None,
                transmuter_contract: None,
                max_discount: None,
                mbrn_at_max_discount: None,
                max_boost: None,
                minimum_time_in_network: None, 
                static_discount: Some(UserDiscountResponse {
                    user: String::from("user"),
                    discount: Decimal::percent(99),
                }),
                stable_backing_max_discount: None,
                stable_backing_first_month_discount: None,
                stable_backing_remaining_discount: None,
                stable_backing_curve_duration_days: None,
                stable_backing_first_month_days: None,
                stable_backing_discountable_debt_multiplier: None,
                stable_backing_transmuter_balance_multiplier: None,
            });
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Query Static Discount
            let discount: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: String::from("user"),
                    },
                )
                .unwrap();
            assert_eq!(discount.discount.to_string(), String::from("0.99"));

            
            //Query Config
            let config: Config = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::Config {},
                )
                .unwrap();
            assert_eq!(
                config, 
                Config {
                    owner: Addr::unchecked(ADMIN), 
                    mbrn_denom: String::from("mbrn_denom"),
                    positions_contract: Addr::unchecked("new_pos_contract"),                
                    oracle_contract: Addr::unchecked("new_oracle_contract"), 
                    staking_contract: Addr::unchecked("contract2"), 
                    lockdrop_contract: Some(Addr::unchecked("new_lockdrop_contract")), 
                    discount_vault_contract: vec![Addr::unchecked("contract3"), Addr::unchecked("new_discount_vault_contract")], 
                    ltv_disco_contract: None,
                    transmuter_contract: None,
                    minimum_time_in_network: 14,
                    max_discount: Decimal::percent(50),
                    mbrn_at_max_discount: Uint128::new(100_000_000_000u128),
                    max_boost: Decimal::percent(9),
                    stable_backing_max_discount: Decimal::percent(75),
                    stable_backing_first_month_discount: Decimal::percent(45),
                    stable_backing_remaining_discount: Decimal::percent(30),
                    stable_backing_curve_duration_days: 90,
                    stable_backing_first_month_days: 30,
                    stable_backing_discountable_debt_multiplier: 18,
                    stable_backing_transmuter_balance_multiplier: Decimal::percent(200),
                });

            //Successful ownership transfer
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: None, 
                positions_contract: None,                 
                oracle_contract: None, 
                staking_contract: None, 
                lockdrop_contract: None,
                discount_vault_contract: None,
                ltv_disco_contract: None,
                transmuter_contract: None,
                max_discount: None,
                mbrn_at_max_discount: None,
                max_boost: None,
                minimum_time_in_network: None,
                static_discount: None,
                stable_backing_max_discount: None,
                stable_backing_first_month_discount: None,
                stable_backing_remaining_discount: None,
                stable_backing_curve_duration_days: None,
                stable_backing_first_month_days: None,
                stable_backing_discountable_debt_multiplier: None,
                stable_backing_transmuter_balance_multiplier: None,
            });
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("new_owner"), cosmos_msg).unwrap();

            
            //Query Config
            let config: Config = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::Config {},
                )
                .unwrap();
            assert_eq!(
                config, 
                Config {
                    owner: Addr::unchecked("new_owner"), 
                    mbrn_denom: String::from("mbrn_denom"),
                    positions_contract: Addr::unchecked("new_pos_contract"),                
                    oracle_contract: Addr::unchecked("new_oracle_contract"), 
                    staking_contract: Addr::unchecked("contract2"), 
                    lockdrop_contract: Some(Addr::unchecked("new_lockdrop_contract")), 
                    discount_vault_contract: vec![Addr::unchecked("contract3"), Addr::unchecked("new_discount_vault_contract")], 
                    ltv_disco_contract: None,
                    transmuter_contract: None,
                    minimum_time_in_network: 14,
                    max_discount: Decimal::percent(50),
                    mbrn_at_max_discount: Uint128::new(100_000_000_000u128),
                    max_boost: Decimal::percent(9),
                    stable_backing_max_discount: Decimal::percent(75),
                    stable_backing_first_month_discount: Decimal::percent(45),
                    stable_backing_remaining_discount: Decimal::percent(30),
                    stable_backing_curve_duration_days: 90,
                    stable_backing_first_month_days: 30,
                    stable_backing_discountable_debt_multiplier: 18,
                    stable_backing_transmuter_balance_multiplier: Decimal::percent(200),
                });

            //Remove old discount vault
            //Successful UpdateConfig
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: None, 
                positions_contract: None,                 
                oracle_contract: None, 
                staking_contract: None, 
                lockdrop_contract: None, 
                discount_vault_contract: Some((String::from("contract3"), false)),
                ltv_disco_contract: None,
                transmuter_contract: None,
                max_discount: None,
                mbrn_at_max_discount: None,
                max_boost: None,
                minimum_time_in_network: None,
                static_discount: None,
                stable_backing_max_discount: None,
                stable_backing_first_month_discount: None,
                stable_backing_remaining_discount: None,
                stable_backing_curve_duration_days: None,
                stable_backing_first_month_days: None,
                stable_backing_discountable_debt_multiplier: None,
                stable_backing_transmuter_balance_multiplier: None,
            });
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("new_owner"), cosmos_msg).unwrap();
        }

        #[test]
        fn test_timed_discount_full_flow() {
            let (mut app, discounts_contract) = proper_instantiate();
            
            // 1. Set a future timed discount period (as owner)
            let current_time = app.block_info().time.seconds();
            let start_time = current_time + 100; // Start in 100 seconds
            let duration = 100; // 100 minutes (duration * 60 seconds)
            let discount = Decimal::percent(95);
            
            let msg = ExecuteMsg::SetDiscountPeriod {
                start_time: Some(start_time),
                duration,
                discount,
            };
            
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            // 2. Query discount before period starts
            let discount_response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: String::from("test_user"),
                    },
                )
                .unwrap();
            
            // Should NOT be the timed discount (should be calculated normally)
            assert_ne!(discount_response.discount, discount);
            assert_eq!(discount_response.discount.to_string(), String::from("0.042"));
            
            // 3. Advance blockchain time to during the period
            app.update_block(|block| {
                block.time = block.time.plus_seconds(150); // Advance to middle of period
            });
            
            let discount_response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: String::from("test_user"),
                    },
                )
                .unwrap();
            
            // Should IS the timed discount value (95%)
            assert_eq!(discount_response.discount, discount);
            
            // 4. Advance time past the period end
            app.update_block(|block| {
                block.time = block.time.plus_seconds(6000); // Advance past end_time (period duration was 100 minutes = 6000 seconds)
            });
            
            let discount_response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: String::from("test_user"),
                    },
                )
                .unwrap();
            
            // Should return to normal calculation (not 95%)
            assert_ne!(discount_response.discount, discount);
            assert_eq!(discount_response.discount.to_string(), String::from("0.042"));
            
            // 5. Clear the expired period
            let msg = ExecuteMsg::ClearDiscountPeriod {};
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap(); // Anyone can clear expired periods
            
            // 6. Test authorization - try to set a period as non-owner
            let msg = ExecuteMsg::SetDiscountPeriod {
                start_time: Some(current_time + 200),
                duration: 50,
                discount: Decimal::percent(50),
            };
            
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            let result = app.execute(Addr::unchecked(USER), cosmos_msg);
            assert!(result.is_err()); // Should fail with Unauthorized
            
            // Set a period as owner first
            let msg = ExecuteMsg::SetDiscountPeriod {
                start_time: Some(current_time + 10000), // Start far in the future
                duration: 50,
                discount: Decimal::percent(50),
            };
            
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            // Try to clear a non-expired period
            let msg = ExecuteMsg::ClearDiscountPeriod {};
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            let result = app.execute(Addr::unchecked(USER), cosmos_msg);
            assert!(result.is_err()); // Should fail because period is not expired
            
            // 7. Test setting period with start_time = None (starts immediately)
            let msg = ExecuteMsg::SetDiscountPeriod {
                start_time: None, // Start immediately
                duration: 30,
                discount: Decimal::percent(80),
            };
            
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            // Immediately query discount
            let discount_response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: String::from("test_user"),
                    },
                )
                .unwrap();
            
            // Should be the timed discount (80%)
            assert_eq!(discount_response.discount, Decimal::percent(80));
        }
    }

    mod stable_backing_discounts {
        use super::*;
        use membrane::system_discounts::{StableBackingDiscountsResponse, QueryMsg, ExecuteMsg, UpdateConfig};
        use membrane::transmuter::UserDeposit;
        use cosmwasm_std::Timestamp;

        fn setup_with_transmuter() -> (App, DiscountsContract, Addr) {
            let mut app = mock_app();

            // Instantiate CDP
            let cdp_id = app.store_code(cdp_contract());
            let cdp_contract_addr = app
                .instantiate_contract(
                    cdp_id,
                    Addr::unchecked(ADMIN),
                    &CDP_MockInstantiateMsg {},
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            // Instantiate Oracle
            let oracle_id = app.store_code(oracle_contract());
            let oracle_contract_addr = app
                .instantiate_contract(
                    oracle_id,
                    Addr::unchecked(ADMIN),
                    &Oracle_MockInstantiateMsg {},
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            // Instantiate Staking
            let staking_id = app.store_code(staking_contract());
            let staking_contract_addr = app
                .instantiate_contract(
                    staking_id,
                    Addr::unchecked(ADMIN),
                    &SP_MockInstantiateMsg {},
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            // Instantiate Transmuter
            let transmuter_id = app.store_code(transmuter_contract());
            let transmuter_contract_addr = app
                .instantiate_contract(
                    transmuter_id,
                    Addr::unchecked(ADMIN),
                    &Transmuter_MockInstantiateMsg {},
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            // Instantiate Discounts
            let discounts_id = app.store_code(discounts_contract());
            let msg = InstantiateMsg {
                owner: None,
                positions_contract: cdp_contract_addr.to_string(),
                oracle_contract: oracle_contract_addr.to_string(),
                staking_contract: staking_contract_addr.to_string(),
                lockdrop_contract: None,
                discount_vault_contract: None,
                ltv_disco_contract: None,
                transmuter_contract: Some(transmuter_contract_addr.to_string()),
                minimum_time_in_network: 7,
                max_discount: Some(Decimal::percent(50)),
                mbrn_at_max_discount: Some(Uint128::new(100_000_000_000u128)),
                max_boost: Some(Decimal::percent(9)),
                stable_backing_max_discount: Some(Decimal::percent(75)),
                stable_backing_first_month_discount: Some(Decimal::percent(45)),
                stable_backing_remaining_discount: Some(Decimal::percent(30)),
                stable_backing_curve_duration_days: Some(90),
                stable_backing_first_month_days: Some(30),
                stable_backing_discountable_debt_multiplier: Some(18),
                stable_backing_transmuter_balance_multiplier: Some(Decimal::percent(200)),
            };

            let discounts_contract_addr = app
                .instantiate_contract(
                    discounts_id,
                    Addr::unchecked(ADMIN),
                    &msg,
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            let discounts_contract = DiscountsContract(discounts_contract_addr);
            clear_transmuter_deposits();

            (app, discounts_contract, transmuter_contract_addr)
        }

        #[test]
        fn test_no_deposits_returns_zero_discount() {
            let (app, discounts_contract, _) = setup_with_transmuter();

            let response: StableBackingDiscountsResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::StableBackingDiscounts {
                        user: USER.to_string(),
                        debt_amount: Uint128::new(1000),
                    },
                )
                .unwrap();

            assert_eq!(response.discount, Decimal::zero());
        }

        #[test]
        fn test_deposit_at_start_has_zero_discount() {
            let (mut app, discounts_contract, _) = setup_with_transmuter();
            let current_time = app.block_info().time.seconds();

            // Set a deposit made just now
            set_transmuter_deposits(
                USER.to_string(),
                vec![UserDeposit {
                    amount: Uint128::new(100),
                    deposit_time: current_time,
                    locked: None,
                    start_time: current_time,
                }],
            );

            let response: StableBackingDiscountsResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::StableBackingDiscounts {
                        user: USER.to_string(),
                        debt_amount: Uint128::new(1000),
                    },
                )
                .unwrap();

            // At day 0, discount should be 0%
            assert_eq!(response.discount, Decimal::zero());
        }

        #[test]
        fn test_deposit_after_first_month_has_45_percent_discount() {
            let (mut app, discounts_contract, _) = setup_with_transmuter();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (30 * SECONDS_PER_DAY); // 30 days ago

            set_transmuter_deposits(
                USER.to_string(),
                vec![UserDeposit {
                    amount: Uint128::new(100),
                    deposit_time,
                    locked: None,
                    start_time: deposit_time,
                }],
            );

            let response: StableBackingDiscountsResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::StableBackingDiscounts {
                        user: USER.to_string(),
                        debt_amount: Uint128::new(1000),
                    },
                )
                .unwrap();

            // After 30 days, should have 45% discount (60% of 75%)
            // Capacity: 100 * 2 * 18 = 3600
            // Debt: 1000, which is less than capacity, so full 45% discount
            assert_eq!(response.discount, Decimal::percent(45));
        }

        #[test]
        fn test_deposit_after_90_days_has_max_discount() {
            let (mut app, discounts_contract, _) = setup_with_transmuter();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (90 * SECONDS_PER_DAY); // 90 days ago

            set_transmuter_deposits(
                USER.to_string(),
                vec![UserDeposit {
                    amount: Uint128::new(100),
                    deposit_time,
                    locked: None,
                    start_time: deposit_time,
                }],
            );

            let response: StableBackingDiscountsResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::StableBackingDiscounts {
                        user: USER.to_string(),
                        debt_amount: Uint128::new(1000),
                    },
                )
                .unwrap();

            // After 90 days, should have max 75% discount
            assert_eq!(response.discount, Decimal::percent(75));
        }

        #[test]
        fn test_proportional_discount_when_debt_exceeds_capacity() {
            let (mut app, discounts_contract, _) = setup_with_transmuter();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (90 * SECONDS_PER_DAY); // 90 days ago

            // Deposit of 100, capacity = 100 * 2 * 18 = 3600
            set_transmuter_deposits(
                USER.to_string(),
                vec![UserDeposit {
                    amount: Uint128::new(100),
                    deposit_time,
                    locked: None,
                    start_time: deposit_time,
                }],
            );

            // Debt of 7200 (double the capacity)
            let response: StableBackingDiscountsResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::StableBackingDiscounts {
                        user: USER.to_string(),
                        debt_amount: Uint128::new(7200),
                    },
                )
                .unwrap();

            // Max discount is 75%, but debt is 2x capacity, so discount should be 75% / 2 = 37.5%
            assert_eq!(response.discount, Decimal::percent(3750) / Decimal::from_ratio(100u128, 1u128));
        }

        #[test]
        fn test_multiple_deposits_weighted_average() {
            let (mut app, discounts_contract, _) = setup_with_transmuter();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;

            // One deposit at 30 days (45% discount), one at 90 days (75% discount)
            set_transmuter_deposits(
                USER.to_string(),
                vec![
                    UserDeposit {
                        amount: Uint128::new(100), // Capacity: 3600
                        deposit_time: current_time - (30 * SECONDS_PER_DAY),
                        locked: None,
                        start_time: current_time - (30 * SECONDS_PER_DAY),
                    },
                    UserDeposit {
                        amount: Uint128::new(100), // Capacity: 3600
                        deposit_time: current_time - (90 * SECONDS_PER_DAY),
                        locked: None,
                        start_time: current_time - (90 * SECONDS_PER_DAY),
                    },
                ],
            );

            let response: StableBackingDiscountsResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::StableBackingDiscounts {
                        user: USER.to_string(),
                        debt_amount: Uint128::new(1000),
                    },
                )
                .unwrap();

            // Weighted average: (45% * 3600 + 75% * 3600) / 7200 = 60%
            assert_eq!(response.discount, Decimal::percent(60));
        }

        #[test]
        fn test_deposit_at_15_days_has_half_first_month_discount() {
            let (mut app, discounts_contract, _) = setup_with_transmuter();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (15 * SECONDS_PER_DAY); // 15 days ago

            set_transmuter_deposits(
                USER.to_string(),
                vec![UserDeposit {
                    amount: Uint128::new(100),
                    deposit_time,
                    locked: None,
                    start_time: deposit_time,
                }],
            );

            let response: StableBackingDiscountsResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::StableBackingDiscounts {
                        user: USER.to_string(),
                        debt_amount: Uint128::new(1000),
                    },
                )
                .unwrap();

            // At 15 days (half of first month), should have 22.5% discount (half of 45%)
            assert_eq!(response.discount, Decimal::percent(2250) / Decimal::from_ratio(100u128, 1u128));
        }

        #[test]
        fn test_deposit_at_60_days_has_partial_remaining_discount() {
            let (mut app, discounts_contract, _) = setup_with_transmuter();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (60 * SECONDS_PER_DAY); // 60 days ago

            set_transmuter_deposits(
                USER.to_string(),
                vec![UserDeposit {
                    amount: Uint128::new(100),
                    deposit_time,
                    locked: None,
                    start_time: deposit_time,
                }],
            );

            let response: StableBackingDiscountsResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::StableBackingDiscounts {
                        user: USER.to_string(),
                        debt_amount: Uint128::new(1000),
                    },
                )
                .unwrap();

            // At 60 days: 45% (first month) + (30 days / 60 days) * 30% = 45% + 15% = 60%
            assert_eq!(response.discount, Decimal::percent(60));
        }

        #[test]
        fn test_update_config_changes_constants() {
            let (mut app, discounts_contract, _) = setup_with_transmuter();

            // Update config to change max discount to 50%
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                positions_contract: None,
                oracle_contract: None,
                staking_contract: None,
                lockdrop_contract: None,
                discount_vault_contract: None,
                ltv_disco_contract: None,
                transmuter_contract: None,
                max_discount: None,
                mbrn_at_max_discount: None,
                max_boost: None,
                minimum_time_in_network: None,
                static_discount: None,
                stable_backing_max_discount: Some(Decimal::percent(50)),
                stable_backing_first_month_discount: Some(Decimal::percent(30)),
                stable_backing_remaining_discount: Some(Decimal::percent(20)),
                stable_backing_curve_duration_days: Some(60),
                stable_backing_first_month_days: Some(20),
                stable_backing_discountable_debt_multiplier: Some(20),
                stable_backing_transmuter_balance_multiplier: Some(Decimal::percent(300)),
            });

            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            // Set a deposit at 60 days (should now use new 60-day curve)
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (60 * SECONDS_PER_DAY);

            set_transmuter_deposits(
                USER.to_string(),
                vec![UserDeposit {
                    amount: Uint128::new(100),
                    deposit_time,
                    locked: None,
                    start_time: deposit_time,
                }],
            );

            let response: StableBackingDiscountsResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::StableBackingDiscounts {
                        user: USER.to_string(),
                        debt_amount: Uint128::new(1000),
                    },
                )
                .unwrap();

            // With new config: 30% (first month) + (40 days / 40 days) * 20% = 30% + 20% = 50% (max)
            assert_eq!(response.discount, Decimal::percent(50));
        }
    }

    mod mbrn_discount_time_curve {
        use super::*;
        use membrane::system_discounts::{UserDiscountResponse, QueryMsg};
        use membrane::ltv_disco::{UserDepositInfo, BackingDeposit};
        use membrane::types::Locked;
        use membrane::ltv_disco::DepositLVTTracking;

        fn setup_with_ltv_disco() -> (App, DiscountsContract, Addr) {
            let mut app = mock_app();

            // Instantiate CDP
            let cdp_id = app.store_code(cdp_contract());
            let cdp_contract_addr = app
                .instantiate_contract(
                    cdp_id,
                    Addr::unchecked(ADMIN),
                    &CDP_MockInstantiateMsg {},
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            // Instantiate Oracle
            let oracle_id = app.store_code(oracle_contract());
            let oracle_contract_addr = app
                .instantiate_contract(
                    oracle_id,
                    Addr::unchecked(ADMIN),
                    &Oracle_MockInstantiateMsg {},
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            // Instantiate Staking
            let staking_id = app.store_code(staking_contract());
            let staking_contract_addr = app
                .instantiate_contract(
                    staking_id,
                    Addr::unchecked(ADMIN),
                    &SP_MockInstantiateMsg {},
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            // Instantiate LTV Disco
            let ltv_disco_id = app.store_code(ltv_disco_contract());
            let ltv_disco_contract_addr = app
                .instantiate_contract(
                    ltv_disco_id,
                    Addr::unchecked(ADMIN),
                    &LTVDisco_MockInstantiateMsg {},
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            // Instantiate Discounts
            let discounts_id = app.store_code(discounts_contract());
            let msg = InstantiateMsg {
                owner: None,
                positions_contract: cdp_contract_addr.to_string(),
                oracle_contract: oracle_contract_addr.to_string(),
                staking_contract: staking_contract_addr.to_string(),
                lockdrop_contract: None,
                discount_vault_contract: None,
                ltv_disco_contract: Some(ltv_disco_contract_addr.to_string()),
                transmuter_contract: None,
                minimum_time_in_network: 7,
                max_discount: Some(Decimal::percent(50)),
                mbrn_at_max_discount: Some(Uint128::new(100_000_000_000u128)),
                max_boost: Some(Decimal::percent(9)),
                stable_backing_max_discount: Some(Decimal::percent(75)),
                stable_backing_first_month_discount: Some(Decimal::percent(45)),
                stable_backing_remaining_discount: Some(Decimal::percent(30)),
                stable_backing_curve_duration_days: Some(90),
                stable_backing_first_month_days: Some(30),
                stable_backing_discountable_debt_multiplier: Some(18),
                stable_backing_transmuter_balance_multiplier: Some(Decimal::percent(200)),
            };

            let discounts_contract_addr = app
                .instantiate_contract(
                    discounts_id,
                    Addr::unchecked(ADMIN),
                    &msg,
                    &[],
                    "test",
                    None,
                )
                .unwrap();

            let discounts_contract = DiscountsContract(discounts_contract_addr);
            clear_ltv_disco_deposits();

            (app, discounts_contract, ltv_disco_contract_addr)
        }

        fn create_backing_deposit(
            start_time: u64,
            vault_tokens: Uint128,
            locked: Option<Locked>,
        ) -> BackingDeposit {
            BackingDeposit {
                user: Addr::unchecked(USER),
                vault_tokens,
                locked_vault_tokens: vault_tokens,
                max_borrow_ltv: Decimal::percent(80),
                last_claimed: 0,
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
                    daily_delta: cosmwasm_std::Int128::zero(),
                    time_cliffs: vec![],
                },
            }
        }

        #[test]
        fn test_no_mbrn_deposits_returns_zero_discount() {
            let (app, discounts_contract, _) = setup_with_ltv_disco();

            let response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: USER.to_string(),
                    },
                )
                .unwrap();

            assert_eq!(response.discount, Decimal::zero());
        }

        #[test]
        fn test_deposit_at_start_has_zero_discount() {
            let (mut app, discounts_contract, _) = setup_with_ltv_disco();
            let current_time = app.block_info().time.seconds();

            set_ltv_disco_deposits(
                USER.to_string(),
                vec![UserDepositInfo {
                    asset: "mbrn".to_string(),
                    ltv: Decimal::percent(80),
                    max_borrow_ltv: Decimal::percent(80),
                    deposit_id: Uint128::one(),
                    deposit: create_backing_deposit(current_time, Uint128::new(100), None),
                    deposit_tokens: Uint128::new(100), // 1:1 conversion in tests
                }],
            );

            let response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: USER.to_string(),
                    },
                )
                .unwrap();

            // At day 0, discount should be 0%
            assert_eq!(response.discount, Decimal::zero());
        }

        #[test]
        fn test_deposit_after_first_month_has_45_percent_discount() {
            let (mut app, discounts_contract, _) = setup_with_ltv_disco();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (30 * SECONDS_PER_DAY); // 30 days ago

            set_ltv_disco_deposits(
                USER.to_string(),
                vec![UserDepositInfo {
                    asset: "mbrn".to_string(),
                    ltv: Decimal::percent(80),
                    max_borrow_ltv: Decimal::percent(80),
                    deposit_id: Uint128::one(),
                    deposit: create_backing_deposit(deposit_time, Uint128::new(100), None),
                    deposit_tokens: Uint128::new(100), // 1:1 conversion in tests
                }],
            );

            let response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: USER.to_string(),
                    },
                )
                .unwrap();

            // After 30 days, should have 45% discount (60% of 75%)
            assert_eq!(response.discount, Decimal::percent(45));
        }

        #[test]
        fn test_deposit_after_90_days_has_max_discount() {
            let (mut app, discounts_contract, _) = setup_with_ltv_disco();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (90 * SECONDS_PER_DAY); // 90 days ago

            set_ltv_disco_deposits(
                USER.to_string(),
                vec![UserDepositInfo {
                    asset: "mbrn".to_string(),
                    ltv: Decimal::percent(80),
                    max_borrow_ltv: Decimal::percent(80),
                    deposit_id: Uint128::one(),
                    deposit: create_backing_deposit(deposit_time, Uint128::new(100), None),
                    deposit_tokens: Uint128::new(100), // 1:1 conversion in tests
                }],
            );

            let response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: USER.to_string(),
                    },
                )
                .unwrap();

            // After 90 days, should have max discount (75%)
            assert_eq!(response.discount, Decimal::percent(75));
        }

        #[test]
        fn test_locked_deposit_accelerates_through_curve() {
            let (mut app, discounts_contract, _) = setup_with_ltv_disco();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (10 * SECONDS_PER_DAY); // 10 days ago
            let locked_until = current_time + (30 * SECONDS_PER_DAY); // Locked for 30 more days

            set_ltv_disco_deposits(
                USER.to_string(),
                vec![UserDepositInfo {
                    asset: "mbrn".to_string(),
                    ltv: Decimal::percent(80),
                    max_borrow_ltv: Decimal::percent(80),
                    deposit_id: Uint128::one(),
                    deposit: create_backing_deposit(
                        deposit_time,
                        Uint128::new(100),
                        Some(Locked {
                            locked_until,
                            perpetual_lock: None,
                            intended_lock_days: Some(30),
                        }),
                    ),
                    deposit_tokens: Uint128::new(100), // 1:1 conversion in tests
                }],
            );

            let response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: USER.to_string(),
                    },
                )
                .unwrap();

            // 10 days since deposit + 30 days lock = 40 effective days
            // At 40 days: 30 days in first month (45% * 30/30 = 45%) + 10 days in remaining (30% * 10/60 = 5%)
            // Total: 50% discount
            // But we need to check the actual calculation - let's verify it's greater than just 10 days
            // 10 days alone would give: 45% * 10/30 = 15%
            // With 30 day lock: 40 days effective = 45% + (30% * 10/60) = 45% + 5% = 50%
            assert!(response.discount > Decimal::percent(15)); // Should be more than without lock
            // Should be around 50% (45% from first month + 5% from remaining)
            assert!(response.discount >= Decimal::percent(48));
            assert!(response.discount <= Decimal::percent(52));
        }

        #[test]
        fn test_expired_lock_uses_only_deposit_time() {
            let (mut app, discounts_contract, _) = setup_with_ltv_disco();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit_time = current_time - (30 * SECONDS_PER_DAY); // 30 days ago
            let locked_until = current_time - (10 * SECONDS_PER_DAY); // Lock expired 10 days ago

            set_ltv_disco_deposits(
                USER.to_string(),
                vec![UserDepositInfo {
                    asset: "mbrn".to_string(),
                    ltv: Decimal::percent(80),
                    max_borrow_ltv: Decimal::percent(80),
                    deposit_id: Uint128::one(),
                    deposit: create_backing_deposit(
                        deposit_time,
                        Uint128::new(100),
                        Some(Locked {
                            locked_until,
                            perpetual_lock: None,
                            intended_lock_days: Some(30),
                        }),
                    ),
                    deposit_tokens: Uint128::new(100), // 1:1 conversion in tests
                }],
            );

            let response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: USER.to_string(),
                    },
                )
                .unwrap();

            // Expired lock should not accelerate - should use only 30 days
            // 30 days = 45% discount (full first month)
            assert_eq!(response.discount, Decimal::percent(45));
        }

        #[test]
        fn test_multiple_deposits_weighted_average() {
            let (mut app, discounts_contract, _) = setup_with_ltv_disco();
            let current_time = app.block_info().time.seconds();
            const SECONDS_PER_DAY: u64 = 86_400;
            let deposit1_time = current_time - (30 * SECONDS_PER_DAY); // 30 days ago
            let deposit2_time = current_time - (60 * SECONDS_PER_DAY); // 60 days ago

            set_ltv_disco_deposits(
                USER.to_string(),
                vec![
                    UserDepositInfo {
                        asset: "mbrn".to_string(),
                        ltv: Decimal::percent(80),
                        max_borrow_ltv: Decimal::percent(80),
                        deposit_id: Uint128::one(),
                        deposit: create_backing_deposit(deposit1_time, Uint128::new(100), None),
                        deposit_tokens: Uint128::new(100), // 1:1 conversion in tests
                    },
                    UserDepositInfo {
                        asset: "mbrn".to_string(),
                        ltv: Decimal::percent(80),
                        max_borrow_ltv: Decimal::percent(80),
                        deposit_id: Uint128::new(2),
                        deposit: create_backing_deposit(deposit2_time, Uint128::new(200), None),
                        deposit_tokens: Uint128::new(200), // 1:1 conversion in tests
                    },
                ],
            );

            let response: UserDiscountResponse = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: USER.to_string(),
                    },
                )
                .unwrap();

            // Deposit 1: 30 days = 45% discount, weight 100
            // Deposit 2: 60 days = 45% + (30% * 30/60) = 45% + 15% = 60% discount, weight 200
            // Weighted average: (45% * 100 + 60% * 200) / 300 = (45 + 120) / 300 = 165 / 300 = 55%
            // But let's check the actual calculation
            // 30 days: 45% * 30/30 = 45%
            // 60 days: 45% + 30% * 30/60 = 45% + 15% = 60%
            // Weighted: (45 * 100 + 60 * 200) / 300 = 16500 / 300 = 55%
            assert!(response.discount >= Decimal::percent(54));
            assert!(response.discount <= Decimal::percent(56));
        }
    }
}
