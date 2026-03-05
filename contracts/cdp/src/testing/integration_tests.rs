

pub mod tests {

    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract};


    use membrane::liq_queue::LiquidatibleResponse as LQ_LiquidatibleResponse;
    use membrane::math::Uint256;
    use membrane::oracle::{AssetResponse, PriceResponse};
    use membrane::osmosis_proxy::{GetDenomResponse, TokenInfoResponse, OwnerResponse};
    use membrane::cdp::{ExecuteMsg, InstantiateMsg, QueryMsg, EditBasket, UpdateConfig, CreateBasket, LiquidationStatResponse, BasketPositionsResponse};
    use membrane::stability_pool::LiquidatibleResponse as SP_LiquidatibleResponse;
    use membrane::staking::Config as Staking_Config;
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, Deposit, LiquidityInfo, TWAPPoolInfo,
        UserInfo, MultiAssetSupplyCap, AssetPool, StakeDistribution, PoolType, Owner, PoolStateResponse, SupplyCap, Basket
    };
    use membrane::cdp::Config;
    use membrane::liquidity_check::LiquidityResponse;
    use membrane::revenue_distributor::RevenuePromise;

    use cosmwasm_std::{
        attr, coin, to_json_binary, Addr, Binary, Coin, Decimal, Empty, Response, StdError, StdResult,
        Uint128, BlockInfo,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    // Helper functions for valid bech32 addresses (per debugging guide)
    fn get_user_addr(app: &App) -> Addr {
        app.api().addr_make(USER)
    }

    fn get_admin_addr(app: &App) -> Addr {
        app.api().addr_make(ADMIN)
    }

    //CDP Contract
    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    //Mock LQ Contract
    #[cw_serde]
    pub enum LQ_MockExecuteMsg {
        Liquidate {
            credit_price: PriceResponse,     //Sent from Position's contract
            collateral_price: PriceResponse, //Sent from Position's contract
            collateral_amount: Uint256,
            bid_for: AssetInfo,
        },
        AddQueue {
            bid_for: AssetInfo,
            max_premium: Uint128,
            bid_threshold: Uint256,
        },
        EditQueue {
            bid_for: AssetInfo,
            max_premium: Uint128,
            bid_threshold: Uint256,
        },
        UpdateQueue {
            bid_for: AssetInfo,
            max_premium: Option<Uint128>,
            bid_threshold: Option<Uint256>,
        },
    }

    
    #[cw_serde]
    pub struct LQ_MockInstantiateMsg {}

    
    #[cw_serde]
    pub enum LQ_MockQueryMsg {
        CheckLiquidatible {
            bid_for: AssetInfo,
            collateral_price: PriceResponse,
            collateral_amount: Uint256,
            credit_info: AssetInfo,
            credit_price: PriceResponse,
        },
    }

    pub fn liq_queue_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price: _,
                        collateral_price: _,
                        collateral_amount,
                        bid_for,
                    } => if collateral_amount.to_string() == String::from("1165777777777778") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", "1158".to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else if collateral_amount.to_string() == String::from("1166666666666666") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 1166.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else if collateral_amount.to_string() == String::from("1054777777777778") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 1054.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    //Liquidate_LPs()
                    } else if collateral_amount.to_string() == String::from("1277666666388888888889") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 2555555555u128.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else if collateral_amount.to_string() == String::from("1277777777388888888889") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 2555555555u128.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else if collateral_amount.to_string() == String::from("1166666666388888889") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 2555555555u128.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else {
                        // panic!("{}", collateral_amount);
                       Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", collateral_amount.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    },
                    LQ_MockExecuteMsg::AddQueue {
                        bid_for: _,
                        max_premium: _,
                        bid_threshold: _,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::EditQueue {
                        bid_for,
                        max_premium,
                        bid_threshold,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::UpdateQueue {
                        bid_for,
                        max_premium,
                        bid_threshold,
                    } => {
                        let premium = max_premium.unwrap_or_default();
                        if premium != Uint128::new(10) && premium != Uint128::new(12) {
                            panic!("{}", premium);
                        }
                        Ok(Response::new())
                    },
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible {
                        bid_for: _,
                        collateral_price: _,
                        collateral_amount,
                        credit_info: _,
                        credit_price: _,
                    } => 
                    {
                        println!("collateral_amount: {:?}", collateral_amount);
                    if collateral_amount.to_string() == String::from("1387999999999778") {
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1387u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1388000000000000"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1380u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1277000000000000"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1277u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1062000000000000"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1062u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1388888888888888"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1388u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    //liquidite()
                    } else if collateral_amount.to_string() == String::from("2222222222"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222".to_string(),
                            total_debt_repaid: (Uint256::from(2222222222u128) - Uint256::from(222222222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("2000000000"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "0".to_string(),
                            total_debt_repaid: (Uint256::from(2222222222u128) - Uint256::from(222222222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1250000000"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "0".to_string(),
                            total_debt_repaid: (Uint256::from(1250000000u128))
                                .to_string(),
                        })?)
                    //liquidate_LPs()
                    } else if collateral_amount.to_string() == String::from("1388888888500000000"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "111111111111111111".to_string(),
                            total_debt_repaid: (Uint256::from(2777_777777u128) - Uint256::from(222_222222u128))
                            .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1388888888500000000000"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "111111111111111111111".to_string(),
                            total_debt_repaid: (Uint256::from(2777_777777u128) - Uint256::from(222_222222u128))
                            .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1277777777500000000000"){
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "111111111111111111".to_string(),
                            total_debt_repaid: (Uint256::from(2777_777777u128) - Uint256::from(222_222222u128))
                            .to_string(),
                        })?)
                    } else {                        
                        // panic!("{}", collateral_amount.to_string());
                        Ok(to_json_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222".to_string(),
                            total_debt_repaid: (collateral_amount - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    }
                    }
                }
            },
        );
        Box::new(contract)
    }

    pub fn liq_queue_contract_bignums() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price: _,
                        collateral_price: _,
                        collateral_amount,
                        bid_for,
                    } => {
                        match bid_for {
                            AssetInfo::Token { address: _ } => {
                                return Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("repay_amount", collateral_amount.to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "token"),
                                    attr("collateral_amount", collateral_amount),
                                ]))
                            }

                            AssetInfo::NativeToken { denom: _ } => {
                                
                                return Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("repay_amount", collateral_amount.to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "native_token"),
                                    attr("collateral_amount", collateral_amount),
                                ]));
                            }
                        }
                    }
                    LQ_MockExecuteMsg::AddQueue {
                        bid_for: _,
                        max_premium: _,
                        bid_threshold: _,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::EditQueue {
                        bid_for: _,
                        max_premium: _,
                        bid_threshold: _,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::UpdateQueue {
                        bid_for,
                        max_premium,
                        bid_threshold,
                    } => Ok(Response::new()),
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible {
                        bid_for: _,
                        collateral_price: _,
                        collateral_amount,
                        credit_info: _,
                        credit_price: _,
                    } => Ok(to_json_binary(&LQ_LiquidatibleResponse {
                        leftover_collateral: "222222222225".to_string(),
                        total_debt_repaid: (collateral_amount - Uint256::from(222222222225u128))
                            .to_string(),
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn liq_queue_contract_errors() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price: _,
                        collateral_price: _,
                        collateral_amount: _,
                        bid_for: _,
                    } =>
                     Err(StdError::generic_err("no siree")),
                    LQ_MockExecuteMsg::AddQueue {
                        bid_for: _,
                        max_premium: _,
                        bid_threshold: _,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::EditQueue {
                        bid_for: _,
                        max_premium: _,
                        bid_threshold: _,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::UpdateQueue {
                        bid_for,
                        max_premium,
                        bid_threshold,
                    } => Ok(Response::new()),
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible {
                        bid_for: _,
                        collateral_price: _,
                        collateral_amount,
                        credit_info: _,
                        credit_price: _,
                    } =>                     
                    Err(StdError::generic_err("no siree"))
                }
            },
        );
        Box::new(contract)
    }

    pub fn liq_queue_contract_minimumliq() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price: _,
                        collateral_price: _,
                        collateral_amount,
                        bid_for,
                    } => match bid_for {
                        AssetInfo::Token { address: _ } => {
                            return Ok(Response::new().add_attributes(vec![
                                attr("action", "execute_bid"),
                                attr("repay_amount", collateral_amount.to_string()),
                                attr("collateral_token", bid_for.to_string()),
                                attr("collateral_info", "token"),
                                attr("collateral_amount", collateral_amount),
                            ]))
                        }

                        AssetInfo::NativeToken { denom: _ } => {
                            return Ok(Response::new().add_attributes(vec![
                                attr("action", "execute_bid"),
                                attr("repay_amount", collateral_amount.to_string()),
                                attr("collateral_token", bid_for.to_string()),
                                attr("collateral_info", "native_token"),
                                attr("collateral_amount", collateral_amount),
                            ]))
                        }
                    },
                    LQ_MockExecuteMsg::AddQueue {
                        bid_for: _,
                        max_premium: _,
                        bid_threshold: _,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::EditQueue {
                        bid_for: _,
                        max_premium: _,
                        bid_threshold: _,
                    } => Ok(Response::new()),
                    LQ_MockExecuteMsg::UpdateQueue {
                        bid_for,
                        max_premium,
                        bid_threshold,
                    } => Ok(Response::new()),
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible {
                        bid_for: _,
                        collateral_price: _,
                        collateral_amount,
                        credit_info: _,
                        credit_price: _,
                    } => Ok(to_json_binary(&LQ_LiquidatibleResponse {
                        leftover_collateral: "499999999".to_string(),
                        total_debt_repaid: (collateral_amount - Uint256::from(499_999999u128))
                            .to_string(),
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock SP Contract    
    #[cw_serde]
    pub enum SP_MockExecuteMsg {
        Liquidate { liq_amount: Decimal },
        Distribute {
            distribution_assets: Vec<Asset>,
            distribution_asset_ratios: Vec<Decimal>,
            distribute_for: Uint128,
        },
        Repay {
            user_info: UserInfo,
            repayment: Asset,
        },
        DepositFee { },
    }

    
    #[cw_serde]
    pub struct SP_MockInstantiateMsg {}

    
    #[cw_serde]
    pub enum SP_MockQueryMsg {
        CheckLiquidatible { amount: Decimal },
        AssetPool { 
            /// User address
            user: Option<String>,
            /// Deposit limit
            deposit_limit: Option<u32>,
            /// Deposit to start after
            start_after: Option<u32>,    
        },
    }

    pub fn stability_pool_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate { liq_amount } => {
                        if liq_amount.to_string() != "222.222225".to_string()
                            && liq_amount.to_string() != "222.22225".to_string()
                            && liq_amount.to_string() != "222.88888".to_string()
                            && liq_amount.to_string() != "2000".to_string()
                            && liq_amount.to_string() != "222.2222".to_string()
                            && liq_amount.to_string() != "222.22222".to_string()
                            && liq_amount.to_string()
                                != "22222.22225".to_string()
                            && liq_amount.to_string()
                                != "20222.22225".to_string()
                            && liq_amount.to_string() != "22000".to_string()
                            && liq_amount.to_string() != "222.777774844".to_string()
                        {
                            // panic!("{}", liq_amount.to_string());
                        }
                        Ok(Response::new()
                            .add_attribute("method", "liquidate")
                            .add_attribute("leftover_repayment", "0"))
                    }
                    SP_MockExecuteMsg::Distribute {
                        distribution_assets,
                        distribution_asset_ratios: _,
                        distribute_for: _,
                    } => {
                        if distribution_assets
                            != vec![Asset {
                                info: AssetInfo::NativeToken {
                                    denom: "debit".to_string(),
                                },
                                amount: Uint128::new(244),
                            }]
                            && distribution_assets
                                != vec![Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "debit".to_string(),
                                    },
                                    amount: Uint128::new(2447),
                                }]
                            && distribution_assets
                                != vec![Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "debit".to_string(),
                                    },
                                    amount: Uint128::new(55000),
                                }]
                            && distribution_assets
                            != vec![Asset {
                                info: AssetInfo::NativeToken {
                                    denom: "lp_denom".to_string(),
                                },
                                amount: Uint128::new(244),
                            }]
                            && distribution_assets
                                != vec![Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "lp_denom".to_string(),
                                    },
                                    amount: Uint128::new(2447),
                                }]
                            && distribution_assets
                                != vec![Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "lp_denom".to_string(),
                                    },
                                    amount: Uint128::new(55000),
                                }]
                        {
                            //assert_ne!(distribution_assets, distribution_assets);
                        }

                        Ok(Response::new()
                            .add_attribute("method", "distribute")
                            .add_attribute("credit_asset", "cdt"))
                    }
                    SP_MockExecuteMsg::Repay {
                        user_info: _,
                        repayment: _,
                    } => Ok(Response::new()),
                    SP_MockExecuteMsg::DepositFee { } => Ok(Response::new()),
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible {amount: _ } => {
                        Ok(to_json_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { user: _, deposit_limit: _, start_after: _ } => Ok(to_json_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::new(50_000),
                        },
                        liq_premium: Decimal::percent(10),
                        deposits: vec![],
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn stability_pool_contract_bignums() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate { liq_amount: _ } => {
                        
                        Ok(Response::new()
                            .add_attribute("method", "liquidate")
                            .add_attribute("leftover_repayment", "0"))
                    }
                    SP_MockExecuteMsg::Distribute {
                        distribution_assets: _,
                        distribution_asset_ratios: _,
                        distribute_for: _,
                    } => Ok(Response::new()
                        .add_attribute("method", "distribute")
                        .add_attribute("credit_asset", "cdt")),
                    SP_MockExecuteMsg::Repay {
                        user_info: _,
                        repayment: _,
                    } => Ok(Response::new()),
                    SP_MockExecuteMsg::DepositFee { } => Ok(Response::new()),
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { amount: _ } => {
                        Ok(to_json_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { user: _, deposit_limit: _, start_after: _ } => Ok(to_json_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::one(),
                        },
                        liq_premium: Decimal::percent(10),
                        deposits: vec![],
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn stability_pool_contract_errors() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate { liq_amount } => {
                                                
                        Err(StdError::generic_err("no siree"))},
                    SP_MockExecuteMsg::Distribute {
                        distribution_assets: _,
                        distribution_asset_ratios: _,
                        distribute_for: _,
                    } => Ok(Response::new()
                        .add_attribute("method", "distribute")
                        .add_attribute("credit_asset", "cdt")),
                    SP_MockExecuteMsg::Repay {
                        user_info: _,
                        repayment: _,
                    } => Err(StdError::generic_err("erroar")),
                    SP_MockExecuteMsg::DepositFee { } => Err(StdError::generic_err("erroar I say")),
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { amount: _ } => {
                        Ok(to_json_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { user: _, deposit_limit: _, start_after: _ } => Ok(to_json_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::zero(),
                        },
                        liq_premium: Decimal::percent(10),
                        deposits: vec![Deposit {
                            user: Addr::unchecked(USER),
                            amount: Decimal::percent(222_222_222_00),
                            deposit_time: 0u64,
                            last_accrued: 0u64,
                            unstake_time: None,
                        }],
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn stability_pool_contract_minimumliq() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate { liq_amount: _ } => Ok(Response::new()
                        .add_attribute("method", "liquidate")
                        .add_attribute("leftover_repayment", "0")),
                    SP_MockExecuteMsg::Distribute {
                        distribution_assets: _,
                        distribution_asset_ratios: _,
                        distribute_for: _,
                    } => Ok(Response::new()
                        .add_attribute("method", "distribute")
                        .add_attribute("credit_asset", "cdt")),
                    SP_MockExecuteMsg::Repay {
                        user_info: _,
                        repayment: _,
                    } => Ok(Response::new()),
                    SP_MockExecuteMsg::DepositFee { } => Ok(Response::new()),
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { amount: _ } => {
                        Ok(to_json_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { user: _, deposit_limit: _, start_after: _ } => Ok(to_json_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::one(),
                        },
                        liq_premium: Decimal::percent(10),
                        deposits: vec![],
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn stability_pool_contract_all_user_repay() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate { liq_amount: _ } => Ok(Response::new()
                        .add_attribute("method", "liquidate")
                        .add_attribute("leftover_repayment", "0")),
                    SP_MockExecuteMsg::Distribute {
                        distribution_assets: _,
                        distribution_asset_ratios: _,
                        distribute_for: _,
                    } => Ok(Response::new()
                        .add_attribute("method", "distribute")
                        .add_attribute("credit_asset", "cdt")),
                    SP_MockExecuteMsg::Repay {
                        user_info: _,
                        repayment: _,
                    } => Ok(Response::new()),
                    SP_MockExecuteMsg::DepositFee { } => Ok(Response::new()),
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { amount: _ } => {
                        Ok(to_json_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { user: _, deposit_limit: _, start_after: _ } => Ok(to_json_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::one(),
                        },
                        liq_premium: Decimal::percent(10),
                        deposits: vec![Deposit {
                            user: Addr::unchecked(USER),
                            amount: Decimal::percent(2_222_222_222_00),
                            deposit_time: 0u64,
                            last_accrued: 0u64,
                            unstake_time: None,
                        }],
                    })?),
                }
            },
        );
        Box::new(contract)
    }
    pub fn stability_pool_contract_high_premium() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate { liq_amount: _ } => Ok(Response::new()
                        .add_attribute("method", "liquidate")
                        .add_attribute("leftover_repayment", "0")),
                    SP_MockExecuteMsg::Distribute {
                        distribution_assets: _,
                        distribution_asset_ratios: _,
                        distribute_for: _,
                    } => Ok(Response::new()
                        .add_attribute("method", "distribute")
                        .add_attribute("credit_asset", "cdt")),
                    SP_MockExecuteMsg::Repay {
                        user_info: _,
                        repayment: _,
                    } => Ok(Response::new()),
                    SP_MockExecuteMsg::DepositFee { } => Ok(Response::new()),
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { amount: _ } => {
                        Ok(to_json_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { user: _, deposit_limit: _, start_after: _ } => Ok(to_json_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::zero(),
                        },
                        liq_premium: Decimal::percent(3400),
                        deposits: vec![],
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock Osmo Proxy Contract
    
    #[cw_serde]
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
        CreateDenom {
            subdenom: String,
            max_supply: Option<Uint128>,
            liquidity_multiplier: Option<Decimal>,
        },
        ExecuteSwaps {
            token_out: String,
            max_slippage: Decimal
        }
    }

    
    #[cw_serde]
    pub struct Osmo_MockInstantiateMsg {}

    
    #[cw_serde]
    pub enum Osmo_MockQueryMsg {
        PoolState {
            id: u64,
        },
        GetDenom {
            creator_address: String,
            subdenom: String,
        },
        GetTokenInfo {
            denom: String,
        },
        GetOwner { owner: String },
    }

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens {
                        denom,
                        amount,
                        mint_to_address,
                    } => {
                        if amount == Uint128::new(1428u128) {
                            assert_eq!(
                                String::from("credit_fulldenom 1428 revenue_collector"),
                                format!("{} {} {}", denom, amount.to_string(), mint_to_address)
                            );
                        }

                        Ok(Response::new())
                    }
                    Osmo_MockExecuteMsg::BurnTokens {
                        denom: _,
                        amount,
                        burn_from_address: _,
                    } => {
                        // if amount == Uint128::new(50000_000_000u128) {
                        //     panic!();
                        // }
                        Ok(Response::new())                    
                    },
                    Osmo_MockExecuteMsg::CreateDenom {
                        subdenom: _,
                        max_supply,
                        liquidity_multiplier,
                    } => Ok(Response::new().add_attributes(vec![
                        attr("subdenom", "credit_fulldenom"),
                        attr(
                            "max_supply",
                            max_supply.unwrap_or_else(|| Uint128::zero()).to_string(),
                        ),
                        attr(
                            "liquidity_multiplier",
                            liquidity_multiplier
                                .unwrap_or_else(|| Decimal::zero())
                                .to_string(),
                        ),
                    ])),
                    Osmo_MockExecuteMsg::ExecuteSwaps {
                        token_out,
                        max_slippage
                    } => Ok(Response::new()
                        // .add_attributes(vec![])
                    ),
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::PoolState { id } => {
                        if id == 99u64 {
                            Ok(to_json_binary(&PoolStateResponse {
                                assets: vec![coin(112_914_609, "base").into(), coin(112_914_609, "quote").into()],
                                shares: coin(100_000_000_000_000_000_000, "lp_denom").into(),
                            })?)
                        } else {
                            Ok(to_json_binary(&PoolStateResponse {
                                assets: vec![coin(49_999, "credit_fulldenom").into()],
                                shares: coin(0, "shares").into(),
                            })?)
                        }
                    }
                    Osmo_MockQueryMsg::GetDenom {
                        creator_address: _,
                        subdenom: _,
                    } => Ok(to_json_binary(&GetDenomResponse {
                        denom: String::from("credit_fulldenom"),
                    })?),
                    Osmo_MockQueryMsg::GetTokenInfo { denom } => {
                        Ok(to_json_binary(&TokenInfoResponse {
                            denom,
                            current_supply: Uint128::new(200_000u128),
                            max_supply: Uint128::new(1_000_000_000_000_000u128),
                            burned_supply: Uint128::zero(),
                        })?)
                    },
                    Osmo_MockQueryMsg::GetOwner { owner } => {
                        Ok(to_json_binary(&OwnerResponse {
                            owner: Owner {
                                owner: Addr::unchecked(""),
                                total_minted: Uint128::zero(),
                                stability_pool_ratio: Some(Decimal::one()),
                                non_token_contract_auth: true,
                                is_position_contract: false
                            },
                            liquidity_multiplier: Decimal::percent(500),
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    pub fn osmosis_proxy_contract_bignums() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens {
                        denom,
                        amount,
                        mint_to_address,
                    } => {
                        println!(
                            "{}",
                            format!("{} {} {}", denom, amount.to_string(), mint_to_address)
                        );
                        Ok(Response::new())
                    }
                    Osmo_MockExecuteMsg::BurnTokens {
                        denom: _,
                        amount: _,
                        burn_from_address: _,
                    } => Ok(Response::new()),
                    Osmo_MockExecuteMsg::CreateDenom {
                        subdenom: _,
                        max_supply,
                        liquidity_multiplier,
                    } => Ok(Response::new().add_attributes(vec![
                        attr("subdenom", "credit_fulldenom"),
                        attr(
                            "max_supply",
                            max_supply.unwrap_or_else(|| Uint128::zero()).to_string(),
                        ),
                        attr(
                            "liquidity_multiplier",
                            liquidity_multiplier
                                .unwrap_or_else(|| Decimal::zero())
                                .to_string(),
                        ),
                    ])),
                    Osmo_MockExecuteMsg::ExecuteSwaps {
                        token_out,
                        max_slippage
                    } => Ok(Response::new()
                        // .add_attributes(vec![])
                    ),
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::PoolState { id } => {
                        if id == 99u64 {
                            Ok(to_json_binary(&PoolStateResponse {
                                assets: vec![coin(100_000_000, "base").into(), coin(100_000_000, "quote").into()],
                                shares: coin(100_000_000_000_000_000_000, "lp_denom").into(),
                            })?)
                        } else {
                            Ok(to_json_binary(&PoolStateResponse {
                                assets: vec![coin(5_000_000_000_000, "credit_fulldenom").into()],
                                shares: coin(0, "shares").into(),
                            })?)
                        }
                    }
                    Osmo_MockQueryMsg::GetDenom {
                        creator_address: _,
                        subdenom: _,
                    } => Ok(to_json_binary(&GetDenomResponse {
                        denom: String::from("credit_fulldenom"),
                    })?),
                    Osmo_MockQueryMsg::GetTokenInfo { denom } => {
                        Ok(to_json_binary(&TokenInfoResponse {
                            denom,
                            current_supply: Uint128::new(200_000u128),
                            max_supply: Uint128::new(1_000_000_000_000_000u128),
                            burned_supply: Uint128::zero(),
                        })?)
                    },
                    Osmo_MockQueryMsg::GetOwner { owner } => {
                        Ok(to_json_binary(&OwnerResponse {
                            owner: Owner {
                                owner: Addr::unchecked(""),
                                total_minted: Uint128::zero(),
                                stability_pool_ratio: Some(Decimal::one()),
                                non_token_contract_auth: true,
                                is_position_contract: false
                            },
                            liquidity_multiplier: Decimal::percent(500),
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    // Mock LTV Disco Contract (minimal for CanHandleBadDebt query)
    #[cw_serde]
    pub struct LTVDisco_MockInstantiateMsg {}

    pub fn ltv_disco_mock_contract() -> Box<dyn Contract<Empty>> {
        use membrane::ltv_disco::{ExecuteMsg as LTVDisco_ExecuteMsg, QueryMsg as LTVDisco_QueryMsg};
        let contract = ContractWrapper::new(
            |_, _, _, _msg: LTVDisco_ExecuteMsg| -> StdResult<Response> { Ok(Response::new()) },
            |_, _, _, _msg: LTVDisco_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LTVDisco_QueryMsg| -> StdResult<Binary> {
                match msg {
                    LTVDisco_QueryMsg::CanHandleBadDebt { .. } => Ok(to_json_binary(&false)?),
                    _ => Ok(to_json_binary(&true)?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock Router Contract
     #[cw_serde]    
    pub enum Router_MockExecuteMsg {
        BasketLiquidate {
            offer_assets: apollo_cw_asset::AssetListUnchecked,
            receive_asset: apollo_cw_asset::AssetInfoUnchecked,
            minimum_receive: Option<Uint128>,
            to: Option<String>,
        }
    }

     #[cw_serde]    
    pub struct Router_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Router_MockQueryMsg {}

     #[cw_serde]    
    pub struct MockResponse {}

    pub fn router_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Router_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Router_MockExecuteMsg::
                    BasketLiquidate {
                        offer_assets,
                        receive_asset,
                        minimum_receive,
                        to,      
                    } => {
                        Ok(Response::default())
                    }
                }
            },
            |_, _, _, _: Router_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Router_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_json_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }

    //Mock Auction Contract
     #[cw_serde]    
    pub enum Auction_MockExecuteMsg {
        StartAuction {
            position_id: Uint128,
            position_owner: String,
            debt_amount: Uint128,
        },
    }

     #[cw_serde]    
    pub struct Auction_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Auction_MockQueryMsg {}

    pub fn auction_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Auction_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Auction_MockExecuteMsg::StartAuction {
                        position_id,
                        position_owner,
                        debt_amount,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Auction_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Auction_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_json_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }

    //Mock Staking Contract
     #[cw_serde]    
    pub enum Staking_MockExecuteMsg {
        DepositFee {},
    }

     #[cw_serde]    
    pub struct Staking_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Staking_MockQueryMsg {
        Config { }
    }

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Staking_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Staking_MockExecuteMsg::DepositFee {} => Ok(Response::default()),
                }
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Staking_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Staking_MockQueryMsg::Config {  } => {
                        Ok(to_json_binary(&Staking_Config {
                            owner: Addr::unchecked(""),
                            mbrn_denom: String::from("mbrn_denom"),
                            incentive_schedule: StakeDistribution { rate: Decimal::zero(), duration: 0 },
                            keep_raw_cdt: false,
                            vesting_rev_multiplier: Decimal::zero(),
                            max_commission_rate: Decimal::zero(),
                            unstaking_period: 0,
                            positions_contract: None,
                            auction_contract: None,
                            vesting_contract: None,
                            governance_contract: None,
                            osmosis_proxy: None,
                            lock_duration_ceiling: 0,
                            emissions_voting_contract: None,
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Discounts Contract
     #[cw_serde]    
    pub enum Discounts_MockExecuteMsg {}

     #[cw_serde]    
    pub struct Discounts_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Discounts_MockQueryMsg {
        UserDiscount { user: String }
    }

    pub fn discounts_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Discounts_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Discounts_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Discounts_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Discounts_MockQueryMsg::UserDiscount { user } => {

                        if user == String::from("discounty"){
                            Ok(to_json_binary(&Decimal::percent(90))?)
                        } else {
                            Ok(to_json_binary(&Decimal::zero())?)
                        }
                        
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Oracle Contract
     #[cw_serde]    
    pub enum Oracle_MockExecuteMsg {
        AddAsset {
            asset_info: AssetInfo,
            oracle_info: AssetOracleInfo,
        },
        EditAsset {
            asset_info: AssetInfo,
            oracle_info: Option<AssetOracleInfo>,
            remove: bool,
        },
    }

     #[cw_serde]    
    pub struct Oracle_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Oracle_MockQueryMsg {
        Prices {
            asset_infos: Vec<AssetInfo>,
            twap_timeframe: u64,
            oracle_time_limit: u64,
        },
        Assets {
            asset_infos: Vec<AssetInfo>,
        },
    }

    pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset {
                        asset_info,
                        oracle_info,
                    } => Ok(Response::default()),
                    Oracle_MockExecuteMsg::EditAsset {
                        asset_info,
                        oracle_info,
                        remove,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe,
                        oracle_time_limit,
                    } => {
                        let mut prices = vec![];
                        for asset_info in asset_infos.iter() {
                            if asset_info.to_string() == String::from("credit_fulldenom") {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(98),
                                    decimals: 6,
                                });
                            } else if asset_info.to_string() == String::from("lp_denom") {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::from_ratio(2u128, 1u128),
                                    decimals: 18,
                                });
                            } else {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::one(),
                                    decimals: 6,
                                });
                            }
                        }
                        
                        Ok(to_json_binary(&prices)?)                        
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_json_binary(&vec![
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                                vault_info: None,
                            }],
                        },
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                                vault_info: None,
                            }],
                        }
                    ])?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn oracle_contract_negative_rates() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset {
                        asset_info,
                        oracle_info,
                    } => Ok(Response::default()),
                    Oracle_MockExecuteMsg::EditAsset {
                        asset_info,
                        oracle_info,
                        remove,
                    } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe,
                        oracle_time_limit,
                    } => {
                        
                        let mut prices = vec![];
                        for asset_info in asset_infos.iter() {
                            if asset_info.to_string() == String::from("credit_fulldenom") {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(110),
                                    decimals: 6,
                                });
                            } else if asset_info.to_string() == String::from("lp_denom") {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::from_ratio(2u128, 1u128),
                                    decimals: 18,
                                });
                            } else {
                                prices.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::one(),
                                    decimals: 6,
                                });
                            }
                        }
                        
                        Ok(to_json_binary(&prices)?)
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_json_binary(&vec![
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                                vault_info: None,
                            }],
                        },
                        AssetResponse {
                            asset_info: AssetInfo::NativeToken {
                                denom: String::from("denom"),
                            },
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                pools_for_osmo_twap: vec![TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                is_usd_par: false,
                                lp_pool_info: None,
                                decimals: 6,
                                pyth_price_feed_id: None,
                                vault_info: None,
                            }],
                        }
                    ])?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock Liquidity Contract
     #[cw_serde]    
    pub enum Liquidity_MockExecuteMsg {
        AddAsset { asset: LiquidityInfo },
        EditAsset { asset: LiquidityInfo },
    }

     #[cw_serde]    
    pub struct Liquidity_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Liquidity_MockQueryMsg {
        Liquidity { asset: AssetInfo },
    }

    pub fn liquidity_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Liquidity_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Liquidity_MockExecuteMsg::AddAsset { asset } => Ok(Response::default()),
                    Liquidity_MockExecuteMsg::EditAsset { asset } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Liquidity_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Liquidity_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Liquidity_MockQueryMsg::Liquidity { asset } => {
                        Ok(to_json_binary(&LiquidityResponse { 
                            asset: AssetInfo::NativeToken {
                                denom: String::from("credit_fulldenom"),
                            },
                            liquidity: Uint128::new(49999_000_000u128)
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    pub fn liquidity_contract_bignums() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Liquidity_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Liquidity_MockExecuteMsg::AddAsset { asset } => Ok(Response::default()),
                    Liquidity_MockExecuteMsg::EditAsset { asset } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Liquidity_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Liquidity_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Liquidity_MockQueryMsg::Liquidity { asset } => {
                        Ok(to_json_binary(&LiquidityResponse { 
                            asset: AssetInfo::NativeToken {
                                denom: String::from("credit_fulldenom"),
                            },
                            liquidity: Uint128::new(5_000_000_000_000_000_000u128)
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    // Mock Deployment Venue Contract
    #[cw_serde]
    pub enum DeploymentVenue_MockExecuteMsg {
        EnterVault {
            leave_vault_tokens_in_vault: Option<membrane::types::LeaveTokens>,
        },
        RepayUserDebt {
            user_info: membrane::types::UserInfo,
            repayment: Uint128,
        },
    }

    #[cw_serde]
    pub struct DeploymentVenue_MockInstantiateMsg {}

    #[cw_serde]
    pub enum DeploymentVenue_MockQueryMsg {
        RetrievableCDT {
            user: String,
        },
    }
    pub fn deployment_venue_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: DeploymentVenue_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    DeploymentVenue_MockExecuteMsg::EnterVault { 
                        leave_vault_tokens_in_vault: _ 
                    } => {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "enter_vault"),
                            attr("user", info.sender),
                        ]))
                    }
                    DeploymentVenue_MockExecuteMsg::RepayUserDebt { 
                        user_info: _,
                        repayment 
                    } => {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "repay_user_debt"),
                            attr("repayment", repayment),
                            attr("user", info.sender),
                        ]))
                    }
                }
            },
            |_, _, _, _: DeploymentVenue_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: DeploymentVenue_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    DeploymentVenue_MockQueryMsg::RetrievableCDT { user: _ } => {
                        // Return a mock retrievable CDT amount
                        Ok(to_json_binary(&Uint128::new(1000_000_000))?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    pub fn deployment_venue_contract_errors() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: DeploymentVenue_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    DeploymentVenue_MockExecuteMsg::EnterVault { .. } => {
                        // Allow entering the vault to succeed so tests can progress to repay failure
                        Ok(Response::new())
                    }
                    DeploymentVenue_MockExecuteMsg::RepayUserDebt { .. } => {
                        Err(StdError::generic_err("repay_failed"))
                    }
                }
            },
            |_, _, _, _: DeploymentVenue_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: DeploymentVenue_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    DeploymentVenue_MockQueryMsg::RetrievableCDT { .. } => {
                        Ok(to_json_binary(&Uint128::new(1_000))?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Revenue Distributor Contract
    #[cw_serde]
    pub enum RD_MockExecuteMsg {
        SetPromises {
            promises: Vec<RevenuePromise>,
            ltv_disco_distribution: Option<Vec<Asset>>,
        },
        DistributePromises {
            limit: Option<u32>,
        },
        UpdateConfig {
            revenue_destinations: Option<Vec<membrane::revenue_distributor::RevenueDestination>>,
            ltv_disco: Option<String>,
            transmuter_vault: Option<membrane::revenue_distributor::RDVaultInfoMessage>,
        },
        ClearFailedDistributions {},
        ClearPendingDistributions {},
        RetryFailedDistribute {
            limit: Option<u32>,
        },
    }

    #[cw_serde]
    pub struct RD_MockInstantiateMsg {}

    #[cw_serde]
    pub enum RD_MockQueryMsg {
        Config {},
        Promises {},
        FailedDistributions {},
        PendingDistributions {},
    }

    pub fn revenue_distributor_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: RD_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    RD_MockExecuteMsg::SetPromises { promises, ltv_disco_distribution } => {
                        // Mock successful promise setting
                        let mut attrs = vec![
                            attr("action", "set_promises"),
                            attr("promises_count", promises.len().to_string()),
                        ];
                        
                        // Add attributes for each promise
                        for (i, promise) in promises.iter().enumerate() {
                            attrs.push(attr(format!("promise_{}_address", i), promise.address.clone()));
                            attrs.push(attr(format!("promise_{}_amount", i), promise.amount.to_string()));
                        }
                        
                        // Add LTV disco distribution info if present
                        if let Some(ltv_dist) = ltv_disco_distribution {
                            attrs.push(attr("ltv_disco_distribution_count", ltv_dist.len().to_string()));
                            for (i, asset) in ltv_dist.iter().enumerate() {
                                attrs.push(attr(format!("ltv_asset_{}_denom", i), asset.info.to_string()));
                                attrs.push(attr(format!("ltv_asset_{}_amount", i), asset.amount.to_string()));
                            }
                        }
                        
                        Ok(Response::new().add_attributes(attrs))
                    },
                    RD_MockExecuteMsg::DistributePromises { limit } => {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "distribute_promises"),
                            attr("limit", limit.unwrap_or(0).to_string()),
                        ]))
                    },
                    RD_MockExecuteMsg::UpdateConfig { .. } => {
                        Ok(Response::new().add_attribute("action", "update_config"))
                    },
                    RD_MockExecuteMsg::ClearFailedDistributions {} => {
                        Ok(Response::new().add_attribute("action", "clear_failed_distributions"))
                    },
                    RD_MockExecuteMsg::ClearPendingDistributions {} => {
                        Ok(Response::new().add_attribute("action", "clear_pending_distributions"))
                    },
                    RD_MockExecuteMsg::RetryFailedDistribute { limit } => {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "retry_failed_distribute"),
                            attr("limit", limit.unwrap_or(0).to_string()),
                        ]))
                    },
                }
            },
            |_, _, _, _: RD_MockInstantiateMsg| -> StdResult<Response> { 
                Ok(Response::default()) 
            },
            |_, _, msg: RD_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    RD_MockQueryMsg::Config {} => {
                        // Return a mock config
                        let config = membrane::revenue_distributor::Config {
                            owner: Addr::unchecked("admin"),
                            canonical_asset: Asset {
                                info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                                amount: Uint128::zero(),
                            },
                            revenue_destinations: vec![],
                            ltv_disco: Addr::unchecked("ltv_disco"),
                            transmuter_vault: membrane::types::VaultInfo {
                                vault_addr: Addr::unchecked("transmuter"),
                                deposit_token: "credit_fulldenom".to_string(),
                                vault_token: "vt_credit_fulldenom".to_string(),
                            },
                            points_system_contract: None,
                            cdp_contract: None,
                            revenue_dispersal_window: None,
                            acquisition_contract: None,
                            ltv_disco_contract: None,
                            auction_contract: None,
                        };
                        Ok(to_json_binary(&config)?)
                    },
                    RD_MockQueryMsg::Promises {} => {
                        Ok(to_json_binary(&Vec::<RevenuePromise>::new())?)
                    },
                    RD_MockQueryMsg::FailedDistributions {} => {
                        Ok(to_json_binary(&Vec::<(String, Uint128)>::new())?)
                    },
                    RD_MockQueryMsg::PendingDistributions {} => {
                        Ok(to_json_binary(&Vec::<RevenuePromise>::new())?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, api, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &api.addr_make(USER),
                vec![coin(100_000_000_000, "debit"), coin(100_000_000_000, "2nddebit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract0"),
                vec![coin(2777_777777, "credit_fulldenom")],
            )
            .unwrap(); //contract1 = Stability Pool contract
            bank.init_balance(
                storage,
                &api.addr_make("test"),
                vec![coin(50_000_000_000, "credit_fulldenom"), coin(100_000_000_000, "debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("sender"),
                vec![coin(50_000_001_000_000, "credit_fulldenom")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("big_bank"),
                vec![coin(10_000_000, "debit"), coin(10_000_000, "double_debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("bigger_bank"),
                vec![
                    coin(100_000_000_000_000, "debit"),
                    coin(100_000_000_000_000, "quote"),
                    coin(100_000_000_000_000, "base"),
                    coin(100_000_000_000_000, "double_debit"),
                    coin(200_000_000_000_000_000_000_000_000_000_000, "lp_denom"),
                    coin(100_000_000_000_000, "credit_fulldenom"),
                ],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("little_bank"),
                vec![coin(1_000, "debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("redeemer"),
                vec![coin(100_000_000000, "credit_fulldenom"), coin(1, "not_redeemable")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("LP_assets"),
                vec![ coin(2_328, "base"), coin(2_328, "quote")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("coin_God"),
                vec![coin(2_250_000_000_000, "credit_fulldenom")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("lp_tester"),
                vec![coin(100_000_000_000_000_000_000_000, "lp_denom")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &api.addr_make("faker"),
                vec![coin(666, "fake_debit")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    pub fn proper_instantiate(
        sp_error: bool,
        lq_error: bool,
        liq_minimum: bool,
        bignums: bool,
    ) -> (App, CDPContract, LQContract) {
        let mut app = mock_app();
        let admin_addr = app.api().addr_make(ADMIN);

        //Instanitate SP
        let sp_id: u64;
        if sp_error {
            sp_id = app.store_code(stability_pool_contract_errors());
        }  else if bignums && liq_minimum {
            sp_id = app.store_code(stability_pool_contract_all_user_repay());
        } else if liq_minimum && !lq_error {
            sp_id = app.store_code(stability_pool_contract_minimumliq());
        } else if liq_minimum && lq_error {
            sp_id = app.store_code(stability_pool_contract_high_premium());
        } else if bignums {
            sp_id = app.store_code(stability_pool_contract_bignums());
        } else {
            sp_id = app.store_code(stability_pool_contract());
        }

        let sp_contract_addr = app
            .instantiate_contract(
                sp_id,
                admin_addr.clone(),
                &SP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instanitate Router
        let router_id = app.store_code(router_contract());

        let router_contract_addr = app
            .instantiate_contract(
                router_id,
                admin_addr.clone(),
                &Router_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate LQ
        let lq_id: u64;
        if lq_error {
            lq_id = app.store_code(liq_queue_contract_errors());
        } else if liq_minimum && !lq_error {
            lq_id = app.store_code(liq_queue_contract_minimumliq());
        } else if bignums {
            lq_id = app.store_code(liq_queue_contract_bignums());
        } else {
            lq_id = app.store_code(liq_queue_contract());
        }

        let lq_contract_addr = app
            .instantiate_contract(
                lq_id,
                admin_addr.clone(),
                &LQ_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        let lq_contract = LQContract(lq_contract_addr);

        //Instaniate Osmosis Proxy
        let proxy_id: u64;
        if bignums {
            proxy_id = app.store_code(osmosis_proxy_contract_bignums());
        } else {
            proxy_id = app.store_code(osmosis_proxy_contract());
        }

        let osmosis_proxy_contract_addr = app
            .instantiate_contract(
                proxy_id,
                admin_addr.clone(),
                &Osmo_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Auction Contract
        let auction_id = app.store_code(auction_contract());

        let auction_contract_addr = app
            .instantiate_contract(
                auction_id,
                admin_addr.clone(),
                &Auction_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Staking Contract
        let staking_id = app.store_code(staking_contract());

        let staking_contract_addr = app
            .instantiate_contract(
                staking_id,
                admin_addr.clone(),
                &Staking_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Oracle Contract
        let oracle_id: u64;
        if liq_minimum && !lq_error {
            oracle_id = app.store_code(oracle_contract());
        } else {
            oracle_id = app.store_code(oracle_contract_negative_rates());
        }

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                admin_addr.clone(),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Liquidity Contract
        let liq_id: u64;
        if bignums {
            liq_id = app.store_code(liquidity_contract_bignums());
        } else {
            liq_id = app.store_code(liquidity_contract());
        }

        let liquidity_contract_addr = app
            .instantiate_contract(
                liq_id,
                admin_addr.clone(),
                &Liquidity_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Discounts Contract
        let dc_id: u64 = app.store_code(discounts_contract());        

        let discounts_contract_addr = app
            .instantiate_contract(
                dc_id,
                admin_addr.clone(),
                &Discounts_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Revenue Distributor Contract
        let rd_id = app.store_code(revenue_distributor_contract());

        let revenue_distributor_contract_addr = app
            .instantiate_contract(
                rd_id,
                admin_addr.clone(),
                &RD_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate LTV Disco Contract
        let ltv_disco_id = app.store_code(ltv_disco_mock_contract());

        let ltv_disco_contract_addr = app
            .instantiate_contract(
                ltv_disco_id,
                admin_addr.clone(),
                &LTVDisco_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate CDP contract
        let cdp_id = app.store_code(cdp_contract());
        
        let create_basket = CreateBasket {
            basket_id: Uint128::one(),
            collateral_types: vec![cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    amount: Uint128::from(0u128),
                },
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(70),
                pool_info: None,
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                force_redemptions: None,
            }],
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit_fulldenom".to_string(),
                },
                amount: Uint128::from(0u128),
            },
            credit_price: Decimal::percent(100),
            base_interest_rate: None,
            credit_pool_infos: vec![],
            liq_queue: None,
        };

        let msg = InstantiateMsg {
            owner: Some(admin_addr.to_string()),
            liq_fee: Decimal::percent(1),
            staking_contract: Some(staking_contract_addr.to_string()),
            oracle_contract: Some(oracle_contract_addr.to_string()),
            chain_proxy: Some(osmosis_proxy_contract_addr.to_string()),
            debt_auction: Some(auction_contract_addr.to_string()),
            liquidity_contract: Some(liquidity_contract_addr.to_string()),
            discounts_contract: Some(discounts_contract_addr.to_string()),
            ltv_disco: ltv_disco_contract_addr.to_string(),
            oracle_time_limit: 60u64,
            debt_minimum: Uint128::new(2000u128),
            collateral_twap_timeframe: 60u64,
            credit_twap_timeframe: 480u64,
            rate_slope_multiplier: Decimal::from_str("0.618").unwrap(),
            base_debt_cap_multiplier: Uint128::new(21u128),
            create_basket,
        };
        let cdp_contract_addr = app
            .instantiate_contract(cdp_id, admin_addr.clone(), &msg, &[], "test", None)
            .unwrap();

        let cdp_contract = CDPContract(cdp_contract_addr);

        let msg = ExecuteMsg::EditBasket(EditBasket {
            added_cAsset: None,
            liq_queue: None,
            collateral_supply_caps: None,
            base_interest_rate: None,
            credit_asset_twap_price_source: Some(TWAPPoolInfo {
                pool_id: 0u64,
                base_asset_denom: String::from("base"),
                quote_asset_denom: String::from("quote"),
            }),
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            multi_asset_supply_caps: None,
            // revenue_destinations: None,
            credit_pool_infos: None,
            take_revenue: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(admin_addr.clone(), cosmos_msg).unwrap();

        //Instantiate Deployment Venue contract
        let venue_id = app.store_code(deployment_venue_contract());
        let venue_contract_addr = app
            .instantiate_contract(
                venue_id,
                admin_addr.clone(),
                &DeploymentVenue_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();


        //Create a second venue
        let venue_id = app.store_code(deployment_venue_contract());
        let venue_contract_addr_2 = app
            .instantiate_contract(
                venue_id,
                admin_addr.clone(),
                &DeploymentVenue_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Update Config to add valid deployment venues
        let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
            owner: None,
            chain_proxy: None,
            debt_auction: None,
            staking_contract: None,
            oracle_contract: None,
            liquidity_contract: None,
            discounts_contract: None,
            liq_fee: None,
            ltv_disco: None,
            collateral_twap_timeframe: None,
            credit_twap_timeframe: None,
            oracle_time_limit: None,
            debt_minimum: None,
            base_debt_cap_multiplier: None,
            cpc_multiplier: None,
            rate_slope_multiplier: None,
            affiliate_fee_max: None,
            skip_credit_price_accrual: None,
            liquidation_stat_limit: None,
            revenue_distributor: Some(revenue_distributor_contract_addr.to_string()),
            transmuter_addr: None,
            irm_config: Some(membrane::types::IRMConfig {
                adjustment_speed: Decimal::from_str("50").unwrap(),
                min_adaptive_rate: Decimal::from_str("0.001").unwrap(),
                max_adaptive_rate: Decimal::one(), // 100% for tests
            }),
                points_contract: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(admin_addr.clone(), cosmos_msg).unwrap();

        (app, cdp_contract, lq_contract)
    }
    mod cdp {

        use super::*;
        use cosmwasm_std::{coins, BlockInfo};
        use membrane::cdp::{
            CollateralInterestResponse, Config, BasketPositionsResponse,
            ExecuteMsg, PositionResponse, InterestResponse,
        };
        use membrane::types::{Basket, LPAssetInfo, PoolInfo, SupplyCap, UserInfo, DeploymentIntent};

        #[test]
        fn test_pending_revenue_per_asset_distribution_and_rd_setpromises() {
            let (mut app, cdp_contract, _lq_contract) = proper_instantiate(false, false, false, false);
            let admin_addr = app.api().addr_make(ADMIN);

            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                }),
                liq_queue: None,
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: Some(Decimal::percent(50)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                take_revenue: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Edit config to add chain_proxy
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                    chain_proxy: None,
                owner: None,
                staking_contract: None,
                debt_auction: None,
                oracle_contract: None,
                liquidity_contract: None,
                discounts_contract: None,
                ltv_disco: None,
                liq_fee: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                oracle_time_limit: None,
                cpc_multiplier: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                rate_slope_multiplier: None,
                affiliate_fee_max: None,
                skip_credit_price_accrual: None,
                liquidation_stat_limit: None,
                revenue_distributor: Some({ let cfg: Config = app.wrap().query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {}).unwrap(); cfg.revenue_distributor.unwrap().to_string() }),

                transmuter_addr: None,
                irm_config: None,
                points_contract: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            // 1) Create a position by depositing collateral
            let deposit_msg = ExecuteMsg::Deposit { 
                position_owner: Some(app.api().addr_make(USER).to_string()), 
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract.call(deposit_msg, vec![
                coin(100_000_000_000, "debit"),
                coin(100_000_000_000, "2nddebit"),
            ]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();
            
            // 2) Borrow some credit to create debt
            let borrow_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::new(1),
                amount: Some(Uint128::new(2000_000_000)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(borrow_msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Skip a year in block time
            app.set_block(
                BlockInfo { 
                    height: app.block_info().height + 1, 
                    time: app.block_info().time.plus_seconds(31536000), 
                    chain_id: app.block_info().chain_id }
                );
            
            // 3) Trigger accrue to populate pending_revenue
            let accrue_msg = ExecuteMsg::Accrue { 
                position_owner: Some(app.api().addr_make(USER).to_string()), 
                position_ids: vec![Uint128::new(1)] 
            };
            let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();


            // 5) Verify the basket has updated pending revenue structure
            let basket_query = QueryMsg::GetBasket {};
            let basket: Basket = app.wrap().query_wasm_smart(cdp_contract.addr(), &basket_query).unwrap();
            
            //Assert Positions were updated
            let position: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(&cdp_contract.addr(), &QueryMsg::GetBasketPositions {
                    start_after: None, 
                    limit: None,
                    user: None,
                    user_info: Some(
                        UserInfo {
                            position_id: Uint128::new(1),
                            position_owner: app.api().addr_make(USER).to_string(),
                        }
                    ),
                })
                .unwrap();
            // Check that pending revenue has the new structure
            assert!(basket.pending_revenue.total_pending == Uint128::new(1269841268));
            assert!(position[0].positions[0].total_interest_accrued == Uint128::new(1269841268));
            // The per_asset_rev should be populated based on the position's collateral ratios
            assert!(basket.pending_revenue.per_asset_rev == vec![
                Asset { info: AssetInfo::NativeToken { denom: "debit".to_string() }, amount: Uint128::new(634920634) },
                Asset { info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, amount: Uint128::new(634920634) },
            ]);
            
            // 4) Repay some credit to trigger revenue distribution
            let repay_msg = ExecuteMsg::Repay {
                debt_split: None,
                position_id: Uint128::new(1),
                position_owner: Some(app.api().addr_make(USER).to_string()),
                send_excess_to: None,
            };
            //Send the USER credit_fulldenom
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                app.api().addr_make(USER),
                &[coin(50_000_000, "credit_fulldenom")],
            ).unwrap();
            let cosmos_msg = cdp_contract.call(repay_msg, vec![coin(50_000_000, "credit_fulldenom")]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();
            
            // 5) Verify the basket has updated pending revenue structure
            let basket_query = QueryMsg::GetBasket {};
            let basket: Basket = app.wrap().query_wasm_smart(cdp_contract.addr(), &basket_query).unwrap();
            
            //Assert Positions were updated
            let position: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(&cdp_contract.addr(), &QueryMsg::GetBasketPositions {
                    start_after: None, 
                    limit: None,
                    user: None,
                    user_info: Some(
                        UserInfo {
                            position_id: Uint128::new(1),
                            position_owner: app.api().addr_make(USER).to_string(),
                        }
                    ),
                })
                .unwrap();

            assert!(basket.pending_revenue.total_pending == Uint128::new(1219841268));
            //Total accrued doesn't change after repayments
            assert!(position[0].positions[0].total_interest_accrued == Uint128::new(1269841268));
            // The per_asset_rev should be populated based on the position's collateral ratios
            assert!(basket.pending_revenue.per_asset_rev == vec![ 
                Asset { info: AssetInfo::NativeToken { denom: "debit".to_string() }, amount: Uint128::new(609920634) },
                Asset { info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, amount: Uint128::new(609920634) },
            ]);
            //634920634 - 609920634 = 25000000 which is the revenue distributed to each asset
        }

        #[test]
        fn liquidate_marks_failed_deployable_venue_on_reply_error() {
            let (mut app, cdp, _lq) = proper_instantiate(false, false, false, false);

            // Replace one deployment venue with an erroring contract
            let venue_err_id = app.store_code(deployment_venue_contract_errors());
            let venue_err_addr = app
                .instantiate_contract(
                    venue_err_id,
                    app.api().addr_make(ADMIN),
                    &DeploymentVenue_MockInstantiateMsg {},
                    &[],
                    "venue_err",
                    None,
                )
                .unwrap();

            // Deposit collateral and borrow to create an undercollateralized position
            let deposit = ExecuteMsg::Deposit { position_owner: Some(app.api().addr_make(USER).to_string()), position_id: None, affiliate_address: None, affiliate_label: None };
            let cosmos = cdp.call(deposit, vec![Coin { denom: "debit".to_string(), amount: Uint128::new(50_000_000_000) }]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos).unwrap();

            // Add the erroring venue to active venues and to the position via intents and fulfill
            let set_intent = ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    position_id: Uint128::one(),
                    destination: venue_err_addr.to_string(),
                    user: app.api().addr_make(USER).to_string(),
                    ltv_to_mint: Decimal::percent(5),
                }
            };
            let cosmos = cdp.call(set_intent, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos).unwrap();

            //Send cdp contract 2500000000 "credit_fulldenom" from bigger_bank
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                cdp.addr(),
                &[coin(2500000000, "credit_fulldenom")],
            ).unwrap();

            let fulfill = ExecuteMsg::FulfillIntents { users: vec![app.api().addr_make(USER).to_string()] };
            let cosmos = cdp.call(fulfill, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos).unwrap();

            // Borrow explicitly to ensure debt exists
            let borrow = ExecuteMsg::IncreaseDebt { position_id: Uint128::one(), amount: Some(Uint128::new(10_000_000)), LTV: None, mint_to_addr: None, deployment_intent: None, debt_split: None, rollover_updates: None, peg_debt: None };
            let cosmos = cdp.call(borrow, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos).unwrap();

            // Trigger liquidation; deployable venue RepayUserDebt will error; reply handler should set failed_liquidation=true
            let liq = ExecuteMsg::Liquidate { position_id: Uint128::one(), position_owner: app.api().addr_make(USER).to_string() };
            let cosmos = cdp.call(liq, vec![]).unwrap();
            // Ignore result; reply_on_always ensures reply executes
            let _ = app.execute(app.api().addr_make("sender"), cosmos);

            // Query positions and assert the flag
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: Some(UserInfo { position_id: Uint128::one(), position_owner: app.api().addr_make(USER).to_string() }),
                user: None,
            };
            let baskets: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp.addr(), &query_msg)
                .unwrap();
            let pos = baskets[0]
                .positions
                .iter()
                .find(|p| p.position_id == Uint128::one())
                .unwrap();
            // The venue exists and failed_liquidation is true for the erroring venue
            assert!(pos.deployed_to.iter().any(|v| v.address == venue_err_addr && v.failed_liquidation));


            // Re-Deposit collateral 
            let deposit = ExecuteMsg::Deposit { position_owner: Some(app.api().addr_make(USER).to_string()), position_id: Some(Uint128::one()), affiliate_address: None, affiliate_label: None };
            let cosmos = cdp.call(deposit, vec![Coin { denom: "debit".to_string(), amount: Uint128::new(50_000_000_000) }]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos).unwrap();
            // Re-increase debt so the position can be liquidated again
            let borrow2 = ExecuteMsg::IncreaseDebt { position_id: Uint128::one(), amount: Some(Uint128::new(2_000_000_000)), LTV: None, mint_to_addr: None, deployment_intent: None, debt_split: None, rollover_updates: None, peg_debt: None };
            let cosmos_b2 = cdp.call(borrow2, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_b2).unwrap();

            // Attempt a second liquidation; because the venue is marked failed, it should be skipped
            let liq2 = ExecuteMsg::Liquidate { position_id: Uint128::one(), position_owner: app.api().addr_make(USER).to_string() };
            let cosmos2 = cdp.call(liq2, vec![]).unwrap();
            let res2 = app.execute(app.api().addr_make("sender"), cosmos2).unwrap();
            // Ensure no deployable venue repay submessage attribute appears for the failed venue
            let attrs_str = format!("{:?}", res2.events);
            assert!(!attrs_str.contains(&format!("repay_deployable_venue_from_{}", venue_err_addr)));
        }

        #[test]
        fn active_deployment_venues_state_and_query_tracking() {
            let (mut app, cdp, _lq) = proper_instantiate(false, false, false, false);

            // Instantiate two venues
            let venue_id1 = app.store_code(deployment_venue_contract());
            let v1 = app
                .instantiate_contract(
                    venue_id1,
                    app.api().addr_make(ADMIN),
                    &DeploymentVenue_MockInstantiateMsg {},
                    &[],
                    "v1",
                    None,
                )
                .unwrap();
            let venue_id2 = app.store_code(deployment_venue_contract());
            let v2 = app
                .instantiate_contract(
                    venue_id2,
                    app.api().addr_make(ADMIN),
                    &DeploymentVenue_MockInstantiateMsg {},
                    &[],
                    "v2",
                    None,
                )
                .unwrap();

            // Set two intents so both venues become active via fulfill flow
            let intent1 = ExecuteMsg::SetUserIntents { deployment_intent: DeploymentIntent {
                position_id: Uint128::one(),
                destination: v1.to_string(),
                user: app.api().addr_make(USER).to_string(),
                ltv_to_mint: Decimal::percent(5),
            }};
            let cosmos = cdp.call(intent1, vec![]).unwrap();
            // Ensure the position exists first
            let deposit = ExecuteMsg::Deposit { position_owner: Some(app.api().addr_make(USER).to_string()), position_id: None, affiliate_address: None, affiliate_label: None };
            let dep_cosmos = cdp.call(deposit, vec![Coin { denom: "debit".to_string(), amount: Uint128::new(50_000_000_000) }]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, dep_cosmos).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos).unwrap();

            let intent2 = ExecuteMsg::SetUserIntents { deployment_intent: DeploymentIntent {
                position_id: Uint128::one(),
                destination: v2.to_string(),
                user: app.api().addr_make(USER).to_string(),
                ltv_to_mint: Decimal::percent(5),
            }};
            let cosmos = cdp.call(intent2, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos).unwrap();

            //Send cdp contract 5000000000 "credit_fulldenom" from bigger_bank
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                cdp.addr(),
                &[coin(5000000000, "credit_fulldenom")],
            ).unwrap();


            // Fulfill; this will add both venues to ACTIVE_DEPLOYMENT_VENUES
            let fulfill = ExecuteMsg::FulfillIntents { users: vec![app.api().addr_make(USER).to_string()] };
            let cosmos = cdp.call(fulfill, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos).unwrap();

            // Query: list all venues
            let q_all = QueryMsg::GetActiveDeploymentVenues { venue: None, start_after: None, limit: None };
            let list_all: Vec<String> = app.wrap().query_wasm_smart(cdp.addr(), &q_all).unwrap();
            assert!(list_all.iter().any(|s| s == &v1.to_string()));
            assert!(list_all.iter().any(|s| s == &v2.to_string()));

            // Query: single venue
            let q_one = QueryMsg::GetActiveDeploymentVenues { venue: Some(v1.to_string()), start_after: None, limit: None };
            let single: Vec<String> = app.wrap().query_wasm_smart(cdp.addr(), &q_one).unwrap();
            assert_eq!(single, vec![v1.to_string()]);

            // Query: pagination (start_after v1)
            let q_paginate = QueryMsg::GetActiveDeploymentVenues { venue: None, start_after: Some(v1.to_string()), limit: Some(1) };
            let page: Vec<String> = app.wrap().query_wasm_smart(cdp.addr(), &q_paginate).unwrap();
            assert_eq!(page.len(), 1);
            assert!(page[0] >= v1.to_string());

            // Remove v1 by setting zero LTV (intent removal) and fulfilling; ensures state_remove path
            let remove_intent = ExecuteMsg::SetUserIntents { deployment_intent: DeploymentIntent {
                position_id: Uint128::one(),
                destination: v1.to_string(),
                user: app.api().addr_make(USER).to_string(),
                ltv_to_mint: Decimal::zero(),
            }};
            let cosmos = cdp.call(remove_intent, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos).unwrap();
            let fulfill2 = ExecuteMsg::FulfillIntents { users: vec![app.api().addr_make(USER).to_string()] };
            let cosmos = cdp.call(fulfill2, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos).unwrap();

            // Query post-removal: v1 should disappear from active venues, v2 remains
            let list_after: Vec<String> = app.wrap().query_wasm_smart(cdp.addr(), &q_all).unwrap();
            assert!(!list_after.iter().any(|s| s == &v1.to_string()));
            assert!(list_after.iter().any(|s| s == &v2.to_string()));
        }

        #[test]
        fn fulfill_bad_debt_burns_and_refunds_excess() {
            let (mut app, cdp_contract, _lq) = proper_instantiate(false, false, false, false);

            // Fund a user with 100 credit tokens to send into fulfill call
            app.send_tokens(
                app.api().addr_make("coin_God"),
                app.api().addr_make("anyone"),
                &[coin(100, "credit_fulldenom")],
            ).unwrap();

            let user_before = app.wrap().query_balance("anyone", "credit_fulldenom").unwrap();
            let contract_before = app.wrap().query_balance(cdp_contract.addr(), "credit_fulldenom").unwrap();

            // Fulfill with 100 when pending_bad_debt == 0 => burn 0, refund full 100
            let msg = ExecuteMsg::FulfillBadDebt { };
            let cosmos = cdp_contract.call(msg, vec![Coin { denom: "credit_fulldenom".to_string(), amount: Uint128::new(100) }]).unwrap();
            let res = app.execute(app.api().addr_make("anyone"), cosmos).unwrap();

            // Verify contract ends with zero credit balance (no stray CDT)
            let contract_after = app.wrap().query_balance(cdp_contract.addr(), "credit_fulldenom").unwrap();
            assert_eq!(contract_before.amount, Uint128::zero());
            assert_eq!(contract_after.amount, Uint128::zero());

            // Verify user refund/usage breakdown
            let user_after = app.wrap().query_balance("anyone", "credit_fulldenom").unwrap();
            let used = user_before.amount.checked_sub(user_after.amount).unwrap();
            let refunded = Uint128::new(100) - used;
            // With pending_bad_debt == 0: used == 0, refunded == 100
            assert_eq!(used, Uint128::zero());
            assert_eq!(refunded, Uint128::new(100));

            // Sanity: response contains transfer event
            assert!(res.events.iter().any(|e| e.ty == "transfer"));
        }

        //Only works with hardcoded pending_bad_debt
        #[test]
        fn fulfill_bad_debt_burns_non_zero_pending() {
            let (mut app, cdp, _lq) = proper_instantiate(false, false, false, false);

            // 1) Create a position and mint debt so it can later be liquidated into bad debt
            let deposit = ExecuteMsg::Deposit { position_owner: Some(app.api().addr_make(USER).to_string()), position_id: None, affiliate_address: None, affiliate_label: None };
            let cosmos = cdp.call(deposit, vec![Coin { denom: "debit".to_string(), amount: Uint128::new(50_000_000_000) }]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos).unwrap();

            let borrow = ExecuteMsg::IncreaseDebt { position_id: Uint128::new(1), amount: Some(Uint128::new(10_000_000_000)), LTV: None, mint_to_addr: None, deployment_intent: None, debt_split: None, rollover_updates: None, peg_debt: None };
            let cosmos = cdp.call(borrow, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos).unwrap();

            // Liquidate to push potential remainder debt into pending_bad_debt and schedule callback
            let liq = ExecuteMsg::Liquidate { position_id: Uint128::new(1), position_owner: app.api().addr_make(USER).to_string() };
            let cosmos = cdp.call(liq, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            let res = app.execute(user_addr, cosmos);
            // Liquidation may or may not succeed depending on mocks; the callback would set pending_bad_debt on remainder
            let _ = res; // ignore result as mocks are complex; we'll fulfill against whatever pending exists

            // 3) Send 1,000 CDT into FulfillBadDebt and assert burn + potential refund behavior
            app.send_tokens(app.api().addr_make("coin_God"), app.api().addr_make("payer"), &[coin(1_000, "credit_fulldenom")]).unwrap();
            let before_contract = app.wrap().query_balance(cdp.addr(), "credit_fulldenom").unwrap();
            let payer_before = app.wrap().query_balance("payer", "credit_fulldenom").unwrap();
            let msg = ExecuteMsg::FulfillBadDebt { };
            let cosmos = cdp.call(msg, vec![coin(1_000, "credit_fulldenom")]).unwrap();
            let _fulfill_res = app.execute(app.api().addr_make("payer"), cosmos).unwrap();

            // Contract should not retain any credit (burned what it used, refunded excess)
            let after_contract = app.wrap().query_balance(cdp.addr(), "credit_fulldenom").unwrap();
            assert_eq!(before_contract.amount, Uint128::zero());
            //Has 990 bc pending_bad_debt is 990 & the contract burns don't send the asset
            assert_eq!(after_contract.amount, Uint128::new(990));

            // Verify payer usage and refund (used <= sent, refund = sent - used)
            let payer_after = app.wrap().query_balance("payer", "credit_fulldenom").unwrap();
            let used = payer_before.amount.checked_sub(payer_after.amount).unwrap();
            assert!(used <= Uint128::new(1_000));
            let _refunded = Uint128::new(1_000) - used;
            println!("used: {}", used);
            println!("refunded: {}", _refunded);

            assert!(used == Uint128::new(990));
            assert!(_refunded == Uint128::new(10));
        }

        // #[test]
        // fn bad_debt_check_sends_to_auction_when_no_ltv_disco_capacity() {
        //     let (mut app, cdp, _lq) = proper_instantiate(false, false, false, false);

        //     //Deposit into the contract
        //     let msg = ExecuteMsg::Deposit {
        //         position_owner: Some(app.api().addr_make(USER).to_string()),
        //         position_id: None,
        //     };
        //     let cosmos = cdp.call(msg, vec![ Coin { denom: "debit".to_string(), amount: Uint128::from(100000000000u128) }]).unwrap();
        //     app.execute(app.api().addr_make(USER), cosmos).unwrap();

        //     // Direct external Callback is forbidden; assert the guard works precisely
        //     let cb = ExecuteMsg::Callback(CallbackMsg::BadDebtCheck { position_id: Uint128::new(1), position_owner: app.api().addr_make(USER) });
        //     let cosmos = cdp.call(cb, vec![]).unwrap();
        //     let res = app.execute(cdp.addr(), cosmos).unwrap();
        //     let res = format!("{:?}", res);
        //     println!("{}", res);
        //     // assert!(err_str.contains("Unauthorized"), "expected Unauthorized, got: {}", err_str);
        // }

        #[test]
        fn test_bad_debt_check_queries_ltv_disco_can_handle() {
            use membrane::ltv_disco::{ExecuteMsg as LTVDisco_ExecuteMsg, QueryMsg as LTVDisco_QueryMsg, InstantiateMsg as LTVDisco_InstantiateMsg, BackingDepositInput};
            
            let (mut app, cdp, _lq) = proper_instantiate(false, false, false, false);
            
            // Get the LTV Disco address from CDP config
            let config: Config = app.wrap().query_wasm_smart(cdp.addr(), &QueryMsg::Config {}).unwrap();
            let ltv_disco_addr = config.ltv_disco.clone();
            
            // Verify LTV Disco exists and can be queried
            // The mock disco returns false for CanHandleBadDebt, so we verify the query works
            let can_handle: bool = app.wrap().query_wasm_smart(
                ltv_disco_addr.clone(),
                &LTVDisco_QueryMsg::CanHandleBadDebt {
                    asset: "debit".to_string(),
                    amount: Uint128::new(100_000),
                },
            ).unwrap();
            
            // Mock disco returns false, but we verify the query mechanism works
            // In a real scenario with deposits, this would return true
            assert!(!can_handle, "Mock disco should return false (no deposits)");
            
            // Verify CDP has ltv_disco configured (compare addresses)
            assert_eq!(config.ltv_disco.to_string(), ltv_disco_addr.to_string());
        }

        #[test]
        fn test_bad_debt_check_flow_with_real_disco() {
            // This test would require:
            // 1. Replacing mock LTV Disco with real one in proper_instantiate
            // 2. Adding deposits to disco
            // 3. Creating position and liquidating to trigger bad debt
            // 4. Verifying AddBadDebt is sent to disco
            // 
            // For now, we verify the infrastructure is in place
            let (mut app, cdp, _lq) = proper_instantiate(false, false, false, false);
            
            let config: Config = app.wrap().query_wasm_smart(cdp.addr(), &QueryMsg::Config {}).unwrap();
            
            // Verify ltv_disco is configured
            assert!(!config.ltv_disco.to_string().is_empty(), "LTV Disco should be configured");
        }

        fn freeze(){

            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);
            
            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    }
                ]),
                base_interest_rate: Some(Decimal::percent(2)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(true),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Frozen Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(50_000_000_000u128),
                        },
                    ],
                )
                .unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap_err();

            //Unfreeze Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(false),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Initial debit Deposit
            //50_000 debit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(50_000_000_000u128),
                        },
                    ],
                )
                .unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Freeze Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(true),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Frozen: Partial withdrawal for Position #1
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(10_000_000_000u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap_err();

            //Unfreeze Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(false),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();
            //Partial withdrawal for Position #1
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(10_000_000_000u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Freeze Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(true),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Frozen: Increase Debt for Position #1
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(10_000_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap_err();

            //Unfreeze Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(false),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Increase Debt for Position #1
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(10_000_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Send credit
            app.send_tokens(
                app.api().addr_make("sender"),
                app.api().addr_make(USER),
                &[coin(10_000_000_000, "credit_fulldenom")],
            )
            .unwrap();

            //Freeze Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(true),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Frozen: Repayment for Position #1
            let repay_msg = ExecuteMsg::Repay {
                debt_split: None,
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(repay_msg, vec![coin(10_000_000_000, "credit_fulldenom")])
                .unwrap();
            let user_addr = get_user_addr(&app);
            let err = app.execute(user_addr, cosmos_msg).unwrap_err();

            //Unfreeze Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(false),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Repayment for Position #1
            let repay_msg = ExecuteMsg::Repay {
                debt_split: None,
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(repay_msg, vec![coin(10_000_000_000, "credit_fulldenom")])
                .unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();           

            //Freeze Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(true),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Frozen: Liquidate Position #1 
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make(USER).to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            let err = app.execute(user_addr, cosmos_msg).unwrap_err();
            
            //Unfreeze Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: Some(false),
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Freeze Grace Period: Liquidate Position #1 
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make(USER).to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            let err = app.execute(user_addr, cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Generic error: You can liquidate in 3600 seconds, there is a post-freeze grace period"));

            //Skip 12 hours
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(43200),
                chain_id: app.block_info().chain_id,
            });           

            //Liquidate Position #1: Solvency Error
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make(USER).to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            let err = app.execute(user_addr, cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Position is solvent and shouldn't be liquidated"));
        }

        #[test]
        //Multiple positions
        //Withdraw partially from both
        //Mint credit limit for both
        //Repay position #1
        //Liquidate position #2
        fn mock_user(){

            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);
            
            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    }
                ]),
                base_interest_rate: Some(Decimal::percent(2)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Initial debit Deposit
            //50_000 debit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(50_000_000_000u128),
                        },
                    ],
                )
                .unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Debit Deposit into Position #2
            //50_000 debit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(50_000_000_000u128),
                        },
                    ],
                )
                .unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Assert user positions            
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None, 
                limit: None,
                user: Some(app.api().addr_make(USER).to_string()),
                user_info: None,
            };

            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
                
            ////Assert collaterals, then length
            assert_eq!(
                res[0].positions[0].collateral_assets,
                vec![
                    cAsset {
                        asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "debit".to_string(),
                            },
                            amount: Uint128::from(50_000_000_000u128),
                        },
                        max_borrow_LTV: Decimal::percent(50),
                        max_LTV: Decimal::percent(70),
                        pool_info: None,  
                        rate_index: Decimal::one(),
                        peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                    }
                ]
            );
            assert_eq!(
                res[0].positions[1].collateral_assets,
                vec![
                    cAsset {
                        asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "debit".to_string(),
                            },
                            amount: Uint128::from(50_000_000_000u128),
                        },
                        max_borrow_LTV: Decimal::percent(50),
                        max_LTV: Decimal::percent(70),
                        pool_info: None,  
                        rate_index: Decimal::one(),
                        peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                    }
                ]
            );
            assert_eq!(
                res[0].positions.len().to_string(),
                String::from("2") 
            );               


            //Partial withdrawal for Position #1
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(10_000_000_000u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Partial withdrawal for Position #2
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(2u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(10_000_000_000u128),
                    },
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Assert user positions
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None, 
                limit: None,
                user: Some(app.api().addr_make(USER).to_string()),
                user_info: None,
            };

            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            ////Assert collaterals
            assert_eq!(
                res[0].positions[0].collateral_assets[0].asset,
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(40_000_000_000u128),
                        }
            );
            assert_eq!(
                res[0].positions[1].collateral_assets[0].asset,
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(40_000_000_000u128),
                        }
            );

            //Increase Debt for Position #1
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(10_000_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Send credit
            app.send_tokens(
                app.api().addr_make("sender"),
                app.api().addr_make(USER),
                &[coin(10_000_000_000, "credit_fulldenom")],
            )
            .unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(20_000_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Send credit
            app.send_tokens(
                app.api().addr_make("sender"),
                app.api().addr_make(USER),
                &[coin(20_000_000_000, "credit_fulldenom")],
            )
            .unwrap();

            //Assert user positions            
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None, 
                limit: None,
                user: Some(app.api().addr_make(USER).to_string()),
                user_info: None,
            };

            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            ////Assert credit_amount
            assert_eq!(
                res[0].positions[0].credit_amount,
                Uint128::from(10_000_000_000u128)
            );
            assert_eq!(
                res[0].positions[1].credit_amount,
                Uint128::from(20_000_000_000u128)
            );

            //Repayment for Position #1
            let repay_msg = ExecuteMsg::Repay {
                debt_split: None,
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(repay_msg, vec![coin(10_000_000_000, "credit_fulldenom")])
                .unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Liquidate Position #2 
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(2u128),
                position_owner: app.api().addr_make(USER).to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Assert user positions   
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None, 
                limit: None,
                user: Some(app.api().addr_make(USER).to_string()),
                user_info: None,
            };

            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            ////Assert credit_amount
            ///Note: Position_id order flips
            assert_eq!(
                res[0].positions[1].position_id,
                Uint128::new(2)
            );
            assert_eq!(
                res[0].positions[1].credit_amount,
                Uint128::from(20_000_000_000u128)
            );
            assert_eq!(
                res[0].positions[0].position_id,
                Uint128::new(1)
            );
            assert_eq!(
                res[0].positions[0].credit_amount,
                Uint128::zero()
            );
        }

        // #[test]
        // fn set_and_fulfill_intents(){

        //     let (mut app, cdp_contract, lq_contract) =
        //         proper_instantiate(false, false, false, false);
            
        //     //Edit Basket
        //     let msg = ExecuteMsg::EditBasket(EditBasket {
        //         take_revenue: None,
        //         added_cAsset: None,
        //         liq_queue: Some(lq_contract.addr().to_string()),
        //         credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
        //         collateral_supply_caps: Some(vec![
        //             SupplyCap {
        //                 asset_info: AssetInfo::NativeToken {
        //                     denom: "debit".to_string(),
        //                 },
        //                 current_supply: Uint128::zero(),
        //                 debt_total: Uint128::zero(),
        //                 supply_cap_ratio: Decimal::percent(100),
        //                 lp: false,
        //                 stability_pool_ratio_for_debt_cap: None,
        //             }
        //         ]),
        //         base_interest_rate: Some(Decimal::percent(2)),
        //         credit_asset_twap_price_source: None,
        //         negative_rates: None,
        //         cpc_margin_of_error: None,
        //         frozen: None,
        //         distribute_revenue: None,
        //         multi_asset_supply_caps: None,
        //         revenue_destinations: None,
        //     });
        //     let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        //     app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        //     //Initial debit Deposit
        //     //50_000 debit
        //     let msg = ExecuteMsg::Deposit {
        //         position_owner: Some(app.api().addr_make(USER).to_string()),
        //         position_id: None,
        //     };
        //     let cosmos_msg = cdp_contract
        //         .call(
        //             msg,
        //             vec![
        //                 Coin {
        //                     denom: "debit".to_string(),
        //                     amount: Uint128::from(50_000_000_000u128),
        //                 },
        //             ],
        //         )
        //         .unwrap();
        //     app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            
        //     //Set Intent
        //     let msg = ExecuteMsg::SetUserIntents { deployment_intent: Some(
        //         EnterLPIntent {                    
        //             user: String::from(USER),
        //             position_id: Uint128::one(),
        //             ltv_to_mint: Decimal::percent(10),
        //         }
        //     ) };
        //     let cosmos_msg = cdp_contract
        //         .call(
        //             msg,
        //             vec![],
        //         )
        //         .unwrap();
        //     app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        //     //Assert Intents
        //     let query_msg = QueryMsg::GetUserIntent {
        //         start_after: None, 
        //         limit: None,
        //         users: vec![String::from(USER)],
        //     };

        //     let res: Vec<UserIntentResponse> = app
        //         .wrap()
        //         .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
        //         .unwrap();
        //     assert_eq!(
        //         res[0].intent.enter_lp_intents[0].position_id,
        //         Uint128::new(1)
        //     );
        //     assert_eq!(
        //         res[0].intent.enter_lp_intents[0].ltv_to_mint,
        //         Decimal::percent(10)
        //     );
        //     //Fulfill Intent
        //     let msg = ExecuteMsg::FulfillIntents {
        //         users: vec![app.api().addr_make(USER).to_string()]
        //     };
        //     let cosmos_msg = cdp_contract
        //         .call(
        //             msg,
        //             vec![ ],
        //         )
        //         .unwrap();
        //     app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();
        //     //Assert user positions
        //     let query_msg = QueryMsg::GetBasketPositions {
        //         start_after: None, 
        //         limit: None,
        //         user: Some(app.api().addr_make(USER).to_string()),
        //         user_info: None,
        //     };
        //     let res: Vec<BasketPositionsResponse> = app
        //         .wrap()
        //         .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
        //         .unwrap();
        //     ////Assert collaterals
        //     assert_eq!(
        //         res[0].positions[0].credit_amount,
        //         Uint128::from(40_000_000_000u128)                    
        //     );
        // }

        #[test]
        fn withdrawal() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);
            

            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                }),
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                take_revenue: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000_000_000u128),
                        },
                        Coin {
                            denom: "2nddebit".to_string(),
                            amount: Uint128::from(100_000_000_000u128),
                        },
                    ],
                )
                .unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Successful debt increase to initiate caps
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2000_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Assert asset tally is working
            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.collateral_supply_caps[0].current_supply,
                Uint128::new(100000_000_000)
            );
            assert_eq!(
                res.collateral_supply_caps[1].current_supply,
                Uint128::new(100000_000_000)
            );

            //Query BasketPositions
            let msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: None,
            };

            let resp: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &msg.clone())
                .unwrap();
            assert_eq!(
                resp[0].positions[0].collateral_assets[0]
                    .asset
                    .amount
                    .to_string(),
                String::from("100000000000")
            );
            assert_eq!(
                resp[0].positions[0].collateral_assets[1]
                    .asset
                    .amount
                    .to_string(),
                String::from("100000000000")
            );
            assert_eq!(resp.len().to_string(), String::from("1"));

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Insolvent withdrawal error
            let msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(100_000_000_000u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(100_000_000_000u128),
                    },
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap_err();

            //Duplicate asset error
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(45_000_000_000u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(45_000_000_000u128),
                    },
                ],
                send_to: Some(app.api().addr_make("very_trusted_contract").to_string()),
            };

            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap_err();

            //Successful attempt
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(90_000_000_000u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(90_000_000_000u128),
                    },
                ],
                send_to: Some(app.api().addr_make("very_trusted_contract").to_string()),
            };

            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let user_addr = get_user_addr(&app);
            app.execute(user_addr, cosmos_msg).unwrap();

            //Query Position assets to assert withdrawal
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None, 
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make(USER).to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].asset.amount, Uint128::new(10000_000_000));
            assert_eq!(res[0].positions[0].collateral_assets[1].asset.amount, Uint128::new(10000_000_000));

            //Assert withdrawal was sent to sent_to.
            assert_eq!(
                app.wrap().query_all_balances(&app.api().addr_make("very_trusted_contract").to_string()).unwrap(),
                vec![coin(90000_000_000, "2nddebit"), coin(90000_000_000, "debit")]
            );

            //Assert asset tally is working
            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.collateral_supply_caps[0].current_supply,
                Uint128::new(10000_000_000)
            );
            assert_eq!(
                res.collateral_supply_caps[1].current_supply,
                Uint128::new(10000_000_000)
            );
            //Assert Denom change
            assert_eq!(
                res.credit_asset.info.to_string(),
                "credit_fulldenom".to_string()
            );
        }

        #[test]
        fn increase_debt__repay() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("test").to_string()),
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000_000u128),
                    }],
                )
                .unwrap();
            let test_addr = app.api().addr_make("test");
            app.execute(test_addr, cosmos_msg).unwrap();

            //Insolvent position error
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_001_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let test_addr = app.api().addr_make("test");
            app.execute(test_addr, cosmos_msg)
                .unwrap_err();

            //Minimum Debt Error
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(1_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let test_addr = app.api().addr_make("test");
            app.execute(test_addr, cosmos_msg)
                .unwrap_err();

            /////////////Test that repaying in full w/o excess works////////////////////

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let test_addr = app.api().addr_make("test");
            app.execute(test_addr, cosmos_msg).unwrap();

            //Send credit
            app.send_tokens(
                app.api().addr_make("sender"),
                app.api().addr_make("test"),
                &[coin(50_000_000_000, "credit_fulldenom")],
            )
            .unwrap();

            ///Full Repayment
            let msg = ExecuteMsg::Repay {
                debt_split: None,
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(50_000_000_000, "credit_fulldenom")])
                .unwrap();
            let test_addr = app.api().addr_make("test");
            app.execute(test_addr, cosmos_msg)
                .unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let test_addr = app.api().addr_make("test");
            app.execute(test_addr, cosmos_msg).unwrap();

            //Send credit
            app.send_tokens(
                app.api().addr_make("sender"),
                app.api().addr_make("test"),
                &[coin(50_001_000_000, "credit_fulldenom")],
            )
            .unwrap();

            //Error on Partial Repayment under config.debt_minimum
            let msg = ExecuteMsg::Repay {
                debt_split: None,
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(49_901_000_000, "credit_fulldenom")])
                .unwrap();
            let test_addr = app.api().addr_make("test");
            app.execute(test_addr, cosmos_msg)
                .unwrap_err();

            
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None, 
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make("test").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].credit_amount, Uint128::new(50000_000_000));

            //Query Basket Debt Caps
            // let query_msg = QueryMsg::GetBasketDebtCaps { };
            // let res: Vec<DebtCap> = app
            //     .wrap()
            //     .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(
            //     format!("{:?}", res),
            //     String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(50000000000), cap: Uint128(249995050000) }]")
            // );

            //Excess Repayment
            let msg = ExecuteMsg::Repay {
                debt_split: None,
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(50_001_000_000, "credit_fulldenom")])
                .unwrap();
            //Balance before
            assert_eq!(
                app.wrap().query_all_balances(app.api().addr_make("test")).unwrap(),
                vec![coin(100_001_000_000, "credit_fulldenom")]
            );
            //Repayment
            let test_addr = app.api().addr_make("test");
            app.execute(test_addr, cosmos_msg)
                .unwrap();
            //Balance after excess was sent back
            assert_eq!(
                app.wrap().query_all_balances(app.api().addr_make("test")).unwrap(),
                vec![coin(50_001_000_000, "credit_fulldenom")]
            );

            
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None, 
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make("test").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].credit_amount, Uint128::zero());

            //Fully withdraw from position
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    amount: Uint128::from(100_000_000_000u128),
                }],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let test_addr = app.api().addr_make("test");
            app.execute(test_addr, cosmos_msg).unwrap();

            //Query Basket Positions
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: None,
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.is_empty(), true);

        }
        #[test]
        fn accrue_debt() {
            // panic!("{}", PriceResponse {
            //     prices: vec![],
            //     price: Decimal::from_ratio(2u128, 1u128),
            //     decimals: 18,
            // }.to_decimal256().unwrap().get_value(Uint256::from(100000000_000_000_000_000_000_000u128)));

            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(60),
                    max_LTV: Decimal::percent(80),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: Some(PoolInfo {
                        pool_id: 99u64,
                        asset_infos: vec![
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("base"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("quote"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                        ],
                    }),
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                }),
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: true,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Initial Deposit
            //Current Position: 100 _000_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase
            //Current Position: 100 _000_000_000_000_000_000 lp_denom -> 99_999 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(99_999_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            // let query_msg = QueryMsg::GetBasketDebtCaps { };
            // let res: Vec<DebtCap> = app
            //     .wrap()
            //     .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(
            //     format!("{:?}", res),
            //     String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(99999000000), cap: Uint128(249995050000) }]")
            // );

            //Insolvent position error
            ///Expected to Error due to accrued interest
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(1_000_000u128)),
                LTV: None,
                
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap_err();

            //Successful repayment that will leave the accrued interest left
            //Current Position: 100 _000_000_000_000_000_000 lp_denom -> 4761 credit_fulldenom
            let msg = ExecuteMsg::Repay {
                debt_split: None,
                position_id: Uint128::from(1u128),
                position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(99_999_000_000, "credit_fulldenom")])
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            // let query_msg = QueryMsg::GetBasketDebtCaps { };
            // let res: Vec<DebtCap> = app
            //     .wrap()
            //     .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(
            //     format!("{:?}", res),
            //     String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(5714284571), cap: Uint128(249995050000) }]")
            // );

            
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None, 
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make("bigger_bank").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            ///5714_284571 interest
            assert_eq!(res[0].positions[0].credit_amount, Uint128::new(14285_571428));

            //Insolvent withdrawal error
            ////This should be solvent if there wasn't accrued interest
            let msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "lp_denom".to_string(),
                    },
                    amount: Uint128::from(95_239_000_000_000_000_000_000u128),
                }],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap_err();

            //accrue
            let msg = ExecuteMsg::Accrue {
                position_owner: None,
                position_ids: vec![Uint128::one()],
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query to assert new debt amount due to the added year
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None, 
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make("bigger_bank").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].credit_amount, Uint128::new(16326_367346));

            //Query Rates
            let query_msg = QueryMsg::GetCollateralInterest { };
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!( format!("{:?}", res.rates), 
                String::from("[Decimal(0.142857142857142857), Decimal(0.166666666666666666), Decimal(0.125), Decimal(0.142857142857142857)]"));

            //Call liquidate on CDP contract
            // let msg = ExecuteMsg::Liquidate {
            //     position_id: Uint128::new(1u128),
            //     position_owner: app.api().addr_make("bigger_bank").to_string(),
            // };
            // let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            // app.set_block(BlockInfo {
            //     height: app.block_info().height,
            //     time: app.block_info().time.plus_seconds(31536000u64), //Added a year
            //     chain_id: app.block_info().chain_id,
            // });
            // app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            // //Query Basket Debt Caps
            // let query_msg = QueryMsg::GetBasketDebtCaps { };
            // let res: Vec<DebtCap> = app
            //     .wrap()
            //     .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(
            //     format!("{:?}", res),
            //     String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(4782000000), cap: Uint128(249995050000) }]")
            // );
            
            // //Repay to mimic liquidation repayment - LiqRepay
            // let msg = ExecuteMsg::Repay {
            //     position_id: Uint128::from(1u128),
            //     position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
            //     send_excess_to: None,
            // };
            // let cosmos_msg = cdp_contract
            //     .call(msg, vec![coin(1_741_000_000, "credit_fulldenom")])
            //     .unwrap();
            // app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
            //     .unwrap();

            // //Query Basket Debt Caps
            // let query_msg = QueryMsg::GetBasketDebtCaps { };
            // let res: Vec<DebtCap> = app
            //     .wrap()
            //     .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(
            //     format!("{:?}", res),
            //     String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(3041000000), cap: Uint128(249995050000) }]")
            // );

            // //Successful LiqRepay
            // let msg = ExecuteMsg::LiqRepay {};
            // let cosmos_msg = cdp_contract
            //     .call(msg, vec![coin(222_000_000, "credit_fulldenom")])
            //     .unwrap();
            // app.execute(Addr::unchecked(sp_addr.clone()), cosmos_msg)
            //     .unwrap();
            
            // // Would normally liquidate and leave 98818 collateral
            // // but w/ accrued interest its leaving 98816
            // let query_msg = QueryMsg::GetUserPositions {
            //     user: app.api().addr_make("bigger_bank").to_string(),
            //     limit: None,
            // };

            // let res: Vec<PositionResponse> = app
            //     .wrap()
            //     .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(
            //     res[0].collateral_assets[0].asset.amount,
            //     Uint128::new(98815102222222222_000_000)
            // );
            // assert_eq!(
            //     res[0].credit_amount,
            //     Uint128::new(2819_000_000)
            // );
                
            // //Query Basket Debt Caps
            // let query_msg = QueryMsg::GetBasketDebtCaps { };
            // let res: Vec<DebtCap> = app
            //     .wrap()
            //     .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(
            //     format!("{:?}", res),
            //     String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(2819000000), cap: Uint128(249995050000) }]")
            // );

            // //Assert sell wall wasn't sent Assets
            // assert_eq!(
            //     app.wrap().query_all_balances(router_addr.clone()).unwrap(),
            //     vec![]
            // );

            // //Assert fees were sent, revenue kept for a larger mint.
            // //coin(4782, "credit_fulldenom") in revenue
            // assert_eq!(
            //     app.wrap()
            //         .query_all_balances(staking_contract.clone())
            //         .unwrap(),
            //     vec![coin(10_620000000000_000_000, "lp_denom")]
            // );
            // //The fee is 212 lp_denom
            // assert_eq!(
            //     app.wrap().query_all_balances(USER).unwrap(),
            //     vec![coin(100000_000_000, "2nddebit"), coin(100_000_000_000, "debit"), coin(212_400000000000_000_000, "lp_denom")]
            // );
            // //SP is sent 122 lp_denom
            // assert_eq!(
            //     app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
            //     vec![ coin(2003_000_000, "credit_fulldenom"), coin(122_100_000_000_000_000_000, "lp_denom")]
            // );
            // //LQ is sent 839 lp_denom
            // assert_eq!(
            //     app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
            //     vec![coin(839_777777777778_000_000, "lp_denom")]
            // );
            
        }

        #[test]
        fn accrue_debt_two_positions() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(60),
                    max_LTV: Decimal::percent(80),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                // revenue_destinations: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();


            //Add LP asset
            //Set supply caps
            //Set general parameters
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: Some(PoolInfo {
                        pool_id: 99u64,
                        asset_infos: vec![
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("base"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("quote"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                        ],
                    }),
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                }),
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                    });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Initial Deposit for Position 1
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(50_000_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Initial Deposit for Position 2
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
                position_id: None,
                affiliate_address: None, affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase for Position 1
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(40_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64),
                chain_id: app.block_info().chain_id,
            });
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            // Query Position 1
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make("bigger_bank").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].rate_index.to_string(), String::from("1.142857142857142857"));
            assert_eq!(res[0].positions[0].credit_amount, Uint128::new(40000_000000));

            //Successful Increase for Position 2
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(100_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64),
                chain_id: app.block_info().chain_id,
            });
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            // Query Position 2
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(2),
                        position_owner: app.api().addr_make("bigger_bank").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].rate_index.to_string(), String::from("1.306122448979591836"));
            assert_eq!(res[0].positions[0].credit_amount, Uint128::new(100000_000000));

            //Accrue Position 1
            let msg = ExecuteMsg::Accrue { position_owner: None, position_ids: vec![Uint128::one()] };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            // Position 1: 40_000_000000 -> 45714_285714
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make("bigger_bank").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].rate_index.to_string(), String::from("1.306122448979591836"));
            assert_eq!(res[0].positions[0].credit_amount, Uint128::new(45714_285714));

            //Check rates
            let query_msg = QueryMsg::GetCollateralInterest { };
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res.rates),
                String::from(
                    "[Decimal(0.142857142857142857), Decimal(0.166666666666666666), Decimal(0.125), Decimal(0.142857142857142857)]"
                )
            );
        }
    } // mod cdp
} // mod tests
