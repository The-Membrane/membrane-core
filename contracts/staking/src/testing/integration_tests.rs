#[cfg(test)]
mod tests {
    use membrane::staking::{ExecuteMsg, InstantiateMsg, QueryMsg, FeeEventsResponse};
    use membrane::types::{AssetInfo, StakeDistribution, Asset, UserInfo, Basket, FeeEvent, LiqAsset};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Decimal, Empty, Response, StdResult, Uint128, CosmosMsg, Coin, WasmMsg,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    //const USER: &str = "user";
    const ADMIN: &str = "admin";

    /// StakingContract is a wrapper around Addr that provides a lot of helpers
    /// for working with this.
    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
    pub struct StakingContract(pub Addr);
    
    impl StakingContract {
        #[allow(dead_code)]
        pub fn addr(&self) -> Addr {
            self.0.clone()
        }

        #[allow(dead_code)]
        pub fn call<T: Into<ExecuteMsg>>(&self, msg: T, funds: Vec<Coin>) -> StdResult<CosmosMsg> {
            let msg = to_binary(&msg.into())?;
            Ok(WasmMsg::Execute {
                contract_addr: self.addr().into(),
                msg,
                funds,
            }
            .into())
        }
    }

    //Staking Contract
    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        );
        Box::new(contract)
    }

    //Mock Osmo Proxy Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct MockResponse {}

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }

    //Mock CDP Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockExecuteMsg {}

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
                Ok(Response::default())
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    CDP_MockQueryMsg::GetBasket { } => Ok(to_binary(&Basket {
                        basket_id: Uint128::one(),
                        current_position_id: Uint128::one(),
                        collateral_types: vec![],
                        collateral_supply_caps: vec![],
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: String::from("credit_fulldenom"),
                            },
                            amount: Uint128::zero(),
                        },
                        credit_price: Decimal::one(),
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

    //Mock Auction Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Auction_MockExecuteMsg {
        StartAuction {
            repayment_position_info: Option<UserInfo>,
            send_to: Option<String>,
            auction_asset: Asset,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Auction_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Auction_MockQueryMsg {}


    pub fn auction_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Auction_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Auction_MockExecuteMsg::StartAuction {
                        repayment_position_info,
                        send_to,
                        auction_asset,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Auction_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Auction_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }


    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &Addr::unchecked("coin_God"),
                vec![coin(100_000_000, "debit"), coin(100_000_000, "2nddebit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract1"), //positions contract
                vec![coin(1000, "credit_fulldenom"), coin(1000, "fee_asset")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, StakingContract, Addr) {
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

        let auction_id = app.store_code(auction_contract());
        let auction_contract_addr = app
            .instantiate_contract(
                auction_id,
                Addr::unchecked(ADMIN),
                &Auction_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Staking contract
        let staking_id = app.store_code(staking_contract());

        let msg = InstantiateMsg {
            owner: Some("owner0000".to_string()),
            positions_contract: Some(cdp_contract_addr.to_string()),
            auction_contract: Some(auction_contract_addr.to_string()),
            vesting_contract: None,
            governance_contract: Some("gov_contract".to_string()),
            osmosis_proxy: Some(osmosis_proxy_contract_addr.to_string()),
            incentive_schedule: Some(StakeDistribution { rate: Decimal::percent(10), duration: 90 }),
            fee_wait_period: None,
            mbrn_denom: String::from("mbrn_denom"),
            dex_router: Some(String::from("router_addr")),
            max_spread: Some(Decimal::percent(10)),
            unstaking_period: None,
        };

        let staking_contract_addr = app
            .instantiate_contract(staking_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let staking_contract = StakingContract(staking_contract_addr);

        (app, staking_contract, auction_contract_addr)
    }

    #[cfg(test)]
    mod staking {

        use super::*;
        
        #[test]
        fn deposit_fee() {
            let (mut app, staking_contract, auction_contract) = proper_instantiate();
            
            //DepositFee: Unauthorized
            let msg = ExecuteMsg::DepositFee {  };
            let cosmos_msg = staking_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

            //DepositFees
            let msg = ExecuteMsg::DepositFee {  };
            let cosmos_msg = staking_contract.call(msg, vec![coin(1000, "credit_fulldenom"), coin(1000, "fee_asset")]).unwrap();
            app.execute(Addr::unchecked("contract1"), cosmos_msg).unwrap();

            //Check that the fee is deposited in the auction contract
            assert_eq!(
                app.wrap().query_all_balances(auction_contract).unwrap(),
                vec![coin(1000, "fee_asset")]
            );

            //Assert Auction Response
            let resp: FeeEventsResponse = app
                .wrap()
                .query_wasm_smart(
                    staking_contract.addr(),
                    &QueryMsg::FeeEvents {
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();
            assert_eq!(resp.fee_events, vec![
            FeeEvent {
                time_of_event: 1571797419,
                fee: LiqAsset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom")
                    },
                    amount: Decimal::percent(1000_00),
                },
            },
        ]);
            
        }
    }
}
