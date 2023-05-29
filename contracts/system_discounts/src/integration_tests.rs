#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use crate::helpers::DiscountsContract;

    use membrane::cdp::PositionResponse;
    use membrane::system_discounts::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::stability_pool::ClaimsResponse;
    use membrane::staking::{StakerResponse, RewardsResponse, Config as Staking_Config};
    use membrane::oracle::PriceResponse;
    use membrane::discount_vault::UserResponse as Discount_UserResponse;
    use membrane::types::{Asset, AssetInfo, AssetPool, Basket, Deposit, StakeDistribution};

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

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {
        GetUserPositions {
            user: String,
            limit: Option<u32>,
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
                    CDP_MockQueryMsg::GetUserPositions { 
                        user,
                        limit,
                    } => {
                        Ok(to_binary(&vec![PositionResponse {
                            position_id: Uint128::new(1),
                            collateral_assets: vec![],
                            credit_amount: Uint128::new(500),
                            cAsset_ratios: vec![],
                            basket_id: Uint128::new(1),
                            avg_borrow_LTV: Decimal::zero(),
                            avg_max_LTV: Decimal::zero(),
                        }])?)
                    },
                    CDP_MockQueryMsg::GetBasket { } => {
                        Ok(to_binary(&Basket {
                            basket_id: Uint128::one(),
                            current_position_id: Uint128::one(),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![],
                            credit_asset: Asset { info: AssetInfo::NativeToken { denom: String::from("credit") }, amount: Uint128::zero() },
                            credit_price: Decimal::one(),
                            liq_queue: None,
                            base_interest_rate: Decimal::zero(),
                            pending_revenue: Uint128::zero(),
                            negative_rates: false,
                            cpc_margin_of_error: Decimal::zero(),
                            multi_asset_supply_caps: vec![],
                            frozen: false,
                            rev_to_stakers: true,
                            credit_last_accrued: 0,
                            rates_last_accrued: 0,
                            oracle_set: false,
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
                        Ok(to_binary(&PriceResponse {
                            prices: vec![],
                            price: Decimal::one(),
                        })?)
                        
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
        StakerRewards { staker: String },
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
                            fee_wait_period: 0,
                            unstaking_period: 0,
                            positions_contract: None,
                            auction_contract: None,
                            vesting_contract: None,
                            governance_contract: None,
                            osmosis_proxy: None,
                        })?)
                    },
                    Staking_MockQueryMsg::StakerRewards { staker } => {
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
                        user: Addr::unchecked(USER),
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
                        deposits: vec![],
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
            stability_pool_contract: sp_contract_addr.to_string(),
            lockdrop_contract: None,
            discount_vault_contract: Some(discount_contract_addr.to_string()),
            minimum_time_in_network: 7,
            
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

        use cosmwasm_std::Decimal;
        use membrane::system_discounts::{Config, UpdateConfig};

        use super::*;
        

        #[test]
        fn get_discount() {
            let (app, discounts_contract) = proper_instantiate();
            
            //Query Liquidity
            let discount: Decimal = app
                .wrap()
                .query_wasm_smart(
                    discounts_contract.addr(),
                    &QueryMsg::UserDiscount {
                        user: String::from("uzzer"),
                    },
                )
                .unwrap();
            assert_eq!(discount.to_string(), String::from("0.486"));
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
                stability_pool_contract: Some(String::from("new_stability_pool_contract")), 
                lockdrop_contract: Some(String::from("new_lockdrop_contract")), 
                discount_vault_contract: Some(String::from("new_discount_vault_contract")), 
                minimum_time_in_network: Some(14),
            });
            let cosmos_msg = discounts_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
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
                    stability_pool_contract: Addr::unchecked("new_stability_pool_contract"), 
                    lockdrop_contract: Some(Addr::unchecked("new_lockdrop_contract")), 
                    discount_vault_contract: Some(Addr::unchecked("new_discount_vault_contract")), 
                    minimum_time_in_network: 14,
            });

            //Successful ownership transfer
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: None,
                positions_contract: None,
                oracle_contract: None,
                staking_contract: None, 
                stability_pool_contract: None,
                lockdrop_contract: None,
                discount_vault_contract: None,
                minimum_time_in_network: None,
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
                    stability_pool_contract: Addr::unchecked("new_stability_pool_contract"), 
                    lockdrop_contract: Some(Addr::unchecked("new_lockdrop_contract")), 
                    discount_vault_contract: Some(Addr::unchecked("new_discount_vault_contract")), 
                    minimum_time_in_network: 14,
            });
        }
    }
}
