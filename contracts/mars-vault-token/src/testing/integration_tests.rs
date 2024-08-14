mod tests {

    use crate::helpers::MarsVaultContract;

    use membrane::mars_vault_token::{ExecuteMsg, InstantiateMsg, QueryMsg, Config};
    use membrane::mars_redbank::{Market, UserCollateralResponse, InterestRateModel};
    use membrane::types::{AssetInfo, Asset, AssetPool};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Empty, Response, StdResult, Uint128, Decimal,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //Mars Vault Contract
    pub fn mars_vault_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        );
        Box::new(contract)
    }

     //Mock Red Bank Contract
    #[cw_serde]
     pub enum Mars_MockExecuteMsg {
        /// Deposit native coins. Deposited coins must be sent in the transaction
        /// this call is made
        Deposit {
            /// Credit account id (Rover)
            account_id: Option<String>,
    
            /// Address that will receive the coins
            on_behalf_of: Option<String>,
        },
    
        /// Withdraw native coins
        Withdraw {
            /// Asset to withdraw
            denom: String,
            /// Amount to be withdrawn. If None is specified, the full amount will be withdrawn.
            amount: Option<Uint128>,
            /// The address where the withdrawn amount is sent
            recipient: Option<String>,
            /// Credit account id (Rover)
            account_id: Option<String>,
            // Withdraw action related to liquidation process initiated in credit manager.
            // This flag is used to identify different way for pricing assets during liquidation.
            liquidation_related: Option<bool>,
        },
    }
 
     
    #[cw_serde]
     pub struct Mars_MockInstantiateMsg {}
 
     
    #[cw_serde]
     pub enum Mars_MockQueryMsg {        
        Market {
            denom: String,
        },
        UnderlyingLiquidityAmount {
            denom: String,
            amount_scaled: Uint128,
        },
        UserCollateral {
            user: String,
            account_id: Option<String>,
            denom: String,
        },
    }
 
    pub fn redbank_contract_ten() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Mars_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Mars_MockExecuteMsg::Deposit { account_id, on_behalf_of } => {
                    Ok(Response::default())
                    },
                    Mars_MockExecuteMsg::Withdraw { denom, amount, recipient, account_id, liquidation_related } => {
                        Ok(Response::default())
                        }
                }
            },
            |_, _, _, _: Mars_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Mars_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Mars_MockQueryMsg::Market { denom } => {
                        Ok(to_binary(&Market {
                            denom: String::from("uusdc"),
                            reserve_factor: Decimal::zero(),                            
                            interest_rate_model: InterestRateModel {
                                optimal_utilization_rate: Decimal::zero(),
                                base: Decimal::zero(),
                                slope_1: Decimal::zero(),
                                slope_2: Decimal::zero(),
                            },                            
                            borrow_index: Decimal::zero(),
                            liquidity_index: Decimal::zero(),
                            borrow_rate: Decimal::zero(),
                            liquidity_rate: Decimal::percent(10),
                            indexes_last_updated: 1u64,                            
                            collateral_total_scaled: Uint128::zero(),
                            debt_total_scaled: Uint128::zero()
                        }).unwrap())
                    },
                    Mars_MockQueryMsg::UnderlyingLiquidityAmount { denom, amount_scaled } => {
                        Ok(to_binary(&Uint128::new(10)).unwrap())
                    },
                    Mars_MockQueryMsg::UserCollateral { user, account_id, denom } => {
                        Ok(to_binary(&UserCollateralResponse {
                            denom: denom,
                            amount: Uint128::new(10),
                            amount_scaled: Uint128::new(10),
                            enabled: false
                        }).unwrap())
                    }
                }
            },
        );
        Box::new(contract)
    }
    pub fn redbank_contract_zero() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Mars_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Mars_MockExecuteMsg::Deposit { account_id, on_behalf_of } => {
                    Ok(Response::default())
                    },
                    Mars_MockExecuteMsg::Withdraw { denom, amount, recipient, account_id, liquidation_related } => {
                        Ok(Response::default())
                        }
                }
            },
            |_, _, _, _: Mars_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Mars_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Mars_MockQueryMsg::Market { denom } => {
                        Ok(to_binary(&Market {
                            denom: String::from("uusdc"),
                            reserve_factor: Decimal::zero(),                            
                            interest_rate_model: InterestRateModel {
                                optimal_utilization_rate: Decimal::zero(),
                                base: Decimal::zero(),
                                slope_1: Decimal::zero(),
                                slope_2: Decimal::zero(),
                            },                            
                            borrow_index: Decimal::zero(),
                            liquidity_index: Decimal::zero(),
                            borrow_rate: Decimal::zero(),
                            liquidity_rate: Decimal::percent(0),
                            indexes_last_updated: 1u64,                            
                            collateral_total_scaled: Uint128::zero(),
                            debt_total_scaled: Uint128::zero()
                        }).unwrap())
                    },
                    Mars_MockQueryMsg::UnderlyingLiquidityAmount { denom, amount_scaled } => {
                        Ok(to_binary(&Uint128::new(10)).unwrap())
                    },
                    Mars_MockQueryMsg::UserCollateral { user, account_id, denom } => {
                        Ok(to_binary(&UserCollateralResponse {
                            denom: denom,
                            amount: Uint128::new(0),
                            amount_scaled: Uint128::new(0),
                            enabled: false
                        }).unwrap())
                    }
                }
            },
        );
        Box::new(contract)
    }
    pub fn redbank_contract_five() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Mars_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Mars_MockExecuteMsg::Deposit { account_id, on_behalf_of } => {
                    Ok(Response::default())
                    },
                    Mars_MockExecuteMsg::Withdraw { denom, amount, recipient, account_id, liquidation_related } => {
                        Ok(Response::default())
                        }
                }
            },
            |_, _, _, _: Mars_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Mars_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Mars_MockQueryMsg::Market { denom } => {
                        Ok(to_binary(&Market {
                            denom: String::from("uusdc"),
                            reserve_factor: Decimal::zero(),                            
                            interest_rate_model: InterestRateModel {
                                optimal_utilization_rate: Decimal::zero(),
                                base: Decimal::zero(),
                                slope_1: Decimal::zero(),
                                slope_2: Decimal::zero(),
                            },                            
                            borrow_index: Decimal::zero(),
                            liquidity_index: Decimal::zero(),
                            borrow_rate: Decimal::zero(),
                            liquidity_rate: Decimal::percent(5),
                            indexes_last_updated: 1u64,                            
                            collateral_total_scaled: Uint128::zero(),
                            debt_total_scaled: Uint128::zero()
                        }).unwrap())
                    },
                    Mars_MockQueryMsg::UnderlyingLiquidityAmount { denom, amount_scaled } => {
                        Ok(to_binary(&Uint128::new(5)).unwrap())
                    },
                    Mars_MockQueryMsg::UserCollateral { user, account_id, denom } => {
                        Ok(to_binary(&UserCollateralResponse {
                            denom: denom,
                            amount: Uint128::new(5),
                            amount_scaled: Uint128::new(5),
                            enabled: false
                        }).unwrap())
                    }
                }
            },
        );
        Box::new(contract)
    }

    pub fn redbank_interest_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Mars_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Mars_MockExecuteMsg::Deposit { account_id, on_behalf_of } => {
                    Ok(Response::default())
                    },
                    Mars_MockExecuteMsg::Withdraw { denom, amount, recipient, account_id, liquidation_related } => {
                        Ok(Response::default())
                        }
                }
            },
            |_, _, _, _: Mars_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Mars_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                Mars_MockQueryMsg::Market { denom } => {
                    Ok(to_binary(&Market {
                        denom: String::from("uusdc"),
                        reserve_factor: Decimal::zero(),                            
                        interest_rate_model: InterestRateModel {
                            optimal_utilization_rate: Decimal::zero(),
                            base: Decimal::zero(),
                            slope_1: Decimal::zero(),
                            slope_2: Decimal::zero(),
                        },                            
                        borrow_index: Decimal::zero(),
                        liquidity_index: Decimal::zero(),
                        borrow_rate: Decimal::zero(),
                        liquidity_rate: Decimal::percent(16),
                        indexes_last_updated: 1u64,                            
                        collateral_total_scaled: Uint128::zero(),
                        debt_total_scaled: Uint128::zero(),
                    }).unwrap())
                },
                Mars_MockQueryMsg::UnderlyingLiquidityAmount { denom, amount_scaled } => {
                    Ok(to_binary(&Uint128::new(16)).unwrap())
                },
                Mars_MockQueryMsg::UserCollateral { user, account_id, denom } => {
                    Ok(to_binary(&UserCollateralResponse {
                        denom: denom,
                        amount: Uint128::new(16),
                        amount_scaled: Uint128::new(10),
                        enabled: false
                    }).unwrap())
                }
            }
            },
        );
        Box::new(contract)
    }
    pub fn redbank_interest_v2_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Mars_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Mars_MockExecuteMsg::Deposit { account_id, on_behalf_of } => {
                    Ok(Response::default())
                    },
                    Mars_MockExecuteMsg::Withdraw { denom, amount, recipient, account_id, liquidation_related } => {
                        Ok(Response::default())
                        }
                }
            },
            |_, _, _, _: Mars_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Mars_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                Mars_MockQueryMsg::Market { denom } => {
                    Ok(to_binary(&Market {
                        denom: String::from("uusdc"),
                        reserve_factor: Decimal::zero(),                            
                        interest_rate_model: InterestRateModel {
                            optimal_utilization_rate: Decimal::zero(),
                            base: Decimal::zero(),
                            slope_1: Decimal::zero(),
                            slope_2: Decimal::zero(),
                        },                            
                        borrow_index: Decimal::zero(),
                        liquidity_index: Decimal::zero(),
                        borrow_rate: Decimal::zero(),
                        liquidity_rate: Decimal::percent(20),
                        indexes_last_updated: 1u64,                            
                        collateral_total_scaled: Uint128::zero(),
                        debt_total_scaled: Uint128::zero(),
                    }).unwrap())
                },
                Mars_MockQueryMsg::UnderlyingLiquidityAmount { denom, amount_scaled } => {
                    Ok(to_binary(&Uint128::new(20)).unwrap())
                },
                Mars_MockQueryMsg::UserCollateral { user, account_id, denom } => {
                    Ok(to_binary(&UserCollateralResponse {
                        denom: denom,
                        amount: Uint128::new(20),
                        amount_scaled: Uint128::new(10),
                        enabled: false
                    }).unwrap())
                }
            }
            },
        );
        Box::new(contract)
    }
    pub fn redbank_24() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Mars_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Mars_MockExecuteMsg::Deposit { account_id, on_behalf_of } => {
                    Ok(Response::default())
                    },
                    Mars_MockExecuteMsg::Withdraw { denom, amount, recipient, account_id, liquidation_related } => {
                        Ok(Response::default())
                        }
                }
            },
            |_, _, _, _: Mars_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Mars_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                Mars_MockQueryMsg::Market { denom } => {
                    Ok(to_binary(&Market {
                        denom: String::from("uusdc"),
                        reserve_factor: Decimal::zero(),                            
                        interest_rate_model: InterestRateModel {
                            optimal_utilization_rate: Decimal::zero(),
                            base: Decimal::zero(),
                            slope_1: Decimal::zero(),
                            slope_2: Decimal::zero(),
                        },                            
                        borrow_index: Decimal::zero(),
                        liquidity_index: Decimal::zero(),
                        borrow_rate: Decimal::zero(),
                        liquidity_rate: Decimal::percent(24),
                        indexes_last_updated: 1u64,                            
                        collateral_total_scaled: Uint128::zero(),
                        debt_total_scaled: Uint128::zero(),
                    }).unwrap())
                },
                Mars_MockQueryMsg::UnderlyingLiquidityAmount { denom, amount_scaled } => {
                    Ok(to_binary(&Uint128::new(20)).unwrap())
                },
                Mars_MockQueryMsg::UserCollateral { user, account_id, denom } => {
                    Ok(to_binary(&UserCollateralResponse {
                        denom: denom,
                        amount: Uint128::new(24),
                        amount_scaled: Uint128::new(10),
                        enabled: false
                    }).unwrap())
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
                &Addr::unchecked("god"),
                vec![coin(100_000_000, "uusdc")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![coin(14, "uusdc"), coin(100_000, "wrong_token"),coin(100_000_000, "factory/contract5/mars-usdc-vault")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract4"),
                vec![],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, MarsVaultContract) {
        let mut app = mock_app();

        //Instaniate Red Banks
        let redbank_id = app.store_code(redbank_contract_zero());
        let redbank_contract_zero_addr = app
            .instantiate_contract(
                redbank_id,
                Addr::unchecked(ADMIN),
                &Mars_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let redbank_id = app.store_code(redbank_contract_five());
        let redbank_contract_five_addr = app
            .instantiate_contract(
                redbank_id,
                Addr::unchecked(ADMIN),
                &Mars_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let redbank_id = app.store_code(redbank_contract_ten());

        let redbank_contract_addr = app
            .instantiate_contract(
                redbank_id,
                Addr::unchecked(ADMIN),
                &Mars_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let redbank_id = app.store_code(redbank_interest_contract());
        let redbank_v1_contract_addr = app
            .instantiate_contract(
                redbank_id,
                Addr::unchecked(ADMIN),
                &Mars_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        let redbank_id = app.store_code(redbank_interest_v2_contract());
        let redbank_v2_contract_addr = app
            .instantiate_contract(
                redbank_id,
                Addr::unchecked(ADMIN),
                &Mars_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Mars Vault contract        
        let vault_id = app.store_code(mars_vault_contract());

        let msg = InstantiateMsg { 
            vault_subdenom: String::from("mars-usdc-vault"), 
            deposit_token: String::from("uusdc"),
            mars_redbank_addr: redbank_contract_zero_addr.to_string(),
        };
        let vault_contract_addr = app
            .instantiate_contract(vault_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let vault_contract = MarsVaultContract(vault_contract_addr);


        let redbank_id = app.store_code(redbank_24());
        let redbank_v2_contract_addr = app
            .instantiate_contract(
                redbank_id,
                Addr::unchecked(ADMIN),
                &Mars_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();
        (app, vault_contract)
    }

    mod mars_vault {

        use std::str::FromStr;

        use cosmwasm_std::{coin, coins, BlockInfo, Uint128};
        use membrane::stability_pool_vault::APRResponse;

        use super::*;

        
        #[test]
        fn enter_vault() {
            let (mut app, vault_contract) = proper_instantiate();

            //Enter Vault: Error, wrong token
            let msg = ExecuteMsg::EnterVault { };
            let cosmos_msg = vault_contract.call(msg, vec![coin(10, "wrong_token")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            //Enter Vault: Error, multiple token
            let msg = ExecuteMsg::EnterVault { };
            let cosmos_msg = vault_contract.call(msg, vec![coin(10, "uusdc"), coin(10, "wrong_token")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Enter Vault
            let msg = ExecuteMsg::EnterVault { };
            let cosmos_msg = vault_contract.call(msg, vec![coin(5, "uusdc")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Query Vault token underlying
            let underlying_deposit_token: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    vault_contract.addr(),
                    &QueryMsg::VaultTokenUnderlying { 
                        vault_token_amount: Uint128::new(5_000_000)
                    },
                )
                .unwrap();
            assert_eq!(
                underlying_deposit_token,
                Uint128::new(5)
            );
            
            //Query Vault deposit token balance
            //Should be 0 bc everything was sent to the vault
            let balance = app
                .wrap()
                .query_balance(Addr::unchecked("contract4"), "uusdc")
                .unwrap().amount;
            assert_eq!(balance, Uint128::zero());
            
            //Enter Vault: None of the deposit is kept in the vault
            let msg = ExecuteMsg::EnterVault { };
            let cosmos_msg = vault_contract.call(msg, vec![coin(5, "uusdc")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg.clone()).unwrap();

            // Query Vault token underlying
            let underlying_deposit_token: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    vault_contract.addr(),
                    &QueryMsg::VaultTokenUnderlying { 
                        vault_token_amount: Uint128::new(5_000_000)
                    },
                )
                .unwrap();
            assert_eq!(
                underlying_deposit_token,
                Uint128::new(5)
            );
            
            //Query Vault deposit token balance
            //Should be 0 bc everything was sent to the vault
            let balance = app
                .wrap()
                .query_balance(Addr::unchecked("contract4"), "uusdc")
                .unwrap().amount;
            assert_eq!(balance, Uint128::new(0));
        }
        
        #[test]
        fn exit_vault() {
            let (mut app, vault_contract) = proper_instantiate();

            //Enter Vault
            let msg = ExecuteMsg::EnterVault { };
            let cosmos_msg = vault_contract.call(msg, vec![coin(5, "uusdc")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Change redbank contract to 5 uusdc
            let msg = ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: Some("contract1".to_string()),
            };
            let cosmos_msg = vault_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();


            //Skip ahead 1 day
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(86400), //Added a day
                chain_id: app.block_info().chain_id,
            }); 

            //Enter Vault: None of the deposit is kept in the vault
            let msg = ExecuteMsg::EnterVault { };
            let cosmos_msg = vault_contract.call(msg, vec![coin(5, "uusdc")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg.clone()).unwrap();
            
            //Change redbank contract to 10 uusdc
            let msg = ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: Some("contract2".to_string()),
            };
            let cosmos_msg = vault_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Skip ahead 4 days
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(86400*4), //Added 4 days
                chain_id: app.block_info().chain_id,
            }); 

            //Change redbank contract to v1 interest: 16 uusdc
            let msg = ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: Some("contract3".to_string()),
            };
            let cosmos_msg = vault_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            // Query Vault token underlying
            let underlying_deposit_token: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    vault_contract.addr(),
                    &QueryMsg::VaultTokenUnderlying { 
                        vault_token_amount: Uint128::new(5_000_000)
                    },
                )
                .unwrap();
            assert_eq!(
                underlying_deposit_token,
                Uint128::new(8)
            );

            //Crank the vault
            let msg = ExecuteMsg::CrankAPR {  };
            let cosmos_msg = vault_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();


            //Skip ahead 2days
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(86400*2), //Added 2 days
                chain_id: app.block_info().chain_id,
            }); 

            //Change redbank contract to v2: 20 uusdc
            let msg = ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: Some("contract4".to_string()),
            };
            let cosmos_msg = vault_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Query APR for the vault
            let apr: APRResponse = app
                .wrap()
                .query_wasm_smart(
                    vault_contract.addr(),
                    &QueryMsg::APR {},
                )
                .unwrap();
            assert_eq!(
                apr.week_apr,
                Some(Decimal::from_str("0.155714285714285712").unwrap())
            );

            //Enter Vault as a new user who gets less vault tokens
            let msg = ExecuteMsg::EnterVault { };
            let cosmos_msg = vault_contract.call(msg, vec![coin(4, "uusdc")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Switch to 24 TVL redbank
            let msg = ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: Some("contract6".to_string()),
            };
            let cosmos_msg = vault_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            // Query Vault token underlying
            let underlying_deposit_token: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    vault_contract.addr(),
                    &QueryMsg::VaultTokenUnderlying { 
                        vault_token_amount: Uint128::new(5_000_000)
                    },
                )
                .unwrap();
            assert_eq!(
                underlying_deposit_token,
                Uint128::new(10)
            );
            let underlying_deposit_token: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    vault_contract.addr(),
                    &QueryMsg::VaultTokenUnderlying { 
                        vault_token_amount: Uint128::new(2000000)
                    },
                )
                .unwrap();
            assert_eq!(
                underlying_deposit_token,
                Uint128::new(4)
            );

            //Exit Vault: THE RATE ASSURANCE ERRORS BC WE CAN'T REMOVE TOKENS FROM THE MARS CONTRACT MID-EXECUTION
            let msg = ExecuteMsg::ExitVault { };
            let cosmos_msg = vault_contract.call(msg, vec![coin(2000000, "factory/contract5/mars-usdc-vault")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let msg = ExecuteMsg::ExitVault { };
            let cosmos_msg = vault_contract.call(msg, vec![coin(5000000, "factory/contract5/mars-usdc-vault")]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Swap to 10 TVL redbank
            let msg = ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: Some("contract2".to_string()),
            };
            let cosmos_msg = vault_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            // Query Vault token underlying
            let underlying_deposit_token: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    vault_contract.addr(),
                    &QueryMsg::VaultTokenUnderlying { 
                        vault_token_amount: Uint128::new(5_000_000)
                    },
                )
                .unwrap();
            assert_eq!(
                underlying_deposit_token,
                Uint128::new(10)
            );

            //Query user balance
            // let balance = app
            //     .wrap()
            //     .query_balance(Addr::unchecked(USER), "uusdc")
            //     .unwrap().amount;
            // assert_eq!(balance, Uint128::new(4));
        }
    }

}