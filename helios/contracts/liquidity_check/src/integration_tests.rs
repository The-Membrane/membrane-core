#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use crate::helpers::LiquidityContract;

    use membrane::liquidity_check::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::types::AssetInfo;

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Empty, Response, StdResult, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use osmo_bindings::{PoolStateResponse};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //Liquity Contract
    pub fn liquidity_contract() -> Box<dyn Contract<Empty>> {
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
    pub enum Osmo_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg {
        PoolState {
            id: u64,
        }
    }

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::PoolState { id } => {
                        if id == 99u64 {
                            Ok(to_binary(&PoolStateResponse {
                                assets: vec![coin(100_000_000, "base"), coin(100_000_000, "quote")],
                                shares: coin(100_000_000, "lp_denom"),
                            })?)
                        } else {
                            Ok(to_binary(&PoolStateResponse {
                                assets: vec![coin(49_999, "credit_fulldenom")],
                                shares: coin(0, "shares"),
                            })?)
                        }
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
                vec![coin(99, "error"), coin(101, "credit_fulldenom")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, LiquidityContract) {
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

        //Instantiate Oracle contract
        let liquidity_id = app.store_code(liquidity_contract());

        let msg = InstantiateMsg {
            owner: None,
            osmosis_proxy: osmosis_proxy_contract_addr.to_string(),
            positions_contract: "cdp".to_string(),
        };

        let liquidity_contract_addr = app
            .instantiate_contract(
                liquidity_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "test",
                None,
            )
            .unwrap();

        let liquidity_contract = LiquidityContract(liquidity_contract_addr);

        (app, liquidity_contract)
    }

    mod liquidity {

        use crate::state::Config;

        use super::*;
        use membrane::types::LiquidityInfo;

        #[test]
        fn add_edit_remove() {
            let (mut app, liquidity_contract) = proper_instantiate();

            //Unauthorized AddAsset: Error
            let msg = ExecuteMsg::AddAsset {
                asset: LiquidityInfo {
                    asset: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    pool_ids: vec![1u64],
                },
            };
            let cosmos_msg = liquidity_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Successful AddAsset
            let msg = ExecuteMsg::AddAsset {
                asset: LiquidityInfo {
                    asset: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    pool_ids: vec![1u64],
                },
            };
            let cosmos_msg = liquidity_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Duplicate AddAsset: Error
            let msg = ExecuteMsg::AddAsset {
                asset: LiquidityInfo {
                    asset: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    pool_ids: vec![1u64],
                },
            };
            let cosmos_msg = liquidity_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

            //Query Asset(s)
            let assets: Vec<LiquidityInfo> = app
                .wrap()
                .query_wasm_smart(
                    liquidity_contract.addr(),
                    &QueryMsg::Assets {
                        asset_info: Some(AssetInfo::NativeToken {
                            denom: String::from("credit_fulldenom"),
                        }),
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();
            assert_eq!(assets[0].pool_ids, vec![1u64]);

            //Successful EditAsset
            let msg = ExecuteMsg::EditAsset {
                asset: LiquidityInfo {
                    asset: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    pool_ids: vec![2u64, 3u64, 4u64],
                },
            };
            let cosmos_msg = liquidity_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("cdp"), cosmos_msg).unwrap();

            //Successful AddAsset: 2nd Asset
            let msg = ExecuteMsg::AddAsset {
                asset: LiquidityInfo {
                    asset: AssetInfo::NativeToken {
                        denom: String::from("credit_two"),
                    },
                    pool_ids: vec![99u64],
                },
            };
            let cosmos_msg = liquidity_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Query Asset(s)
            let assets: Vec<LiquidityInfo> = app
                .wrap()
                .query_wasm_smart(
                    liquidity_contract.addr(),
                    &QueryMsg::Assets {
                        asset_info: None,
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();
            assert_eq!(assets.len(), 2u64 as usize);
            assert_eq!(assets[0].pool_ids, vec![1u64, 2u64, 3u64, 4u64]);

            //Successful RemoveAsset
            let msg = ExecuteMsg::RemoveAsset {
                asset: AssetInfo::NativeToken {
                    denom: String::from("credit_fulldenom"),
                },
            };
            let cosmos_msg = liquidity_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Query Asset was removed
            let assets: Vec<LiquidityInfo> = app
                .wrap()
                .query_wasm_smart(
                    liquidity_contract.addr(),
                    &QueryMsg::Assets {
                        asset_info: None,
                        limit: None,
                        start_after: None,
                    },
                )
                .unwrap();
            assert_eq!(assets.len(), 1u64 as usize);
        }

        #[test]
        fn get_liquidity() {
            let (mut app, liquidity_contract) = proper_instantiate();

            //Successful AddAsset
            let msg = ExecuteMsg::AddAsset {
                asset: LiquidityInfo {
                    asset: AssetInfo::NativeToken {
                        denom: String::from("credit_fulldenom"),
                    },
                    pool_ids: vec![1u64, 2u64],
                },
            };
            let cosmos_msg = liquidity_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Error: Invalid Asset
            app.wrap()
                .query_wasm_smart::<Uint128>(
                    liquidity_contract.addr(),
                    &QueryMsg::Liquidity {
                        asset: AssetInfo::NativeToken {
                            denom: String::from("invalid"),
                        },
                    },
                )
                .unwrap_err();

            //Query Liquidity
            let liquidity: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    liquidity_contract.addr(),
                    &QueryMsg::Liquidity {
                        asset: AssetInfo::NativeToken {
                            denom: String::from("credit_fulldenom"),
                        },
                    },
                )
                .unwrap();
            assert_eq!(liquidity, Uint128::new(99998u128));
        }

        #[test]
        fn update_config() {
            let (mut app, liquidity_contract) = proper_instantiate();

            //Successful AddAsset
            let msg = ExecuteMsg::UpdateConfig { 
                owner: Some(String::from("new_owner")), 
                osmosis_proxy: Some(String::from("new_op_contract")),  
                positions_contract: Some(String::from("new_pos_contract")), 
            };
            let cosmos_msg = liquidity_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Query Liquidity
            let config: Config = app
                .wrap()
                .query_wasm_smart(
                    liquidity_contract.addr(),
                    &QueryMsg::Config {},
                )
                .unwrap();
            assert_eq!(
                config, 
                Config {
                    owner: Addr::unchecked("new_owner"), 
                    osmosis_proxy:  Addr::unchecked("new_op_contract"),  
                    positions_contract:  Addr::unchecked("new_pos_contract"), 
            });
        }
    }
}
