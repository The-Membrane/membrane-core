#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use crate::helpers::MarginContract;

    use membrane::apollo_router::SwapToAssetsInput;
    use membrane::margin_proxy::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::cdp::{PositionsResponse, PositionResponse};
    use membrane::types::{AssetInfo, Position, cAsset, Asset, Basket};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Empty, Response, StdResult, Uint128, Decimal, attr,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //Margin Contract
    pub fn margin_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contracts::execute,
            crate::contracts::instantiate,
            crate::contracts::query,
        ).with_reply(crate::contracts::reply);

        Box::new(contract)
    }

    //Mock Positions Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockExecuteMsg {
        Deposit {
            position_id: Option<Uint128>, //If the user wants to create a new/separate position, no position id is passed
            position_owner: Option<String>,
        },
        IncreaseDebt {
            position_id: Uint128,
            amount: Option<Uint128>,
            LTV: Option<Decimal>,
            mint_to_addr: Option<String>,
        },
        ClosePosition {
            position_id: Uint128,
            send_to: Option<String>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct CDP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {
        GetUserPositions {
            //All positions from a user
            user: String,
            limit: Option<u32>,
        },
        GetPosition {
            //Singular position
            position_id: Uint128,
            position_owner: String,
        },
        GetBasket {},
    }

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    CDP_MockExecuteMsg::Deposit {
                        position_id,
                        position_owner
                    } => Ok(Response::default().add_attributes(vec![
                                attr("position_id", position_id.unwrap_or_else(|| Uint128::one())),
                                attr("position_owner", position_owner.unwrap_or_else(|| String::from(USER))),
                            ])),
                    CDP_MockExecuteMsg::IncreaseDebt {
                        position_id,
                        amount,
                        LTV,
                        mint_to_addr,
                    } => Ok(
                        Response::new()
                            .add_attributes(vec![
                                attr("total_loan", "1000"),
                                attr("increased_by", "2000000"),
                                attr("user", USER),
                            ])
                    ),
                    CDP_MockExecuteMsg::ClosePosition {
                        position_id,
                        send_to,
                    } => Ok(
                        Response::new()
                            .add_attributes(vec![
                                attr("basket_id", "1"),
                                attr("position_id", "1"),
                                attr("user", USER),
                            ])
                    ),
                }
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
                            credit_amount: Uint128::new(1),  
                            cAsset_ratios: vec![],
                            basket_id: Uint128::new(1),
                            avg_borrow_LTV: Decimal::zero(),
                            avg_max_LTV: Decimal::zero(),                              
                        }])?)
                    },
                    CDP_MockQueryMsg::GetPosition { 
                        position_id,
                        position_owner,
                    } => {
                        Ok(to_binary(&PositionResponse {
                            position_id,
                            collateral_assets: vec![
                                cAsset { 
                                    asset: Asset {
                                        amount: Uint128::zero(),
                                        info: AssetInfo::NativeToken { denom: String::from("debit") },
                                    }, 
                                    max_borrow_LTV: Decimal::zero(), 
                                    max_LTV: Decimal::zero(),  
                                    rate_index: Decimal::zero(), 
                                    pool_info: None,
                                },
                            ],
                            cAsset_ratios: vec![ Decimal::one() ],
                            credit_amount: Uint128::new(1),
                            avg_borrow_LTV: Decimal::zero(),
                            avg_max_LTV: Decimal::zero(),
                            basket_id: Uint128::one(),
                        })?)
                    },
                    CDP_MockQueryMsg::GetBasket { } => {
                        Ok(to_binary(&Basket {
                            basket_id: Uint128::zero(),
                            current_position_id: Uint128::zero(),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![],
                            credit_asset: Asset { info: AssetInfo::NativeToken { denom: String::from("credit") }, amount: Uint128::zero() },
                            credit_price: Decimal::zero(),
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
                            oracle_set: true,
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    //Mock Router Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Router_MockExecuteMsg {
        Swap {
            to: SwapToAssetsInput,
            max_spread: Option<Decimal>,
            recipient: Option<String>,
            hook_msg: Option<Binary>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Router_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Router_MockQueryMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct MockResponse {}

    pub fn router_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Router_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Router_MockExecuteMsg::Swap {
                        to,
                        max_spread,
                        recipient,
                        hook_msg
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Router_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _: Router_MockQueryMsg| -> StdResult<Binary> { to_binary(&MockResponse {}) },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![coin(100_000, "debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract2"),
                vec![coin(100_000_000, "credit")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, MarginContract) {
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

        //Instantiate Router contract
        let router_id = app.store_code(router_contract());

        let router_contract_addr = app
            .instantiate_contract(
                router_id,
                Addr::unchecked(ADMIN),
                &Router_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Margin contract
        let margin_id = app.store_code(margin_contract());

        let msg = InstantiateMsg {
            owner: None,
            apollo_router_contract: router_contract_addr.to_string(),
            positions_contract: cdp_contract_addr.to_string(),
            max_slippage: Decimal::percent(10),
        };

        let margin_contract_addr = app
            .instantiate_contract(
                margin_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "test",
                None,
            )
            .unwrap();

        let margin_contract = MarginContract(margin_contract_addr);

        (app, margin_contract)
    }

    mod margin {

        use cosmwasm_std::coins;
        use membrane::margin_proxy::Config;

        use super::*;

        #[test]
        fn deposit() {
            let (mut app, margin_contract) = proper_instantiate();

            //New deposit
            let msg = ExecuteMsg::Deposit { position_id: None };
            let cosmos_msg = margin_contract.call(msg, coins(1_000, "debit")).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query contract to assert position_info was saved to the User
            let positions: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(
                    margin_contract.addr(),
                    &QueryMsg::GetUserPositions { user: String::from(USER) },
                )
                .unwrap();
            assert_eq!(positions[0].position_id, Uint128::new(1));
            //
            let positions: Vec<Uint128> = app
                .wrap()
                .query_wasm_smart(
                    margin_contract.addr(),
                    &QueryMsg::GetPositionIDs { limit: Some(1), start_after: None },
                )
                .unwrap();
            assert_eq!(positions[0], Uint128::new(1));
            
            //Existing position deposit: Success
            let msg = ExecuteMsg::Deposit { position_id: Some(Uint128::new(1)) };
            let cosmos_msg = margin_contract.call(msg, coins(1_000, "debit")).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //The composition query doesn't change so can only test the success case

            //Close Position
            let msg = ExecuteMsg::ClosePosition { 
                position_id: Uint128::new(1), 
                max_spread: Decimal::percent(2),
            };
            let cosmos_msg = margin_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query contract to assert position_info was removed from User
            let positions: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(
                    margin_contract.addr(),
                    &QueryMsg::GetUserPositions { user: String::from(USER) },
                )
                .unwrap();
            assert_eq!(positions, vec![]);

        }

        #[test]
        fn loop_leverage() {
            let (mut app, margin_contract) = proper_instantiate();

            //Deposit: Success
            let msg = ExecuteMsg::Deposit { position_id: None };
            let cosmos_msg = margin_contract.call(msg, coins(1_000, "debit")).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Loop: Error User doesn't own position
            let msg = ExecuteMsg::Loop { 
                position_id: Uint128::new(2), 
                num_loops: Some(5), 
                target_LTV: Decimal::percent(40),
            };
            let cosmos_msg = margin_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Loop: Success 5 loops
            let msg = ExecuteMsg::Loop { 
                position_id: Uint128::new(1), 
                num_loops: Some(5), 
                target_LTV: Decimal::percent(40),
            };
            let cosmos_msg = margin_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();


        }

        #[test]
        fn update_config() {
            let (mut app, margin_contract) = proper_instantiate();

            //Successful AddAsset
            let msg = ExecuteMsg::UpdateConfig { 
                owner: Some(String::from("new_owner")), 
                positions_contract: Some(String::from("new_pos_contract")),
                apollo_router_contract: Some(String::from("new_router_contract")),
                max_slippage: Some(Decimal::one()),
            };
            let cosmos_msg = margin_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Query Liquidity
            let config: Config = app
                .wrap()
                .query_wasm_smart(
                    margin_contract.addr(),
                    &QueryMsg::Config {},
                )
                .unwrap();
            assert_eq!(
                config, 
                Config {
                    owner: Addr::unchecked("new_owner"), 
                    positions_contract:  Addr::unchecked("new_pos_contract"), 
                    apollo_router_contract: Addr::unchecked("new_router_contract"),
                    max_slippage: Decimal::one(),
            });
        }
    }
}
