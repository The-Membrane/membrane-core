#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use crate::helpers::DiscountsContract;

    use membrane::positions::{PositionsResponse, BasketResponse};
    use membrane::system_discounts::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::lockdrop::UserResponse;
    use membrane::stability_pool::{DepositResponse, ClaimsResponse};
    use membrane::staking::{StakerResponse, RewardsResponse, Config as Staking_Config};
    use membrane::oracle::PriceResponse;
    use membrane::discount_vault::UserResponse as Discount_UserResponse;
    use membrane::types::{Asset, AssetInfo, Position, DebtTokenAsset, Deposit};

    use cosmwasm_std::{
        to_binary, Addr, Binary, Empty, Response, StdResult, Uint128, Decimal,
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
            //All positions from a user
            basket_id: Option<Uint128>,
            user: String,
            limit: Option<u32>,
        },
        GetBasket {
            basket_id: Uint128,
        },
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
                        basket_id,
                        user,
                        limit,
                    } => {
                        Ok(to_binary(&PositionsResponse {
                            user: String::from(USER),
                            positions: vec![
                                Position { 
                                    position_id: Uint128::new(1),
                                    collateral_assets: vec![],
                                    credit_amount: Uint128::new(500),
                                    basket_id: Uint128::new(1),
                                }
                            ],
                        })?)
                    },
                    CDP_MockQueryMsg::GetBasket { basket_id } => {
                        Ok(to_binary(&BasketResponse {
                            owner: String::from(""),
                            basket_id: String::from(""),
                            current_position_id: String::from(""),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![],
                            credit_asset: Asset { info: AssetInfo::NativeToken { denom: String::from("credit") }, amount: Uint128::zero() },
                            credit_price: Decimal::one(),
                            liq_queue: String::from(""),
                            base_interest_rate: Decimal::zero(),
                            liquidity_multiplier: Decimal::zero(),
                            desired_debt_cap_util: Decimal::zero(),
                            pending_revenue: Uint128::zero(),
                            negative_rates: false,
                            cpc_margin_of_error: Decimal::zero(),
                            multi_asset_supply_caps: vec![],
                            frozen: false,
                            rev_to_stakers: true,
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
                            staking_rate: Decimal::zero(),
                            fee_wait_period: 0,
                            unstaking_period: 0,
                            positions_contract: None,
                            vesting_contract: None,
                            governance_contract: None,
                            osmosis_proxy: None,
                            dex_router: None,
                            max_spread: None,
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
        AssetDeposits { user: String, asset_info: AssetInfo },
        UnclaimedIncentives { user: String, asset_info: AssetInfo },
        UserClaims { user: String },
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
                        asset_info,
                    } => Ok(to_binary(&DepositResponse {
                        asset: asset_info,
                        deposits: vec![Deposit {
                            user: Addr::unchecked(USER),
                            amount: Decimal::percent(222_00),
                            deposit_time: 0u64,
                            last_accrued: 0u64,
                            unstake_time: None,
                        }],
                    })?),
                    SP_MockQueryMsg::UnclaimedIncentives {
                        user: _,
                        asset_info: _,
                    } => Ok(to_binary(&Uint128::new(5))?),
                    SP_MockQueryMsg::UserClaims {
                        user: _,
                    } => Ok(to_binary(&ClaimsResponse {
                        claims: vec![
                            Asset {
                                info: AssetInfo::NativeToken { denom: String::from("juicy_claims") },
                                amount: Uint128::new(4),
                            }
                        ],
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock LP Lockdrop Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Lockdrop_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Lockdrop_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Lockdrop_MockQueryMsg {
        User { user: String },
    }

    pub fn lp_lockdrop_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Lockdrop_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Lockdrop_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Lockdrop_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Lockdrop_MockQueryMsg::User {
                        user: _,
                    } => Ok(to_binary(&UserResponse {
                        user: String::from(""),
                        total_debt_token: DebtTokenAsset {
                            info: AssetInfo::NativeToken { denom: String::from("") },
                            amount: Uint128::new(11),
                            basket_id: Uint128::one(),
                        },
                        deposits: vec![],
                        lock_up_distributions: vec![],
                        accrued_incentives: Asset {
                            info: AssetInfo::NativeToken { denom: String::from("mbrn_denom") },
                            amount: Uint128::new(12),
                        },
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
                        premium_user_value: Decimal::percent(13_00),
                        deposits: vec![],
                        lock_up_distributions: vec![],
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

        //Instaniate LP Lockdrop
        let lockdrop_id = app.store_code(lp_lockdrop_contract());

        let lockdrop_contract_addr = app
            .instantiate_contract(
                lockdrop_id,
                Addr::unchecked(ADMIN),
                &Lockdrop_MockInstantiateMsg {},
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
            basket_id: Uint128::one(),
            oracle_contract: oracle_contract_addr.to_string(),
            staking_contract: staking_contract_addr.to_string(),
            stability_pool_contract: sp_contract_addr.to_string(),
            lockdrop_contract: lockdrop_contract_addr.to_string(),
            discount_vault_contract: discount_contract_addr.to_string(),
            
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
        use membrane::system_discounts::Config;

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
            assert_eq!(discount.to_string(), String::from("0.998"));
        }

        #[test]
        fn update_config() {
            let (mut app, discounts_contract) = proper_instantiate();

            //Successful UpdateConfig
            let msg = ExecuteMsg::UpdateConfig { 
                owner: Some(String::from("new_owner")), 
                basket_id: Some(Uint128::zero()),
                mbrn_denom: Some(String::from("new_denom")), 
                positions_contract: Some(String::from("new_pos_contract")),                 
                oracle_contract: Some(String::from("new_oracle_contract")), 
                staking_contract: Some(String::from("new_staking_contract")), 
                stability_pool_contract: Some(String::from("new_stability_pool_contract")), 
                lockdrop_contract: Some(String::from("new_lockdrop_contract")), 
                discount_vault_contract: Some(String::from("new_discount_vault_contract")), 
            };
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
                    owner: Addr::unchecked("new_owner"), 
                    basket_id: Uint128::zero(),
                    mbrn_denom: String::from("new_denom"),
                    positions_contract: Addr::unchecked("new_pos_contract"),                
                    oracle_contract: Addr::unchecked("new_oracle_contract"), 
                    staking_contract: Addr::unchecked("new_staking_contract"), 
                    stability_pool_contract: Addr::unchecked("new_stability_pool_contract"), 
                    lockdrop_contract: Addr::unchecked("new_lockdrop_contract"), 
                    discount_vault_contract: Addr::unchecked("new_discount_vault_contract"), 
            });
        }
    }
}