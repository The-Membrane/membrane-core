#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use crate::helpers::DebtContract;

    use membrane::auction::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::cdp::{BasketPositionsResponse, PositionResponse};
    use membrane::oracle::PriceResponse;
    use membrane::types::{Asset, AssetInfo, Basket, DebtAuction, FeeAuction, UserInfo};

    use cosmwasm_std::{
        coin, to_json_binary, Addr, Binary, Decimal, Empty, Response, StdResult, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //Debt Contract
    pub fn debt_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contracts::execute,
            crate::contracts::instantiate,
            crate::contracts::query,
        );
        Box::new(contract)
    }

    //Mock Osmo Proxy Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockExecuteMsg {
        MintTokens {
            denom: String,
            amount: Uint128,
            mint_to_address: String,
        },
        BurnTokens {
            denom: String,
            amount: Uint128,
            burn_from_address: String,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg {  }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct MockResponse {  }

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens {
                        denom,
                        amount,
                        mint_to_address,
                    } => {
                        if amount != Uint128::new(105_319u128) && amount != Uint128::new(1_063u128) && amount != Uint128::new(3_191)
                        {
                            panic!("{}, {}, {}", denom, amount, mint_to_address)
                        }
                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::BurnTokens {
                        denom,
                        amount,
                        burn_from_address,
                    } => Ok(Response::new()),
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_json_binary(&MockResponse { })?)
            },
        );
        Box::new(contract)
    }

    //Mock Oracle Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockExecuteMsg {  }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Oracle_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockQueryMsg {
        Price {
            asset_info: AssetInfo,
            twap_timeframe: u64,
            oracle_time_limit: u64,
            basket_id: Option<Uint128>,
        }
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
                        oracle_time_limit,
                        basket_id,
                    } => {
                        if asset_info.to_string() == String::from("no_price"){
                            return Err(cosmwasm_std::StdError::generic_err("Asset has no oracle price"))
                        } else {
                            Ok(to_json_binary(&vec![PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 0,
                            }])?)
                        }
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock CDP Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockExecuteMsg {
        Repay {
            position_id: Uint128,
            position_owner: Option<String>,
            send_excess_to: Option<String>, 
        },
        FulfillBadDebt { },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct CDP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {
        GetBasket { },
        GetBasketPositions {
            start_after: Option<String>,
            limit: Option<u32>,
            user_info: Option<UserInfo>,
            user: Option<String>,
        },
    }

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    CDP_MockExecuteMsg::Repay {
                        position_id,
                        position_owner,
                        send_excess_to,
                    } => Ok(Response::default()),
                    CDP_MockExecuteMsg::FulfillBadDebt { } => Ok(Response::default()),
                }
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    CDP_MockQueryMsg::GetBasket { } => Ok(to_json_binary(&Basket {
                        basket_id: Uint128::one(),
                        current_position_id: Uint128::one(),
                        collateral_types: vec![],
                        collateral_supply_caps: vec![],
                        multi_asset_supply_caps: vec![],
                        credit_asset: membrane::types::CreditAssetBreakdown {
                            info: AssetInfo::NativeToken {
                                denom: String::from("credit_fulldenom"),
                            },
                            variable_amount: Uint128::zero(),
                            one_month_amount: Uint128::zero(),
                            three_month_amount: Uint128::zero(),
                            six_month_amount: Uint128::zero(),
                            peg_variable_amount: Uint128::zero(),
                            peg_one_month_amount: Uint128::zero(),
                            peg_three_month_amount: Uint128::zero(),
                            peg_six_month_amount: Uint128::zero(),
                        },
                        credit_price: PriceResponse {
                            prices: vec![],
                            price: Decimal::one(),
                            decimals: 0
                        },
                        liq_queue: None,
                        pending_revenue: membrane::types::PendingRevenue {
                            total_pending: Uint128::zero(),
                            per_asset_rev: vec![],
                        },
                        frozen: false,
                        distribute_revenue: true,
                        oracle_set: false,
                        pending_bad_debt: Uint128::zero(),
                    })?),
                    CDP_MockQueryMsg::GetBasketPositions { start_after, limit, user_info, user } => {
                        Ok(to_json_binary(&vec![
                            BasketPositionsResponse {
                                user: String::from("user"),
                                positions: vec![
                                    PositionResponse {
                                        position_id: Uint128::zero(),
                                        collateral_assets: vec![],
                                        cAsset_ratios: vec![],
                                        credit_amount: Uint128::zero(),
                                        rate_segments: vec![],
                                        avg_borrow_LTV: Decimal::zero(),
                                        avg_max_LTV: Decimal::zero(),
                                        deployed_to: vec![],
                                        pending_interest: Uint128::zero(),
                                        total_interest_accrued: Uint128::zero(),
                                        peg_rate_segments: vec![],
                                    }
                                ]
                            }
                        ])?)
                    }
                }

            },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &Addr::unchecked("contract3"),
                vec![coin(30_000_000_000_000, "mbrn_denom")],
            )
            .unwrap(); //contract3 = Builders contract
            bank.init_balance(
                storage,
                &Addr::unchecked("coin_God"),
                vec![coin(100_000_000, "debit"), coin(100_000_000, "2nddebit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![coin(99, "error"), coin(201_000, "credit_fulldenom"), coin(96_000, "mbrn_denom"), coin(196_000, "uosmo")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked(ADMIN),
                vec![coin(1_000_000, "credit_fulldenom"), coin(1_000_000, "fee_asset")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, DebtContract, Addr) {
        let mut app = mock_app();

        //Instaniate Osmosis Proxy
        let proxy_id = app.store_code(osmosis_proxy_contract());

        let osmosis_proxy_contract_addr = app
            .instantiate_contract(
                proxy_id,
                Addr::unchecked(ADMIN),
                &Osmo_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Oracle Contract
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

        //Instaniate CDP Contract
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

        //Instantiate Gov contract
        let debt_id = app.store_code(debt_contract());

        let msg = InstantiateMsg {
            owner: None,
            oracle_contract: oracle_contract_addr.to_string(),
            osmosis_proxy: osmosis_proxy_contract_addr.to_string(),
            positions_contract: cdp_contract_addr.to_string(),
            governance_contract: String::from("contract0"),
            staking_contract: String::from("contract0"),
            twap_timeframe: 90u64,
            mbrn_denom: String::from("mbrn_denom"),
            initial_discount: Decimal::percent(1),
            discount_increase_timeframe: 60u64,
            discount_increase: Decimal::percent(1),
            delay_window_minutes: Some(0), // No delay for tests
            revenue_distributor_contract: None,
            ltv_disco_contract: None,
            debt_contract: None,
        };

        let debt_contract_addr = app
            .instantiate_contract(debt_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let debt_contract = DebtContract(debt_contract_addr);

        (app, debt_contract, cdp_contract_addr)
    }

    mod auction {

        use super::*;
        use cosmwasm_std::BlockInfo;
        use membrane::{
            auction::{UpdateConfig, Config},
            types::{RepayPosition, UserInfo},
        };

        #[test]
        fn start_auction() {
            let (mut app, debt_contract, cdp_contract) = proper_instantiate();

            //Unauthorized StartAuction
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: Some(UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from("owner"),
                }),
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    amount: Uint128::new(100u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Successful StartAuction
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: Some(UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from("owner"),
                }),
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    amount: Uint128::new(100u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Assert DebtAuction Response
            let auction: DebtAuction = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::DebtAuction {},
                )
                .unwrap();
            assert_eq!(auction.auction_start_time, 1571797419u64);
            assert_eq!(auction.remaining_recapitalization, Uint128::new(100u128));
            assert_eq!(
                auction.repayment_positions,
                vec![RepayPosition {
                    repayment: Uint128::new(100u128),
                    position_info: UserInfo {
                        position_id: Uint128::new(1u128),
                        position_owner: String::from("owner"),
                    }
                }]
            );

            //Successful Start adding to existing auction
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: Some(UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from("owner"),
                }),
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    amount: Uint128::new(100u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(cdp_contract, cosmos_msg).unwrap();

            //Assert DebtAuction Response
            let auction: DebtAuction = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::DebtAuction {},
                )
                .unwrap();
            assert_eq!(auction.auction_start_time, 1571797419u64); //Start_time doesn't change
            assert_eq!(auction.remaining_recapitalization, Uint128::new(200u128));
            assert_eq!(
                auction.repayment_positions,
                vec![
                    RepayPosition {
                        repayment: Uint128::new(100u128),
                        position_info: UserInfo {
                            position_id: Uint128::new(1u128),
                            position_owner: String::from("owner"),
                        }
                    },
                    RepayPosition {
                        repayment: Uint128::new(100u128),
                        position_info: UserInfo {
                            position_id: Uint128::new(1u128),
                            position_owner: String::from("owner"),
                        }
                    }
                ]
            );

            //Successful FeeAuction
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: None,
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    amount: Uint128::new(100u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Assert FeeAuction Response
            let auction: Vec<FeeAuction> = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::OngoingFeeAuctions {
                        auction_asset: Some(AssetInfo::NativeToken {
                            denom: String::from("credit_fulldenom"),
                        }),
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();
            assert_eq!(auction[0].auction_start_time, 1603333419);
            assert_eq!(auction[0].auction_asset.amount, Uint128::new(100u128));

            //Successful Start adding to existing auction
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: None,
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    amount: Uint128::new(100u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Assert FeeAuction Response
            let auction: Vec<FeeAuction> = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::OngoingFeeAuctions {
                        auction_asset: Some(AssetInfo::NativeToken {
                            denom: String::from("credit_fulldenom"),
                        }),
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();
            assert_eq!(auction[0].auction_start_time, 1603333419); //Start_time doesn't change
            assert_eq!(auction[0].auction_asset.amount, Uint128::new(200u128));

        }

        #[test]
        fn swap_for_fee(){
            let (mut app, debt_contract, cdp_contract) = proper_instantiate();

            //Successful StartAuction: FeeAuction
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: None,
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("fee_asset"),
                    },
                    amount: Uint128::new(100_000u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![coin(100_000, "fee_asset")]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Errored Swap, invalid asset
            let msg = ExecuteMsg::SwapForFee { auction_asset: AssetInfo::NativeToken { denom: String::from("fee_asset") }};
            let cosmos_msg = debt_contract.call(msg, vec![coin(99, "error")]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Invalid asset (error) sent to fulfill auction. Must be uosmo")
            );
            //Errored Swap, multiple assets sent
            let msg = ExecuteMsg::SwapForFee { auction_asset: AssetInfo::NativeToken { denom: String::from("fee_asset") }};
            let cosmos_msg = debt_contract.call(msg, vec![coin(93_000, "uosmo"), coin(99, "error")]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Only one coin can be sent")
            );

            //Successful Partial Fill
            let msg = ExecuteMsg::SwapForFee { auction_asset: AssetInfo::NativeToken { denom: String::from("fee_asset") }};
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(93_000, "uosmo")])
                .unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(300u64), //60 * 5 = 6% discount
                chain_id: app.block_info().chain_id,
            });
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert Auction partial fulfillment
            let auction: Vec<FeeAuction> = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::OngoingFeeAuctions {
                        auction_asset: Some(AssetInfo::NativeToken {
                            denom: String::from("fee_asset"),
                        }),
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();

            assert_eq!(auction[0].auction_start_time, 1571797419);
            assert_eq!(
                auction[0].auction_asset.amount,
                Uint128::new(1063u128)
            );
            //Swap cost 93000 OSMO for 98936 fee_asset
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(201_000, "credit_fulldenom"), coin(99, "error"), coin(98936, "fee_asset"),  coin(96_000, "mbrn_denom"), coin(103000, "uosmo")]
            );

            //Successful Overpay Swap
            let msg = ExecuteMsg::SwapForFee { auction_asset: AssetInfo::NativeToken { denom: String::from("fee_asset") }};
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(3_000, "uosmo")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Swap cost 1000 OSMO for 1063 fee_asset
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(201_000, "credit_fulldenom"), coin(99, "error"), coin(99_999, "fee_asset"),  coin(96_000, "mbrn_denom"), coin(102000, "uosmo")]
            );
            //Assert Governance got the proceeds
            assert_eq!(
                app.wrap().query_all_balances("contract0").unwrap(),
                vec![coin(94000, "uosmo")]
            );

            //Assert Auction is empty
            let err = app
                .wrap()
                .query_wasm_smart::<Vec<FeeAuction>>(
                    debt_contract.addr(),
                    &QueryMsg::OngoingFeeAuctions {
                        auction_asset: Some(AssetInfo::NativeToken {
                            denom: String::from("fee_asset"),
                        }),
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap_err();
            assert_eq!(err.to_string(), String::from("Generic error: Querier contract error: Generic error: Auction asset: fee_asset, doesn't have an ongoing auction"));

            //Invalid Swap on 0'd Auction
            let msg = ExecuteMsg::SwapForFee {
                auction_asset: AssetInfo::NativeToken { denom: String::from("fee_asset") }
            };
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(1_000, "uosmo")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            let (mut app, debt_contract, cdp_contract) = proper_instantiate();

            //Successful StartAuction: FeeAuction
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: None,
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("fee_asset"),
                    },
                    amount: Uint128::new(100_000u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![coin(100_000, "fee_asset")]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(360*60 + 1u64), //an 6hrs plus one second
                chain_id: app.block_info().chain_id,
            });

            //Assert Auction returns even at 100% discount
            let err = app
                .wrap()
                .query_wasm_smart::<Vec<FeeAuction>>(
                    debt_contract.addr(),
                    &QueryMsg::OngoingFeeAuctions {
                        auction_asset: None,
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();
            //Successful Overpay Swap that doesn't send extra fee_asset.
            //if it sent extra, the tx will fail.
            let msg = ExecuteMsg::SwapForFee { auction_asset: AssetInfo::NativeToken { denom: String::from("fee_asset") }};
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(3_000, "uosmo")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

        }

        #[test]
        fn swap_for_mbrn() {
            let (mut app, debt_contract, cdp_contract) = proper_instantiate();

            //Successful StartAuction: Position Repayment
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: Some(UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from("owner"),
                }),
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    amount: Uint128::new(100_000u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Errored Swap, invalid asset
            let msg = ExecuteMsg::SwapForMBRN {};
            let cosmos_msg = debt_contract.call(msg, vec![coin(99, "error")]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Invalid asset (error) sent to fulfill auction. Must be credit_fulldenom")
            );
            //Errored Swap, multiple assets sent
            let msg = ExecuteMsg::SwapForMBRN {};
            let cosmos_msg = debt_contract.call(msg, vec![coin(99_000, "credit_fulldenom"), coin(99, "error")]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Only one coin can be sent")
            );

            //Successful Partial Fill
            let msg = ExecuteMsg::SwapForMBRN {};
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(99_000, "credit_fulldenom")])
                .unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(300u64), //60 * 5
                chain_id: app.block_info().chain_id,
            });
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            ///Mint amount asserted in mock contract definition
            //Assert Auction partial fulfillment
            let auction: DebtAuction = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::DebtAuction {},
                )
                .unwrap();

            assert_eq!(auction.auction_start_time, 1571797419u64);
            assert_eq!(
                auction.remaining_recapitalization,
                Uint128::new(1_000u128)
            );

            // Assert CDP received fulfilled bad debt funds (99,000)
            let cdp_balance = app.wrap().query_balance(cdp_contract.as_str(), "credit_fulldenom").unwrap();
            assert_eq!(cdp_balance.amount, Uint128::new(99_000));

            //Successful StartAuction: Send_to
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: None,
                send_to: Some(String::from("send_to_me")),
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    amount: Uint128::new(100_000u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Assert Auction Recap increase
            let auction: DebtAuction = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::DebtAuction {},
                )
                .unwrap();
                assert_eq!(
                    auction.remaining_recapitalization,
                    Uint128::new(101_000u128)
                );

            //Successful Partial Fill
            let msg = ExecuteMsg::SwapForMBRN {};
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(99_000, "credit_fulldenom")])
                .unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert Auction partial fulfillment
            let auction: DebtAuction = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::DebtAuction {},
                )
                .unwrap();

            assert_eq!(auction.auction_start_time, 1571797419u64);
            assert_eq!(
                auction.remaining_recapitalization,
                Uint128::new(2_000u128)
            );

            // Assert CDP received fulfilled bad debt funds (99,000 + 99k from the second one)
            let cdp_balance = app.wrap().query_balance(cdp_contract.as_str(), "credit_fulldenom").unwrap();
            assert_eq!(cdp_balance.amount, Uint128::new(198_000));

            //Successful Overpay Swap
            let msg = ExecuteMsg::SwapForMBRN {};
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(3_000, "credit_fulldenom")])
                .unwrap();
            let _ = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(1_000, "credit_fulldenom"), coin(99, "error"), coin(96_000, "mbrn_denom"),  coin(196_000, "uosmo")]
            );

            // Assert CDP received the final 2,000 fulfillment: 99k + 99k + 2k
            let cdp_balance = app.wrap().query_balance(cdp_contract.as_str(), "credit_fulldenom").unwrap();
            assert_eq!(cdp_balance.amount, Uint128::new(200_000));

            //Assert Auction is empty & therefore removed
            let auction = app
                .wrap()
                .query_wasm_smart::<DebtAuction>(
                    debt_contract.addr(),
                    &QueryMsg::DebtAuction {},
                )
                .unwrap_err();
            
            //Invalid Swap on 0'd Auction
            let msg = ExecuteMsg::SwapForMBRN {};
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(1_000, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

        }

        #[test]
        fn remove_auction() {
            let (mut app, debt_contract, cdp_contract) = proper_instantiate();

            //Successful StartAuction
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: Some(UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from("owner"),
                }),
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    amount: Uint128::new(100u128),
                },
                per_asset_distribution: None,
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Successful RemoveAuction
            let msg = ExecuteMsg::RemoveAuction {};
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Assert DebtAuction removal            
            app
                .wrap()
                .query_wasm_smart::<DebtAuction>(
                    debt_contract.addr(),
                    &QueryMsg::DebtAuction {},
                )
                .unwrap_err();
        }

        #[test]
        fn update_config() {
            let (mut app, debt_contract, cdp_contract) = proper_instantiate();

            //Update Config: Error invalid desired asset
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: None,
                oracle_contract: None,
                osmosis_proxy: None,
                mbrn_denom: None,
                cdt_denom: None,
                governance_contract: None,
                staking_contract: None,
                desired_asset: Some(String::from("no_price")),
                positions_contract: None,
                twap_timeframe: None,
                initial_discount: None,
                discount_increase_timeframe: None,
                discount_increase: None,
                send_to_stakers: None,
                delay_window_minutes: None,
                revenue_distributor_contract: None,
                ltv_disco_contract: None,
                debt_contract: None,
            });
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //Assert desired asset isn't changed
            let config: Config = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::Config {},
                )
                .unwrap();
            assert_eq!(
                config.desired_asset,
                String::from("uosmo"),
            );

            //Update Config
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: Some(String::from("new_owner")), 
                oracle_contract: None,  
                osmosis_proxy: Some(String::from("new_contract")),  
                mbrn_denom: Some(String::from("new_denom")), 
                cdt_denom: Some(String::from("new_cdt")),
                desired_asset: Some(String::from("i_choose_you")),
                positions_contract: Some(String::from("new_contract")),  
                governance_contract: Some(String::from("governance_contract")),
                staking_contract: Some(String::from("staking_contract")),
                twap_timeframe: Some(61u64),
                initial_discount: Some(Decimal::percent(2)), 
                discount_increase_timeframe: Some(61u64), 
                discount_increase: Some(Decimal::percent(4)), 
                send_to_stakers: Some(true),
                delay_window_minutes: None,
                revenue_distributor_contract: None,
                ltv_disco_contract: None,
                debt_contract: None,
            });
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //New owner must call for ownership to change
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: None,
                oracle_contract: None,
                osmosis_proxy: None,
                mbrn_denom: None,
                cdt_denom: None,
                governance_contract: None,
                staking_contract: None,
                desired_asset: None,
                positions_contract: None,
                twap_timeframe: None,
                initial_discount: None,
                discount_increase_timeframe: None,
                discount_increase: Some(Decimal::percent(5)),
                send_to_stakers: None,
                delay_window_minutes: None,
                revenue_distributor_contract: None,
                ltv_disco_contract: None,
                debt_contract: None,
            });
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("new_owner"), cosmos_msg).unwrap();

            //Assert Config Response
            let config: Config = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::Config {},
                )
                .unwrap();
            assert_eq!(
                config,
                Config {
                    owner: Addr::unchecked("new_owner"), 
                    oracle_contract: Addr::unchecked("contract1"),  
                    osmosis_proxy: Addr::unchecked("new_contract"),  
                    mbrn_denom: String::from("new_denom"), 
                    cdt_denom: String::from("new_cdt"),
                    desired_asset: String::from("i_choose_you"),
                    positions_contract: Addr::unchecked("new_contract"),  
                    governance_contract: Addr::unchecked("governance_contract"),
                    staking_contract: Addr::unchecked("staking_contract"),
                    twap_timeframe: 61u64,
                    initial_discount: Decimal::percent(2), 
                    discount_increase_timeframe: 61u64, 
                    discount_increase: Decimal::percent(5), 
                    send_to_stakers: true,
                    delay_window_minutes: 0,
                    revenue_distributor_contract: None,
                    ltv_disco_contract: None,
                    debt_contract: None,
                },
            );
        }

        #[test]
        fn test_start_auction_with_per_asset_distribution() {
            let (mut app, debt_contract, cdp_contract) = proper_instantiate();

            // Start auction with per_asset_distribution
            let per_asset_distribution = vec![
                Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("uatom"),
                    },
                    amount: Uint128::new(60u128),
                },
                Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("uosmo"),
                    },
                    amount: Uint128::new(40u128),
                },
            ];

            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: None,
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("fee_asset"),
                    },
                    amount: Uint128::new(1000u128),
                },
                per_asset_distribution: Some(per_asset_distribution.clone()),
            };
            
            // Send the fee asset along with the message
            let cosmos_msg = debt_contract.call(msg, vec![coin(1000, "fee_asset")]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            // Query the fee auction to verify it was created
            let auctions: Vec<FeeAuction> = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::OngoingFeeAuctions {
                        auction_asset: Some(AssetInfo::NativeToken {
                            denom: String::from("fee_asset"),
                        }),
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();
            
            assert_eq!(auctions.len(), 1);
            assert_eq!(auctions[0].auction_asset.amount, Uint128::new(1000u128));
            
            // Verify per_asset_distribution is stored
            assert!(auctions[0].per_asset_distribution.is_some());
            let stored_distribution = auctions[0].per_asset_distribution.clone().unwrap();
            assert_eq!(stored_distribution.len(), 2);
            assert_eq!(stored_distribution[0].info.to_string(), "uatom");
            assert_eq!(stored_distribution[0].amount, Uint128::new(60u128));
            assert_eq!(stored_distribution[1].info.to_string(), "uosmo");
            assert_eq!(stored_distribution[1].amount, Uint128::new(40u128));
        }

        #[test]
        fn test_fee_auction_without_per_asset_distribution() {
            let (mut app, debt_contract, cdp_contract) = proper_instantiate();

            // Start auction without per_asset_distribution (backwards compatibility)
            let msg = ExecuteMsg::StartAuction {
                repayment_position_info: None,
                send_to: None,
                auction_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("fee_asset"),
                    },
                    amount: Uint128::new(500u128),
                },
                per_asset_distribution: None,
            };
            
            let cosmos_msg = debt_contract.call(msg, vec![coin(500, "fee_asset")]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            // Query the fee auction
            let auctions: Vec<FeeAuction> = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.addr(),
                    &QueryMsg::OngoingFeeAuctions {
                        auction_asset: Some(AssetInfo::NativeToken {
                            denom: String::from("fee_asset"),
                        }),
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();
            
            assert_eq!(auctions.len(), 1);
            // per_asset_distribution should be None
            assert!(auctions[0].per_asset_distribution.is_none());
        }
    }

    mod mbrn_sale {
        use super::*;
        use cosmwasm_std::BlockInfo;
        use membrane::auction::{UpdateConfig, Config};
        use membrane::ltv_disco::ExecuteMsg as LtvDiscoExecuteMsg;
        use membrane::types::{MBRNSale, AssetBadDebtAllocation};

        // Mock Disco contract that handles SendMBRNForSale and ClearBadDebtFreeze
        #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        pub struct Disco_MockInstantiateMsg {}

        pub fn disco_contract() -> Box<dyn Contract<Empty>> {
            let contract = ContractWrapper::new(
                |deps, env, info, msg: LtvDiscoExecuteMsg| -> StdResult<Response> {
                    match msg {
                        LtvDiscoExecuteMsg::SendMBRNForSale { amount, recipient, asset } => {
                            // Send MBRN from our balance to recipient
                            let send_msg = cosmwasm_std::BankMsg::Send {
                                to_address: recipient,
                                amount: vec![coin(amount.u128(), "mbrn_denom")],
                            };
                            Ok(Response::new()
                                .add_message(send_msg)
                                .add_attribute("method", "send_mbrn_for_sale")
                                .add_attribute("amount", amount)
                                .add_attribute("asset", asset))
                        }
                        LtvDiscoExecuteMsg::ClearBadDebtFreeze { asset, cdt_amount } => {
                            Ok(Response::new()
                                .add_attribute("method", "clear_bad_debt_freeze")
                                .add_attribute("asset", asset)
                                .add_attribute("cdt_amount", cdt_amount))
                        }
                        _ => Ok(Response::default()),
                    }
                },
                |_, _, _, _: Disco_MockInstantiateMsg| -> StdResult<Response> {
                    Ok(Response::default())
                },
                |_, _, _: cosmwasm_std::Empty| -> StdResult<Binary> {
                    Ok(to_json_binary(&"")?)
                },
            );
            Box::new(contract)
        }

        fn setup_with_disco() -> (App, DebtContract, Addr, Addr) {
            let mut app = AppBuilder::new().build(|router, _, storage| {
                let bank = BankKeeper::new();
                bank.init_balance(
                    storage,
                    &Addr::unchecked(USER),
                    vec![coin(1_000_000, "credit_fulldenom"), coin(100_000, "mbrn_denom")],
                ).unwrap();
                bank.init_balance(
                    storage,
                    &Addr::unchecked(ADMIN),
                    vec![coin(1_000_000, "credit_fulldenom")],
                ).unwrap();
                router.bank = bank;
            });

            // Instantiate mocks
            let proxy_id = app.store_code(osmosis_proxy_contract());
            let proxy_addr = app.instantiate_contract(proxy_id, Addr::unchecked(ADMIN), &Osmo_MockInstantiateMsg {}, &[], "proxy", None).unwrap();

            let oracle_id = app.store_code(oracle_contract());
            let oracle_addr = app.instantiate_contract(oracle_id, Addr::unchecked(ADMIN), &Oracle_MockInstantiateMsg {}, &[], "oracle", None).unwrap();

            let cdp_id = app.store_code(cdp_contract());
            let cdp_addr = app.instantiate_contract(cdp_id, Addr::unchecked(ADMIN), &CDP_MockInstantiateMsg {}, &[], "cdp", None).unwrap();

            let disco_id = app.store_code(disco_contract());
            let disco_addr = app.instantiate_contract(disco_id, Addr::unchecked(ADMIN), &Disco_MockInstantiateMsg {}, &[], "disco", None).unwrap();

            // Fund disco with MBRN
            app.send_tokens(Addr::unchecked(USER), disco_addr.clone(), &[coin(100_000, "mbrn_denom")]).unwrap();

            // Instantiate auction with ltv_disco configured
            let auction_id = app.store_code(debt_contract());
            let msg = InstantiateMsg {
                owner: None,
                oracle_contract: oracle_addr.to_string(),
                osmosis_proxy: proxy_addr.to_string(),
                positions_contract: cdp_addr.to_string(),
                governance_contract: cdp_addr.to_string(), // reuse cdp as dummy
                staking_contract: cdp_addr.to_string(),    // reuse cdp as dummy
                twap_timeframe: 90u64,
                mbrn_denom: String::from("mbrn_denom"),
                initial_discount: Decimal::percent(1),
                discount_increase_timeframe: 60u64,
                discount_increase: Decimal::percent(1),
                delay_window_minutes: Some(0),
                revenue_distributor_contract: None,
                ltv_disco_contract: Some(disco_addr.to_string()),
                debt_contract: Some(cdp_addr.to_string()), // reuse cdp mock (has FulfillBadDebt handler)
            };
            let auction_addr = app.instantiate_contract(auction_id, Addr::unchecked(ADMIN), &msg, &[], "auction", None).unwrap();
            let auction_contract = DebtContract(auction_addr.clone());

            (app, auction_contract, cdp_addr, disco_addr)
        }

        #[test]
        fn start_mbrn_sale_per_asset() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // Unauthorized: non-disco can't start
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(500),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            // Disco starts sale for uatom
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(10),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            // Check sale state
            let sale: MBRNSale = app.wrap().query_wasm_smart(
                auction_contract.addr(),
                &QueryMsg::MBRNSale {},
            ).unwrap();
            assert_eq!(sale.max_cdt, Uint128::new(1000));
            assert_eq!(sale.asset_allocations.len(), 1);
            assert_eq!(sale.asset_allocations[0].asset, "uatom");
            assert_eq!(sale.asset_allocations[0].mbrn_allocated, Uint128::new(10));

            // Disco adds second asset
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(2000),
                asset: "uosmo".into(),
                mbrn_allocation: Uint128::new(30),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            // Check sale now has 2 assets
            let sale: MBRNSale = app.wrap().query_wasm_smart(
                auction_contract.addr(),
                &QueryMsg::MBRNSale {},
            ).unwrap();
            assert_eq!(sale.max_cdt, Uint128::new(3000));
            assert_eq!(sale.asset_allocations.len(), 2);
            assert_eq!(sale.asset_allocations[1].asset, "uosmo");
            assert_eq!(sale.asset_allocations[1].mbrn_allocated, Uint128::new(30));

            // Adding to same asset stacks
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(500),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(5),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            let sale: MBRNSale = app.wrap().query_wasm_smart(
                auction_contract.addr(),
                &QueryMsg::MBRNSale {},
            ).unwrap();
            assert_eq!(sale.asset_allocations.len(), 2); // Still 2 assets
            assert_eq!(sale.asset_allocations[0].mbrn_allocated, Uint128::new(15)); // 10 + 5
            assert_eq!(sale.asset_allocations[0].cdt_bad_debt, Uint128::new(1500)); // 1000 + 500
        }

        #[test]
        fn even_split_and_exhaustion() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // Start sale with 3 assets: allocations = 100, 500, 1000 MBRN
            // CDT bad debt: 10_000 each (large enough to not be the limiting factor)
            for (asset, mbrn) in [("uatom", 100u128), ("uosmo", 500), ("ustars", 1000)] {
                let msg = ExecuteMsg::StartMBRNSale {
                    max_cdt: Uint128::new(10_000),
                    asset: asset.into(),
                    mbrn_allocation: Uint128::new(mbrn),
                };
                let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
                app.execute(disco_addr.clone(), cosmos_msg).unwrap();
            }

            // Advance time past delay window for discount
            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // With 1:1 pricing and ~3% discount, 1 CDT ≈ 1.03 MBRN.
            // Send 300 CDT → ~309 MBRN total → even split: ~103 MBRN per asset.
            // uatom has 100 MBRN → exhausted. uosmo (500) and ustars (1000) continue.
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(300, "credit_fulldenom")]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Check sale state — uatom should be removed (MBRN exhausted)
            let sale: Option<MBRNSale> = app.wrap().query_wasm_smart(
                auction_contract.addr(),
                &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.is_some(), "Sale should still exist (uosmo and ustars remain)");
            let sale = sale.unwrap();

            // uatom should be removed from allocations
            assert!(!sale.asset_allocations.iter().any(|a| a.asset == "uatom"),
                "uatom should be exhausted and removed");
            // uosmo and ustars should remain with some usage
            let uosmo = sale.asset_allocations.iter().find(|a| a.asset == "uosmo").unwrap();
            assert!(uosmo.mbrn_used > Uint128::zero(), "uosmo should have some MBRN used");
            assert!(uosmo.mbrn_used < uosmo.mbrn_allocated, "uosmo should not be exhausted yet");

            let ustars = sale.asset_allocations.iter().find(|a| a.asset == "ustars").unwrap();
            assert!(ustars.mbrn_used > Uint128::zero(), "ustars should have some MBRN used");

            // Verify ClearBadDebtFreeze was called for uatom (check events)
            let has_clear = res.events.iter().any(|e|
                e.attributes.iter().any(|a| a.key == "method" && a.value == "clear_bad_debt_freeze")
                && e.attributes.iter().any(|a| a.key == "asset" && a.value == "uatom")
            );
            assert!(has_clear, "ClearBadDebtFreeze should have been called for uatom");

            // Verify SendMBRNForSale recipient is the buyer, not the auction contract
            let send_events: Vec<_> = res.events.iter()
                .filter(|e| e.attributes.iter().any(|a| a.key == "method" && a.value == "send_mbrn_for_sale"))
                .collect();
            assert!(!send_events.is_empty(), "SendMBRNForSale should have been called");
        }

        #[test]
        fn recursive_redistribution() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // 2 assets: one very small (10 MBRN), one large (50_000 MBRN)
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(10_000),
                asset: "tiny".into(),
                mbrn_allocation: Uint128::new(10),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(50_000),
                asset: "large".into(),
                mbrn_allocation: Uint128::new(50_000),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // Buy with 100 CDT. Even split: 50 CDT each → ~51 MBRN each.
            // "tiny" has 10 MBRN → exhausted, uses ~10 MBRN / ~10 CDT.
            // Leftover ~40 CDT redistributed to "large" via recursive call.
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Check that "tiny" was exhausted
            let sale: Option<MBRNSale> = app.wrap().query_wasm_smart(
                auction_contract.addr(),
                &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.is_some(), "Sale should still exist (large remains)");
            let sale = sale.unwrap();
            assert!(!sale.asset_allocations.iter().any(|a| a.asset == "tiny"), "tiny should be exhausted");
            assert!(sale.asset_allocations.iter().any(|a| a.asset == "large"));

            // "large" should have received more CDT than just 50 (its initial 50% share)
            // due to recursive redistribution of tiny's leftover
            let large_alloc = sale.asset_allocations.iter().find(|a| a.asset == "large").unwrap();
            assert!(large_alloc.cdt_fulfilled > Uint128::new(50),
                "large should have received more than half due to redistribution, got {}", large_alloc.cdt_fulfilled);

            // Verify SendMBRNForSale was called multiple times (initial + recursive)
            let send_events: Vec<_> = res.events.iter()
                .filter(|e| e.attributes.iter().any(|a| a.key == "method" && a.value == "send_mbrn_for_sale"))
                .collect();
            assert!(send_events.len() >= 2,
                "SendMBRNForSale should be called at least twice (initial + recursive), got {}", send_events.len());
        }

        #[test]
        fn buy_completes_sale_when_all_exhausted() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // Single asset with small allocation (50 MBRN, 100 CDT bad debt)
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(100),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(50),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // Buy with 200 CDT — more than the 100 CDT bad debt, so CDT cap triggers
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(200, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Sale should be completely removed (all allocations exhausted)
            let sale: Option<MBRNSale> = app.wrap().query_wasm_smart(
                auction_contract.addr(),
                &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.is_none(), "Sale should be removed when all allocations are exhausted");
        }

        #[test]
        fn start_mbrn_sale_unauthorized() {
            let (mut app, auction_contract, _, _disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(100),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            // Non-disco sender
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn start_mbrn_sale_zero_cdt() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::zero(),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(100),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr, cosmos_msg).unwrap_err();
        }

        #[test]
        fn start_mbrn_sale_no_funds_allowed() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // Sending funds should fail (pull model)
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(100),
            };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(disco_addr, cosmos_msg).unwrap_err();
        }

        #[test]
        fn buy_no_active_sale() {
            let (mut app, auction_contract, _, _disco_addr) = setup_with_disco();

            // No sale started — buy should fail
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn buy_partial_single_asset() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(10_000),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(10_000),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // Small buy — should not exhaust
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(10, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Sale should still exist with partial fulfillment
            let sale: Option<MBRNSale> = app.wrap().query_wasm_smart(
                auction_contract.addr(),
                &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.is_some());
            let sale = sale.unwrap();
            assert_eq!(sale.asset_allocations.len(), 1);
            assert!(sale.asset_allocations[0].cdt_fulfilled > Uint128::zero());
            assert!(sale.asset_allocations[0].mbrn_used > Uint128::zero());
            assert!(sale.cdt_fulfilled > Uint128::zero());
        }

        #[test]
        fn buy_cdt_cap_limits_asset() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // Asset with lots of MBRN but small CDT bad debt — CDT cap triggers
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(10), // tiny CDT
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(10_000), // huge MBRN
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // Send enough CDT to cover the 10 CDT bad debt
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Sale should be complete (CDT fulfilled even though MBRN has plenty left)
            let sale: Option<MBRNSale> = app.wrap().query_wasm_smart(
                auction_contract.addr(),
                &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.is_none(), "Sale should complete when CDT bad debt is fulfilled");
        }

        #[test]
        fn buy_mbrn_cap_limits_asset() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // Asset with tiny MBRN but large CDT bad debt — MBRN cap triggers
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(100_000),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(5), // tiny MBRN
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(1000, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Sale should be complete (MBRN exhausted)
            let sale: Option<MBRNSale> = app.wrap().query_wasm_smart(
                auction_contract.addr(),
                &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.is_none(), "Sale should complete when MBRN is exhausted");
        }

        #[test]
        fn buy_depth_cap_refunds_at_max() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // 4 assets with 1 MBRN each — needs 4 recursive rounds to exhaust all,
            // but max depth is 3 so the last refund goes to buyer
            for i in 0..4 {
                let msg = ExecuteMsg::StartMBRNSale {
                    max_cdt: Uint128::new(10_000),
                    asset: format!("asset{}", i),
                    mbrn_allocation: Uint128::new(1),
                };
                let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
                app.execute(disco_addr.clone(), cosmos_msg).unwrap();
            }

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // Large buy — each round exhausts one asset, leftover redistributes
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(1000, "credit_fulldenom")]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Check depth attributes to confirm recursion happened
            let depth_attrs: Vec<_> = res.events.iter()
                .flat_map(|e| e.attributes.iter())
                .filter(|a| a.key == "depth")
                .map(|a| a.value.clone())
                .collect();
            // Should have at least depth 0 and some recursive depths
            assert!(depth_attrs.contains(&"0".to_string()), "Should have initial depth 0");
        }

        #[test]
        fn buyer_field_unauthorized_from_non_contract() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(100),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // External user trying to spoof buyer field should fail
            let msg = ExecuteMsg::BuySuppliedMBRN {
                depth: Some(1),
                buyer: Some("victim".to_string()),
            };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn clear_bad_debt_freeze_called_per_asset() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // 2 assets, one small one large
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(10_000), asset: "small".into(),
                mbrn_allocation: Uint128::new(5),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(10_000), asset: "big".into(),
                mbrn_allocation: Uint128::new(50_000),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // Buy enough to exhaust "small" but not "big"
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(20, "credit_fulldenom")]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // ClearBadDebtFreeze should be called for "small" only
            let clear_for_small = res.events.iter().any(|e|
                e.attributes.iter().any(|a| a.key == "method" && a.value == "clear_bad_debt_freeze")
                && e.attributes.iter().any(|a| a.key == "asset" && a.value == "small")
            );
            let clear_for_big = res.events.iter().any(|e|
                e.attributes.iter().any(|a| a.key == "method" && a.value == "clear_bad_debt_freeze")
                && e.attributes.iter().any(|a| a.key == "asset" && a.value == "big")
            );
            assert!(clear_for_small, "ClearBadDebtFreeze should fire for exhausted small asset");
            assert!(!clear_for_big, "ClearBadDebtFreeze should NOT fire for active big asset");
        }

        #[test]
        fn multiple_buys_progressive() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(10_000),
                asset: "uatom".into(),
                mbrn_allocation: Uint128::new(1_000),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // Buy 1: partial
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(10, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let sale: MBRNSale = app.wrap().query_wasm_smart(
                auction_contract.addr(), &QueryMsg::MBRNSale {},
            ).unwrap();
            let first_mbrn = sale.asset_allocations[0].mbrn_used;
            let first_cdt = sale.cdt_fulfilled;
            assert!(first_mbrn > Uint128::zero());
            assert!(first_cdt > Uint128::zero());

            // Buy 2: more partial
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(10, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let sale: MBRNSale = app.wrap().query_wasm_smart(
                auction_contract.addr(), &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.asset_allocations[0].mbrn_used > first_mbrn, "MBRN used should increase");
            assert!(sale.cdt_fulfilled > first_cdt, "CDT fulfilled should increase");
        }

        // =================== FulfillBadDebt (Altruistic) ===================

        #[test]
        fn fulfill_bad_debt_no_active_sale() {
            let (mut app, auction_contract, _, _) = setup_with_disco();

            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn fulfill_bad_debt_wrong_asset() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000), asset: "uatom".into(),
                mbrn_allocation: Uint128::new(100),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            // Try to fulfill for a different asset
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uosmo".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn fulfill_bad_debt_no_cdt_sent() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000), asset: "uatom".into(),
                mbrn_allocation: Uint128::new(100),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            // No funds
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn fulfill_bad_debt_partial() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000), asset: "uatom".into(),
                mbrn_allocation: Uint128::new(500),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            // Partial fulfill: 300 of 1000
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(300, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Sale should still exist with reduced bad debt
            let sale: MBRNSale = app.wrap().query_wasm_smart(
                auction_contract.addr(), &QueryMsg::MBRNSale {},
            ).unwrap();
            assert_eq!(sale.asset_allocations[0].cdt_fulfilled, Uint128::new(300));
            assert_eq!(sale.cdt_fulfilled, Uint128::new(300));
            // MBRN should NOT be touched (no slashing for altruistic fill)
            assert_eq!(sale.asset_allocations[0].mbrn_used, Uint128::zero());
        }

        #[test]
        fn fulfill_bad_debt_full_clears_freeze() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(500), asset: "uatom".into(),
                mbrn_allocation: Uint128::new(100),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            // Fully cover the bad debt
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(500, "credit_fulldenom")]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Sale should be removed
            let sale: Option<MBRNSale> = app.wrap().query_wasm_smart(
                auction_contract.addr(), &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.is_none(), "Sale should be removed after full fulfillment");

            // ClearBadDebtFreeze should have been called
            let has_clear = res.events.iter().any(|e|
                e.attributes.iter().any(|a| a.key == "method" && a.value == "clear_bad_debt_freeze")
                && e.attributes.iter().any(|a| a.key == "asset" && a.value == "uatom")
            );
            assert!(has_clear, "ClearBadDebtFreeze should fire for fully fulfilled asset");
        }

        #[test]
        fn fulfill_bad_debt_excess_refunded() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(100), asset: "uatom".into(),
                mbrn_allocation: Uint128::new(50),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            // Send 300 CDT for 100 CDT bad debt — 200 should be refunded
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(300, "credit_fulldenom")]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Verify only 100 CDT was applied
            let applied = res.events.iter()
                .flat_map(|e| e.attributes.iter())
                .find(|a| a.key == "cdt_applied")
                .map(|a| a.value.clone());
            assert_eq!(applied, Some("100".to_string()), "Only 100 CDT should be applied to bad debt");

            // Sale should be complete
            let sale: Option<MBRNSale> = app.wrap().query_wasm_smart(
                auction_contract.addr(), &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.is_none());
        }

        #[test]
        fn fulfill_bad_debt_one_of_multiple_assets() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // Two assets
            for (asset, cdt, mbrn) in [("uatom", 500u128, 100u128), ("uosmo", 1000, 200)] {
                let msg = ExecuteMsg::StartMBRNSale {
                    max_cdt: Uint128::new(cdt), asset: asset.into(),
                    mbrn_allocation: Uint128::new(mbrn),
                };
                let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
                app.execute(disco_addr.clone(), cosmos_msg).unwrap();
            }

            // Fulfill only uatom's bad debt
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(500, "credit_fulldenom")]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Sale should still exist (uosmo remains)
            let sale: MBRNSale = app.wrap().query_wasm_smart(
                auction_contract.addr(), &QueryMsg::MBRNSale {},
            ).unwrap();
            assert_eq!(sale.asset_allocations.len(), 1);
            assert_eq!(sale.asset_allocations[0].asset, "uosmo");

            // ClearBadDebtFreeze called for uatom only
            let clear_uatom = res.events.iter().any(|e|
                e.attributes.iter().any(|a| a.key == "method" && a.value == "clear_bad_debt_freeze")
                && e.attributes.iter().any(|a| a.key == "asset" && a.value == "uatom")
            );
            assert!(clear_uatom, "ClearBadDebtFreeze should fire for uatom");
        }

        #[test]
        fn fulfill_bad_debt_then_buy_mbrn() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            // Two assets
            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(500), asset: "uatom".into(),
                mbrn_allocation: Uint128::new(100),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000), asset: "uosmo".into(),
                mbrn_allocation: Uint128::new(500),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            // Altruistically fill uatom
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(500, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Now buy MBRN — should only affect uosmo
            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // uosmo should have progress, uatom should be gone
            let sale: MBRNSale = app.wrap().query_wasm_smart(
                auction_contract.addr(), &QueryMsg::MBRNSale {},
            ).unwrap();
            assert_eq!(sale.asset_allocations.len(), 1);
            assert_eq!(sale.asset_allocations[0].asset, "uosmo");
            assert!(sale.asset_allocations[0].mbrn_used > Uint128::zero());
        }

        #[test]
        fn fulfill_bad_debt_already_fulfilled() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(100), asset: "uatom".into(),
                mbrn_allocation: Uint128::new(50),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            // Fulfill fully
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Try again — should fail (sale removed)
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(50, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn fulfill_and_buy_interleaved() {
            let (mut app, auction_contract, _, disco_addr) = setup_with_disco();

            let msg = ExecuteMsg::StartMBRNSale {
                max_cdt: Uint128::new(1000), asset: "uatom".into(),
                mbrn_allocation: Uint128::new(500),
            };
            let cosmos_msg = auction_contract.call(msg, vec![]).unwrap();
            app.execute(disco_addr.clone(), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: 1000,
                time: app.block_info().time.plus_seconds(120),
                chain_id: "test".to_string(),
            });

            // Buy some MBRN first (partial)
            let msg = ExecuteMsg::BuySuppliedMBRN { depth: None, buyer: None };
            let cosmos_msg = auction_contract.call(msg, vec![coin(100, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let sale: MBRNSale = app.wrap().query_wasm_smart(
                auction_contract.addr(), &QueryMsg::MBRNSale {},
            ).unwrap();
            let cdt_after_buy = sale.asset_allocations[0].cdt_fulfilled;
            assert!(cdt_after_buy > Uint128::zero());

            // Now altruistically fill the rest
            let remaining = Uint128::new(1000) - cdt_after_buy;
            let msg = ExecuteMsg::FulfillBadDebt { asset: "uatom".into() };
            let cosmos_msg = auction_contract.call(msg, vec![coin(remaining.u128(), "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Sale should be complete
            let sale: Option<MBRNSale> = app.wrap().query_wasm_smart(
                auction_contract.addr(), &QueryMsg::MBRNSale {},
            ).unwrap();
            assert!(sale.is_none(), "Sale should complete after buy + fulfill cover total bad debt");
        }
    }
}
