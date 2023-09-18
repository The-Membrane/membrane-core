#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use crate::helpers::DebtContract;

    use membrane::auction::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::oracle::PriceResponse;
    use membrane::types::{Asset, AssetInfo, Basket, FeeAuction, DebtAuction};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Decimal, Empty, Response, StdResult, Uint128,
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
                            panic!("{}", amount)
                        }
                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::BurnTokens {
                        denom,
                        amount,
                        burn_from_address,
                    } => {
                        if amount != Uint128::new(93000u128) && amount != Uint128::new(940u128)
                        {
                            panic!("{}", amount)
                        }
                        Ok(Response::new())
                    }
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse { })?)
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
                        basket_id,
                    } => {
                        if asset_info.to_string() == String::from("no_price"){
                            return Err(cosmwasm_std::StdError::GenericErr { msg: String::from("Asset has no oracle price") })
                        } else {
                            Ok(to_binary(&PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            })?)
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
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct CDP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {
        GetBasket { },
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
                }
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    CDP_MockQueryMsg::GetBasket { } => Ok(to_binary(&Basket {
                        basket_id: Uint128::one(),
                        current_position_id: Uint128::one(),
                        collateral_types: vec![],
                        collateral_supply_caps: vec![],
                        lastest_collateral_rates: vec![],
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: String::from("credit_fulldenom"),
                            },
                            amount: Uint128::zero(),
                        },
                        credit_price: PriceResponse { 
                            prices: vec![], 
                            price: Decimal::one(), 
                            decimals: 6
                        },
                        liq_queue: None,
                        base_interest_rate: Decimal::zero(),
                        pending_revenue: Uint128::zero(),
                        negative_rates: true,
                        cpc_margin_of_error: Decimal::zero(),
                        multi_asset_supply_caps: vec![],
                        frozen: false,
                        rev_to_stakers: true,
                        credit_last_accrued: 0,
                        rates_last_accrued: 0,
                        oracle_set: false,
                    })?),
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
                vec![coin(99, "error"), coin(201_000, "credit_fulldenom"), coin(96_000, "mbrn_denom"), coin(96_000, "uosmo")],
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
            types::{RepayPosition, UserInfo, AuctionRecipient},
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
        fn swap_with_mbrn(){
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

            ///Burn amount asserted in mock contract definition
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
                Uint128::new(1_000u128)
            );

            //Successful Overpay Swap
            let msg = ExecuteMsg::SwapForFee { auction_asset: AssetInfo::NativeToken { denom: String::from("fee_asset") }};
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(3_000, "uosmo")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Swap cost 940 MBRN for 1000 fee_asset
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(201_000, "credit_fulldenom"), coin(99, "error"), coin(94_000, "fee_asset"),  coin(96_000, "mbrn_denom"), coin(2060, "uosmo")]
            );
            //Assert Governance got the proceeds
            assert_eq!(
                app.wrap().query_all_balances("contract0").unwrap(),
                vec![coin(93940, "uosmo")]
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
            assert_eq!(
                auction.repayment_positions,
                vec![RepayPosition {
                    repayment: Uint128::new(1_000u128),
                    position_info: UserInfo {
                        position_id: Uint128::new(1u128),
                        position_owner: String::from("owner"),
                    }
                }]
            );

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
            assert_eq!(
                auction.send_to,
                vec![AuctionRecipient {
                    amount: Uint128::new(2_000),
                    recipient: Addr::unchecked("send_to_me"),
                }]
            );

            //Successful Overpay Swap
            let msg = ExecuteMsg::SwapForMBRN {};
            let cosmos_msg = debt_contract
                .call(msg, vec![coin(3_000, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(1_000, "credit_fulldenom"), coin(99, "error"), coin(96_000, "mbrn_denom"),  coin(96_000, "uosmo")]
            );

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
            });
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: Generic error: Querier contract error: Generic error: Asset has no oracle price"));

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
                },
            );
        }
    }
}
