
mod tests {

    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract};


    use membrane::liq_queue::LiquidatibleResponse as LQ_LiquidatibleResponse;
    use membrane::math::Uint256;
    use membrane::oracle::{AssetResponse, PriceResponse};
    use membrane::osmosis_proxy::{GetDenomResponse, TokenInfoResponse, OwnerResponse};
    use membrane::cdp::{ExecuteMsg, InstantiateMsg, QueryMsg, EditBasket, UpdateConfig};
    use membrane::stability_pool::LiquidatibleResponse as SP_LiquidatibleResponse;
    use membrane::staking::Config as Staking_Config;
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, Deposit, LiquidityInfo, TWAPPoolInfo,
        UserInfo, MultiAssetSupplyCap, AssetPool, StakeDistribution, PoolType, DebtCap, Owner
    };

    use cosmwasm_std::{
        attr, coin, to_binary, Addr, Binary, Coin, Decimal, Empty, Response, StdError, StdResult,
        Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use osmo_bindings::PoolStateResponse;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const USER: &str = "user";
    const ADMIN: &str = "admin";

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
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum LQ_MockExecuteMsg {
        Liquidate {
            credit_price: Decimal,     //Sent from Position's contract
            collateral_price: Decimal, //Sent from Position's contract
            collateral_amount: Uint256,
            bid_for: AssetInfo,
            position_id: Uint128,
            position_owner: String,
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

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct LQ_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum LQ_MockQueryMsg {
        CheckLiquidatible {
            bid_for: AssetInfo,
            collateral_price: Decimal,
            collateral_amount: Uint256,
            credit_info: AssetInfo,
            credit_price: Decimal,
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
                        position_id: _,
                        position_owner: _,
                    } => if collateral_amount.to_string() == String::from("1165777777777778") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", "1158".to_string()),
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
                    } else if collateral_amount.to_string() == String::from("839777777777778") {
                        Ok(Response::new().add_attributes(vec![
                            attr("action", "execute_bid"),
                            attr("repay_amount", 839.to_string()),
                            attr("collateral_token", bid_for.to_string()),
                            attr("collateral_info", "native_token"),
                            attr("collateral_amount", collateral_amount),
                        ]))
                    } else {
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
                    if collateral_amount.to_string() == String::from("1387999999999778") {
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1387u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1388000000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1380u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1277000000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1277u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1062000000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1062u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else {
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222".to_string(),
                            total_debt_repaid: (collateral_amount - Uint256::from(222u128))
                                .to_string(),
                        })?)
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
                        position_id: _,
                        position_owner: _,
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
                    } => Ok(to_binary(&LQ_LiquidatibleResponse {
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
                        position_id: _,
                        position_owner: _,
                    } => Err(StdError::GenericErr {
                        msg: "no siree".to_string(),
                    }),
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
                    if collateral_amount.to_string() == String::from("1388000000000000") {
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1388u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else if collateral_amount.to_string() == String::from("1277000000000000"){
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222222222222222".to_string(),
                            total_debt_repaid: (Uint256::from(1277u128) - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    } else {
                        Ok(to_binary(&LQ_LiquidatibleResponse {
                            leftover_collateral: "222".to_string(),
                            total_debt_repaid: (collateral_amount - Uint256::from(222u128))
                                .to_string(),
                        })?)
                    }
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
                        position_id: _,
                        position_owner: _,
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
                    } => Ok(to_binary(&LQ_LiquidatibleResponse {
                        leftover_collateral: "499".to_string(),
                        total_debt_repaid: (collateral_amount - Uint256::from(499u128))
                            .to_string(),
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock SP Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
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
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct SP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum SP_MockQueryMsg {
        CheckLiquidatible { amount: Decimal },
        AssetPool { },
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
                            //panic!("{}", liq_amount.to_string());
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
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible {amount: _ } => {
                        Ok(to_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { } => Ok(to_binary(&AssetPool {
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
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { amount: _ } => {
                        Ok(to_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { } => Ok(to_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::zero(),
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
                    SP_MockExecuteMsg::Liquidate { liq_amount: _ } => Err(StdError::GenericErr {
                        msg: "no siree".to_string(),
                    }),
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
                    } => Err(StdError::GenericErr {
                        msg: String::from("erroar"),
                    }),
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { amount: _ } => {
                        Ok(to_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { } => Ok(to_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::zero(),
                        },
                        liq_premium: Decimal::percent(10),
                        deposits: vec![Deposit {
                            user: Addr::unchecked(USER),
                            amount: Decimal::percent(222_00),
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
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { amount: _ } => {
                        Ok(to_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { } => Ok(to_binary(&AssetPool {
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "cdt".to_string(),
                            },
                            amount: Uint128::zero(),
                        },
                        liq_premium: Decimal::percent(10),
                        deposits: vec![],
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
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { amount: _ } => {
                        Ok(to_binary(&SP_LiquidatibleResponse {
                            leftover: Decimal::zero(),
                        })?)
                    }
                    SP_MockQueryMsg::AssetPool { } => Ok(to_binary(&AssetPool {
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
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
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
        CreateDenom {
            subdenom: String,
            max_supply: Option<Uint128>,
            liquidity_multiplier: Option<Decimal>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
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
                }
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
                                shares: coin(100_000_000_000_000_000_000, "lp_denom"),
                            })?)
                        } else {
                            Ok(to_binary(&PoolStateResponse {
                                assets: vec![coin(49_999, "credit_fulldenom")],
                                shares: coin(0, "shares"),
                            })?)
                        }
                    }
                    Osmo_MockQueryMsg::GetDenom {
                        creator_address: _,
                        subdenom: _,
                    } => Ok(to_binary(&GetDenomResponse {
                        denom: String::from("credit_fulldenom"),
                    })?),
                    Osmo_MockQueryMsg::GetTokenInfo { denom } => {
                        Ok(to_binary(&TokenInfoResponse {
                            denom,
                            current_supply: Uint128::new(200_000u128),
                            max_supply: Uint128::new(1_000_000_000_000_000u128),
                            burned_supply: Uint128::zero(),
                        })?)
                    },
                    Osmo_MockQueryMsg::GetOwner { owner } => {
                        Ok(to_binary(&OwnerResponse {
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
                }
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
                                shares: coin(100_000_000_000_000_000_000, "lp_denom"),
                            })?)
                        } else {
                            Ok(to_binary(&PoolStateResponse {
                                assets: vec![coin(5_000_000_000_000, "credit_fulldenom")],
                                shares: coin(0, "shares"),
                            })?)
                        }
                    }
                    Osmo_MockQueryMsg::GetDenom {
                        creator_address: _,
                        subdenom: _,
                    } => Ok(to_binary(&GetDenomResponse {
                        denom: String::from("credit_fulldenom"),
                    })?),
                    Osmo_MockQueryMsg::GetTokenInfo { denom } => {
                        Ok(to_binary(&TokenInfoResponse {
                            denom,
                            current_supply: Uint128::new(200_000u128),
                            max_supply: Uint128::new(1_000_000_000_000_000u128),
                            burned_supply: Uint128::zero(),
                        })?)
                    },
                    Osmo_MockQueryMsg::GetOwner { owner } => {
                        Ok(to_binary(&OwnerResponse {
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

    //Mock Router Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Router_MockExecuteMsg {
        BasketLiquidate {
            offer_assets: apollo_cw_asset::AssetListUnchecked,
            receive_asset: apollo_cw_asset::AssetInfoUnchecked,
            minimum_receive: Option<Uint128>,
            to: Option<String>,
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Router_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Router_MockQueryMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
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
                Ok(to_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }

    //Mock Auction Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Auction_MockExecuteMsg {
        StartAuction {
            position_id: Uint128,
            position_owner: String,
            debt_amount: Uint128,
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
                Ok(to_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }

    //Mock Staking Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockExecuteMsg {
        DepositFee {},
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Staking_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
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
                        Ok(to_binary(&Staking_Config {
                            owner: Addr::unchecked(""),
                            mbrn_denom: String::from("mbrn_denom"),
                            incentive_schedule: StakeDistribution { rate: Decimal::zero(), duration: 0 },
                            keep_raw_cdt: false,
                            vesting_rev_multiplier: Decimal::zero(),
                            max_commission_rate: Decimal::zero(),
                            fee_wait_period: 0,
                            unstaking_period: 0,
                            positions_contract: None,
                            auction_contract: None,
                            vesting_contract: None,
                            governance_contract: None,
                            osmosis_proxy: None,
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Discounts Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Discounts_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Discounts_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
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
                            Ok(to_binary(&Decimal::percent(90))?)
                        } else {
                            Ok(to_binary(&Decimal::zero())?)
                        }
                        
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Oracle Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
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
                    Oracle_MockQueryMsg::Price {
                        asset_info,
                        twap_timeframe,
                        basket_id,
                    } => {
                        if basket_id.is_some() {
                            if basket_id.unwrap() == Uint128::new(2u128) {
                                Ok(to_binary(&PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(500),
                                    decimals: 6,
                                })?)
                            } else if asset_info.to_string() == String::from("credit_fulldenom") {
                                Ok(to_binary(&PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(98),
                                    decimals: 6,
                                })?)
                            } else if asset_info.to_string() == String::from("lp_denom") {
                                Ok(to_binary(&PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(200),
                                    decimals: 18,
                                })?)
                            } else {
                                Ok(to_binary(&PriceResponse {
                                    prices: vec![],
                                    price: Decimal::one(),
                                    decimals: 6,
                                })?)
                            }
                        } else if asset_info.to_string() == String::from("credit_fulldenom") { 
                            Ok(to_binary(&PriceResponse {
                                prices: vec![],
                                price: Decimal::percent(98),
                                decimals: 6,
                            })?)
                        } else if asset_info.to_string() == String::from("lp_denom") {
                            Ok(to_binary(&PriceResponse {
                                prices: vec![],
                                price: Decimal::percent(200),
                                decimals: 18,
                            })?)
                        } else {
                            Ok(to_binary(&PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            })?)
                        }
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_binary(&vec![
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
                    Oracle_MockQueryMsg::Price {
                        asset_info,
                        twap_timeframe,
                        basket_id,
                    } => {
                        
                        //Everything is $1 unless basket#2 which is $5 collateral and $1.02 credit
                        if basket_id.is_some() {
                            if basket_id.unwrap() == Uint128::new(2u128) {
                                Ok(to_binary(&PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(500),
                                    decimals: 6,
                                })?)
                            } else if asset_info.to_string() == String::from("credit_fulldenom") {
                                Ok(to_binary(&PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(102),
                                    decimals: 6,
                                })?)
                            } else if asset_info.to_string() == String::from("lp_denom") {
                                Ok(to_binary(&PriceResponse {
                                    prices: vec![],
                                    price: Decimal::percent(200),
                                    decimals: 18,
                                })?)
                            } else {
                                Ok(to_binary(&PriceResponse {
                                    prices: vec![],
                                    price: Decimal::one(),
                                    decimals: 6,
                                })?)
                            }
                        } else if asset_info.to_string() == String::from("credit_fulldenom") {
                            Ok(to_binary(&PriceResponse {
                                prices: vec![],
                                price: Decimal::percent(102),
                                decimals: 6,
                            })?)
                        } else if asset_info.to_string() == String::from("lp_denom") {
                            Ok(to_binary(&PriceResponse {
                                prices: vec![],
                                price: Decimal::percent(200),
                                decimals: 18,
                            })?)
                        } else {
                            Ok(to_binary(&PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            })?)
                        }
                    }
                    Oracle_MockQueryMsg::Assets { asset_infos } => Ok(to_binary(&vec![
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
                            }],
                        }
                    ])?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock Liquidity Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Liquidity_MockExecuteMsg {
        AddAsset { asset: LiquidityInfo },
        EditAsset { asset: LiquidityInfo },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Liquidity_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
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
                        Ok(to_binary(&Uint128::new(49999u128))?)
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
                        Ok(to_binary(&Uint128::new(5_000_000_000_000u128))?)
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
                &Addr::unchecked(USER),
                vec![coin(100_000, "debit"), coin(100_000, "2nddebit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract0"),
                vec![coin(2225, "credit_fulldenom")],
            )
            .unwrap(); //contract1 = Stability Pool contract
            bank.init_balance(
                storage,
                &Addr::unchecked("test"),
                vec![coin(50_000, "credit_fulldenom"), coin(100_000, "debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("sender"),
                vec![coin(50_000_001, "credit_fulldenom")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("big_bank"),
                vec![coin(10_000_000, "debit"), coin(10_000_000, "double_debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("bigger_bank"),
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
                &Addr::unchecked("little_bank"),
                vec![coin(1_000, "debit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("redeemer"),
                vec![coin(100_000, "credit_fulldenom"), coin(1, "not_redeemable")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("LP_assets"),
                vec![ coin(2_328, "base"), coin(2_328, "quote")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("coin_God"),
                vec![coin(2_250_000_000_000, "credit_fulldenom")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("lp_tester"),
                vec![coin(100_000_000_000_000_000_000, "lp_denom")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("faker"),
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

        //Instanitate SP
        let sp_id: u64;
        if sp_error {
            sp_id = app.store_code(stability_pool_contract_errors());
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
                Addr::unchecked(ADMIN),
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
                Addr::unchecked(ADMIN),
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
                Addr::unchecked(ADMIN),
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
                Addr::unchecked(ADMIN),
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
                Addr::unchecked(ADMIN),
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
                Addr::unchecked(ADMIN),
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
                Addr::unchecked(ADMIN),
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
                Addr::unchecked(ADMIN),
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
                Addr::unchecked(ADMIN),
                &Discounts_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate CDP contract
        let cdp_id = app.store_code(cdp_contract());

        let msg = InstantiateMsg {
            owner: Some(ADMIN.to_string()),
            liq_fee: Decimal::percent(1),
            stability_pool: Some(sp_contract_addr.to_string()),
            dex_router: Some(router_contract_addr.to_string()),
            staking_contract: Some(staking_contract_addr.to_string()),
            oracle_contract: Some(oracle_contract_addr.to_string()),
            osmosis_proxy: Some(osmosis_proxy_contract_addr.to_string()),
            debt_auction: Some(auction_contract_addr.to_string()),
            liquidity_contract: Some(liquidity_contract_addr.to_string()),
            discounts_contract: Some(discounts_contract_addr.to_string()),
            oracle_time_limit: 60u64,
            debt_minimum: Uint128::new(2000u128),
            collateral_twap_timeframe: 60u64,
            credit_twap_timeframe: 480u64,
            rate_slope_multiplier: Decimal::from_str("0.618").unwrap(),
            base_debt_cap_multiplier: Uint128::new(21u128),
        };
        let cdp_contract_addr = app
            .instantiate_contract(cdp_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let cdp_contract = CDPContract(cdp_contract_addr);
        
        let msg = ExecuteMsg::CreateBasket {
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
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            credit_pool_infos: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        (app, cdp_contract, lq_contract)
    }

    mod cdp {

        use super::*;
        use cosmwasm_std::{coins, BlockInfo};
        use membrane::cdp::{
            BadDebtResponse, CollateralInterestResponse, Config, BasketPositionsResponse,
            ExecuteMsg, InsolvencyResponse, PositionResponse, InterestResponse, RedeemabilityResponse
        };
        use membrane::types::{InsolventPosition, LPAssetInfo, PoolInfo, SupplyCap, UserInfo, Basket};

        
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial debit Deposit
            //50_000 debit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(USER.to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(50_000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Debit Deposit into Position #2
            //50_000 debit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(USER.to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(50_000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert user positions
            let query_msg = QueryMsg::GetUserPositions {
                user: String::from(USER),
                limit: None,
            };

            let res: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
                
            ////Assert collaterals, then length
            assert_eq!(
                res[0].collateral_assets,
                vec![
                    cAsset {
                        asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "debit".to_string(),
                            },
                            amount: Uint128::from(50_000u128),
                        },
                        max_borrow_LTV: Decimal::percent(50),
                        max_LTV: Decimal::percent(70),
                        pool_info: None,  
                        rate_index: Decimal::one(),
                    }
                ]
            );
            assert_eq!(
                res[1].collateral_assets,
                vec![
                    cAsset {
                        asset: Asset {
                            info: AssetInfo::NativeToken {
                                denom: "debit".to_string(),
                            },
                            amount: Uint128::from(50_000u128),
                        },
                        max_borrow_LTV: Decimal::percent(50),
                        max_LTV: Decimal::percent(70),
                        pool_info: None,  
                        rate_index: Decimal::one(),
                    }
                ]
            );
            assert_eq!(
                res.len().to_string(),
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
                        amount: Uint128::from(10_000u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Partial withdrawal for Position #2
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(2u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(10_000u128),
                    },
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert user positions
            let query_msg = QueryMsg::GetUserPositions {
                user: String::from(USER),
                limit: None,
            };

            let res: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            ////Assert collaterals
            assert_eq!(
                res[0].collateral_assets[0].asset,
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(40_000u128),
                        }
            );
            assert_eq!(
                res[1].collateral_assets[0].asset,
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(40_000u128),
                        }
            );

            //Increase Debt for Position #1
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(10_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Send credit
            app.send_tokens(
                Addr::unchecked("sender"),
                Addr::unchecked(USER),
                &[coin(10_000, "credit_fulldenom")],
            )
            .unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(20_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Send credit
            app.send_tokens(
                Addr::unchecked("sender"),
                Addr::unchecked(USER),
                &[coin(20_000, "credit_fulldenom")],
            )
            .unwrap();

            //Assert user positions
            let query_msg = QueryMsg::GetUserPositions {
                user: String::from(USER),
                limit: None,
            };

            let res: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            ////Assert credit_amount
            assert_eq!(
                res[0].credit_amount,
                Uint128::from(10_000u128)
            );
            assert_eq!(
                res[1].credit_amount,
                Uint128::from(20_000u128)
            );

            //Repayment for Position #1
            let repay_msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(repay_msg, vec![coin(10_000, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Liquidate Position #2 
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(2u128),
                position_owner: USER.to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert user positions
            let query_msg = QueryMsg::GetUserPositions {
                user: String::from(USER),
                limit: None,
            };

            let res: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            ////Assert credit_amount
            ///Note: Position_id order flips
            assert_eq!(
                res[1].position_id,
                Uint128::new(2)
            );
            assert_eq!(
                res[1].credit_amount,
                Uint128::from(20_000u128)
            );
            assert_eq!(
                res[0].position_id,
                Uint128::new(1)
            );
            assert_eq!(
                res[0].credit_amount,
                Uint128::zero()
            );
        }

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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(USER.to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                        },
                        Coin {
                            denom: "2nddebit".to_string(),
                            amount: Uint128::from(100_000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Successful debt increase to initiate caps
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert asset tally is working
            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.collateral_supply_caps[0].current_supply,
                Uint128::new(100000)
            );
            assert_eq!(
                res.collateral_supply_caps[1].current_supply,
                Uint128::new(100000)
            );

            //Query BasketPositions
            let msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
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
                String::from("100000")
            );
            assert_eq!(
                resp[0].positions[0].collateral_assets[1]
                    .asset
                    .amount
                    .to_string(),
                String::from("100000")
            );
            assert_eq!(resp.len().to_string(), String::from("1"));

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Insolvent withdrawal error
            let msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(100_000u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(100_000u128),
                    },
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Successful attempt
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(90_000u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(90_000u128),
                    },
                ],
                send_to: Some(String::from("very_trusted_contract")),
            };

            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query Position assets to assert withdrawal
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(10000));
            assert_eq!(res.collateral_assets[1].asset.amount, Uint128::new(10000));

            //Assert withdrawal was sent to sent_to.
            assert_eq!(
                app.wrap().query_all_balances("very_trusted_contract").unwrap(),
                vec![coin(90000, "2nddebit"), coin(90000, "debit")]
            );

            //Assert asset tally is working
            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.collateral_supply_caps[0].current_supply,
                Uint128::new(10000)
            );
            assert_eq!(
                res.collateral_supply_caps[1].current_supply,
                Uint128::new(10000)
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
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("test".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Insolvent position error
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_001u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg)
                .unwrap_err();

            //Minimum Debt Error
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(1u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg)
                .unwrap_err();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Send credit
            app.send_tokens(
                Addr::unchecked("sender"),
                Addr::unchecked("test"),
                &[coin(50_001, "credit_fulldenom")],
            )
            .unwrap();

            //Error on Partial Repayment under config.debt_minimum
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(49_901, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg)
                .unwrap_err();

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "test".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.credit_amount, Uint128::new(50000));

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(50000), cap: Uint128(299995) }]")
            );

            //Excess Repayment
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(50_001, "credit_fulldenom")])
                .unwrap();
            //Balance before
            assert_eq!(
                app.wrap().query_all_balances(Addr::unchecked("test")).unwrap(),
                vec![coin(100_001, "credit_fulldenom")]
            );
            //Repayment
            app.execute(Addr::unchecked("test"), cosmos_msg)
                .unwrap();
            //Balance after excess was sent back
            assert_eq!(
                app.wrap().query_all_balances(Addr::unchecked("test")).unwrap(),
                vec![coin(50_001, "credit_fulldenom")]
            );

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "test".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.credit_amount, Uint128::zero());
        }

        #[test]
        fn accrue_debt() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            //Current Position: 100_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase
            //Current Position: 100_000_000_000_000_000 lp_denom -> 99_999 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(99_999u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(99999), cap: Uint128(299995) }]")
            );

            //Insolvent position error
            ///Expected to Error due to accrued interest
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(1u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap_err();

            //Successful repayment that will leave the accrued interest left
            //Current Position: 100_000_000_000_000_000 lp_denom -> 4761 credit_fulldenom
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: Some(String::from("bigger_bank")),
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(99_999, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(4761), cap: Uint128(299995) }]")
            );

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            ///4761 interest
            assert_eq!(res.credit_amount, Uint128::new(4761));

            //Insolvent withdrawal error
            ////This should be solvent if there wasn't accrued interest
            let msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "lp_denom".to_string(),
                    },
                    amount: Uint128::from(95_239_000_000_000_000u128),
                }],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap_err();

            //Query to assert new debt amount due to the added year
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.credit_amount, Uint128::new(4771));

            //Query Rates
            let query_msg = QueryMsg::GetCollateralInterest { };
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!( format!("{:?}", res.rates), 
                String::from("[Decimal(0), Decimal(0), Decimal(0), Decimal(0.002267180643486915)]"));

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(4782), cap: Uint128(299995) }]")
            );

            //Repay to mimic liquidation repayment - LiqRepay
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: Some(String::from("bigger_bank")),
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(1_741, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(3041), cap: Uint128(299995) }]")
            );

            //Successful LiqRepay
            let msg = ExecuteMsg::LiqRepay {};
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(222, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(sp_addr.clone()), cosmos_msg)
                .unwrap();
            
            // Would normally liquidate and leave 98818 collateral
            // but w/ accrued interest its leaving 98816
            let query_msg = QueryMsg::GetUserPositions {
                user: String::from("bigger_bank"),
                limit: None,
            };

            let res: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res[0].collateral_assets[0].asset.amount,
                Uint128::new(98815102222222222)
            );
            assert_eq!(
                res[0].credit_amount,
                Uint128::new(2819)
            );
                
            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(2819), cap: Uint128(299995) }]")
            );

            //Assert sell wall wasn't sent Assets
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![]
            );

            //Assert fees were sent, revenue kept for a larger mint.
            //coin(4782, "credit_fulldenom") in revenue
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(10_620000000000, "lp_denom")]
            );
            //The fee is 212 lp_denom
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(100_000, "debit"), coin(212_400000000000, "lp_denom")]
            );
            //SP is sent 122 lp_denom
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![ coin(2003, "credit_fulldenom"), coin(122_100_000_000_000, "lp_denom")]
            );
            //LQ is sent 839 lp_denom
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(839_777777777778, "lp_denom")]
            );
            
        }

        #[test]
        fn accrue_debt_two_positions() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit for Position 1
            //Current Position: 50_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(50_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Initial Deposit for Position 2
            //Current Position: 100_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase for Position 1
            //Current Position: 50_000_000_000_000_000 lp_denom -> 40_000 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(40_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
           

            //////Querying Position 1 to assert debt and rate_index
            // Position 1
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            //Rate was at 0 so nothing accrued to the index
            assert_eq!(res.collateral_assets[0].rate_index.to_string(), String::from("1"));
            assert_eq!(res.credit_amount, Uint128::new(40000));

          
            //Successful Increase for Position 2
            //Current Position: 100_000_000_000_000_000 lp_denom -> 100_000 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(100_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();            
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //This is where Position 1's rate gets set
            
            ///Query both positions to assert Position 2 index was added correctly 
            /// AND higher rate didn't affect Position 1's initial accrued interest debt
            //
            //Position 2
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(2u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].rate_index.to_string(), String::from("1.022857600009143039"));
            assert_eq!(res.credit_amount, Uint128::new(100000));

            // Position 1
            //40_000 -> 41066
            //2.28% 
            //Asserting that the credit wasn't accrued ~8%
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].rate_index.to_string(), String::from("1.022857600009143039"));
            assert_eq!(res.credit_amount, Uint128::new(40914));


            //Check rates to confirm its at ~8%
            let query_msg = QueryMsg::GetCollateralInterest { };
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res.rates),
                String::from(
                    "[Decimal(0), Decimal(0), Decimal(0), Decimal(0.080001600032000639)]"
                )
            );
        }

        #[test]
        fn accrue_discounted_debt() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            //Manually adding Discount contract
            //It is added during the InstantiationMsg but there is some weird error that isn't adding it
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: None,
                liquidity_contract: None,
                discounts_contract: Some(String::from("contract8")),
                liq_fee: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            //Current Position: 100_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("discounty".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase
            //Current Position: 100_000_000_000_000_000 lp_denom -> 99_999 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(99_999u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("discounty"), cosmos_msg)
                .unwrap();

            //Check interest rates
            //4.7% rate
            let query_msg = QueryMsg::GetCollateralInterest { };
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res.rates),
                String::from(
                    "[Decimal(0), Decimal(0), Decimal(0), Decimal(0.047619365084656172)]"
                )
            );

            //Assert the 90% discount on rates
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "discounty".to_string(),
            };            
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.credit_amount, Uint128::new((100475)) );
        }

        #[test]
        fn accrue_credit_repayment_price() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, true);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                cpc_margin_of_error: Some( Decimal::percent(1) ),
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("test".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(49_999u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();
            //Send credit
            app.send_tokens(
                Addr::unchecked("sender"),
                Addr::unchecked("test"),
                &[coin(49_999, "credit_fulldenom")],
            )
            .unwrap();

            //Insolvent position error
            ///Expected to Error due to a greater repayment price
            /// //otherwise this would be solvent and a valid increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(1u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("test"), cosmos_msg)
                .unwrap_err();

            //Successful repayment up to the new minimum debt
            //With only repayment price increases, the amount being repaid doesn't change..
            //..but the amount that results in minimum debt errors decreases
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(50_117, "credit_fulldenom")])
                .unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Assert Increased credit price is saved correctly
            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.credit_price.to_string(), String::from("1.040816326530612244"));

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "test".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.credit_amount, Uint128::new((1922)) );

            //Insolvent withdrawal at that brings position to previous debt minimum
            ////This wouldn't be insolvent if there wasn't an increased repayment price
            /// 1960 backed by 3920: 50% borrow LTV so would've been solvent at $1 credit
            let msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    amount: Uint128::from(96_080u128),
                }],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64),
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("test"), cosmos_msg)
                .unwrap_err();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: "test".to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Would normally liquidate and leave 97770 "debit"
            // but w/ accrued interest its leaving 97442
            let query_msg = QueryMsg::GetUserPositions {
                user: String::from("test"),
                limit: None,
            };
            let res: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res[0].collateral_assets[0].asset.amount,
                Uint128::new(97442)
            );           

            //////////////NEGATIVE RATES///////
            ///
            /// ///////
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, true);

            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                cpc_margin_of_error: Some( Decimal::percent(1) ),
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("test".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(49_999u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();
            //Send credit
            app.send_tokens(
                Addr::unchecked("sender"),
                Addr::unchecked("test"),
                &[coin(49_999, "credit_fulldenom")],
            )
            .unwrap();

            ///Expected to pass due to a lower repayment price
            /// //otherwise this would be insolvent
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();

            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Successful repayment up to the new minimum debt
            //With repayment price decreases, the amount being repaid doesn't change..
            //..but the amount that results in minimum debt errors increases from 2000 to 2002
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(47_999, "credit_fulldenom")])
                .unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            let err = app
                .execute(Addr::unchecked("test"), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Position's debt is below minimum: 2000")
            );

            //Assert Increased credit price is saved correctly
            //After 3 years
            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.credit_price.to_string(), String::from("0.94"));

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "test".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.credit_amount, Uint128::new(50001));

            //Query Redemption Rate/Repayment Interest
            let resp: InterestResponse = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &QueryMsg::GetCreditRate { })
            .unwrap();

            assert_eq!(
                resp.credit_interest.to_string(),
                String::from("0.085106382978723404"),
            );
            assert_eq!(
                resp.negative_rate,
                true,
            );
        }

        #[test]
        fn accrue_repayment_rate_to_interest_rate() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, true);

            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: Some( Decimal::percent(1) ),
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
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
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Assert interest rates decreased from the negative redemption rate
            //Base rate is 285714285714
            //Accrued rate is 279999999999
            let query_msg = QueryMsg::GetCollateralInterest { };
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.rates[0].to_string(),
                String::from("0.000279999999999999")
            );
            
        }

        #[test]
        fn interest_rates() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit for Position 1
            //Current Position: 1_000_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(1_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

           
            //Successful Increase for Position 1
            //Current Position: 1_000_000_000_000_000_000 lp_denom -> 2_000 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(2000), cap: Uint128(249995) }]")
            );
                             
           
            //Check lp_denom rate is near 0% due to low debt
            let query_msg = QueryMsg::GetCollateralInterest { };
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
               res.rates[3].to_string(),
                String::from("0.001333360000533343")
            );

            //Successful Increase for Position 1
            //Current Position: 1_000_000_000_000_000_000 lp_denom -> 199_996 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(197_996u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(199996), cap: Uint128(249995) }]")
            );                             
           
            //Check lp_Denom rate is at the top of Slope 1 due to debt at the desired_debt_util (80%)
            let query_msg = QueryMsg::GetCollateralInterest { };
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.rates[3].to_string(),
                String::from("0.133333333333333332")
            );

            //Successful Increase for Position 1
            //Current Position: 1_000_000_000_000_000_000 lp_denom -> 249_995 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(49_999u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(249995), cap: Uint128(249995) }]")
            );
              
            // //Lower debt cap to test Slope 2
            // let msg = ExecuteMsg::EditBasket(EditBasket {
            //     added_cAsset: None,
            //     liq_queue: None,
            //     credit_pool_infos: None,
            //     collateral_supply_caps: None,
            //     base_interest_rate: None,
            //     credit_asset_twap_price_source: None,
            //     negative_rates: None,
            //     cpc_margin_of_error: None,
            //     frozen: None,
            //     rev_to_stakers: None,
            //     multi_asset_supply_caps: None,
            // });
            // let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            // app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
           
            // //Check lp_denom rate is in Slope 2 due to debt above the cap
            // let query_msg = QueryMsg::GetCollateralInterest { };
            // let res: CollateralInterestResponse = app
            //     .wrap()
            //     .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(
            //    res.rates[3].to_string(),
            //     String::from("5.416666632")
            // );
        }

        #[test]
        fn revenue() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Edit Basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: Some(false),
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("test".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(49_999u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();
            //Send credit
            app.send_tokens(
                Addr::unchecked("sender"),
                Addr::unchecked("test"),
                &[coin(49_999, "credit_fulldenom")],
            )
            .unwrap();

            //Successful repayment that will leave the accrued interest left
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(49_000, "credit_fulldenom")])
                .unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            ///1428 revenue 
            assert_eq!(res.pending_revenue.to_string(), String::from("1428"));

            //Successful Mint
            let msg = ExecuteMsg::MintRevenue {
                send_to: Some(String::from("revenue_collector")),
                repay_for: None,
                amount: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Mint fields are asserted in the msg handler
            //So as long as the Osmo Proxy contract works, the mint will
        }

        #[test]
        fn liq_repay() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("test".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            /// //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Successful liquidation
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: "test".to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Unauthorized
            let msg = ExecuteMsg::LiqRepay {};
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg)
                .unwrap_err();

            //Send SP funds to liquidate
            app.send_tokens(
                Addr::unchecked("sender"),
                Addr::unchecked(sp_addr.clone()),
                &[coin(50_000, "credit_fulldenom")],
            )
            .unwrap();

            //Successful LiqRepay
            let msg = ExecuteMsg::LiqRepay {};
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(50_000, "credit_fulldenom")])
                .unwrap();
            let res = app.execute(Addr::unchecked(sp_addr), cosmos_msg).unwrap();

            //Assert messages
            let response = res
                .events
                .into_iter()
                .find(|e| e.attributes.iter().any(|attr| attr.value == "liq_repay"))
                .ok_or_else(|| panic!("unable to find LIQ_REPAY event"))
                .unwrap();

            assert_eq!(
                response.attributes[1..],
                vec![
                    attr("method", "repay"),
                    attr("position_id","1"),
                    attr("loan_amount","0"),                    
                    attr("method", "liq_repay"),
                    attr("distribution_assets", String::from("[Asset { info: NativeToken { denom: \"debit\" }, amount: Uint128(55000) }]")),
                    attr("distribute_for", "50000"),
                ]
            );

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "test".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.credit_amount, Uint128::zero());
        }

        #[test]
        fn liquidate() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(USER.to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call LiqRepay to mimic a successfull SP Liquidate call
            let msg = ExecuteMsg::LiqRepay {};

            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(222, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(sp_addr.clone()), cosmos_msg)
                .unwrap();

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97290));

            //Assert sell wall wasn't sent assets
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(22, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(444, "debit")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(2003, "credit_fulldenom"), coin(244, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(2000, "debit")]
            );
        

            /////////SP Errors////
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(true, false, false, false);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(USER.to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97312));

            //Assert sell wall was sent assets
            //SW increases from 222 to 444 bc the user's SP repayment failed as well
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![coin(444, "debit")]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(22, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(444, "debit")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(1778, "debit")]
            );
            //Assert SP wasn't sent any due to the Error
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(2225, "credit_fulldenom")]
            );

            //////LQ Errors///
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(USER.to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call LiqRepay to mimic a successfull SP Liquidate call by both the initial SP and then LQ reply
            let msg = ExecuteMsg::LiqRepay {};

            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(2225, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(sp_addr.clone()), cosmos_msg)
                .unwrap();

            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97087));

            //Assert sell wall wasn't sent assets
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(22, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(444, "debit")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(2447, "debit")]
            );
            //Assert LQ wasn't sent any due to the Error
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![]
            );

            //////Both Errors/////
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(true, true, false, false);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(USER.to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97312));

            //Assert sell wall was sent assets all Assets
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![coin(2222, "debit")]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(22, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(444, "debit")]
            );

            //Assert neither module was sent any due to the Error
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(2225, "credit_fulldenom")]
            );
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![]
            );

            //Assert asset tally is working
            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.collateral_supply_caps[0].current_supply,
                Uint128::new(97312)
            );

            //////LQ Errors///
            /// and SP skips due to high premium
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, true, false);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(USER.to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(10_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(7090));

            //Assert sell wall was sent all assets. Initial allocation + LQ_errors
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![coin(2444, "debit")]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(22, "debit")]
            );
            //444 debit was the fee
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(90444, "debit")]
            );

            //Assert SP wasn't sent any due to a high premium
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(2225, "credit_fulldenom")]
            );
            //Assert LQ wasn't sent any due to the Error
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![]
            );
        }

        #[test]
        fn liquidate_LPs() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            //Add LQ
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call LiqRepay to mimic a successfull SP Liquidate call
            let msg = ExecuteMsg::LiqRepay {};

            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(1611, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(sp_addr.clone()), cosmos_msg)
                .unwrap();

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97518442222222222));
            //2777 credit liquidated at $1
            //lp_denom is worth $2
            //Assert sell wall wasn't sent assets
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(13880000000000, "lp_denom")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(100000, "debit"), coin(416400000000000, "lp_denom")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(614, "credit_fulldenom"), coin(885500000000000, "lp_denom")]
            );
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(1165777777777778, "lp_denom")]
            );
            
            /////////SP Errors////
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(true, false, false, false);

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            //Add LQ
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            //Send CDP the LP pool assets to mimic a withdrawal
            app.send_tokens(
                Addr::unchecked("bigger_bank"),
                Addr::unchecked(cdp_contract.clone().addr()),
                &vec![coin(861, "base"), coin(861, "quote")],
            )
            .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();           

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97653942222222222));

            //Assert sell wall was sent assets
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![coin(861, "base"), coin(861, "quote")]
            );

            //Assert 1% fee was sent.
            //This is 13 instead of 27 bc the share token is the only collateral worth $2 instead of 1.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(13_880000000000, "lp_denom")]
            );
            //Assert 30% fee
            //Same here, 416 instead of 833 if it were valued at a $1.
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(100000, "debit"), coin(416_400000000000, "lp_denom")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(1054_777777777778, "lp_denom")]
            );            
            //Assert SP wasn't sent any due to the Error
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(2225, "credit_fulldenom")]
            );

            //////LQ Errors///
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            //Add LQ
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();    
            
            //Call LiqRepay to mimic a successfull SP Liquidate call by both the initial SP and then LQ reply
            let msg = ExecuteMsg::LiqRepay {};

            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(2225, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(sp_addr.clone()), cosmos_msg)
                .unwrap();

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(98346520000000000));

            //Assert sell wall wasn't sent assets
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(13_880000000000, "lp_denom")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(100000, "debit"), coin(416_400000000000, "lp_denom")]
            );

            //Assert collateral to be liquidated was sent
            //$2447 worth
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(1223_200000000000, "lp_denom")]
            );
            //Assert LQ wasn't sent any due to the Error
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![]
            );

            //////Both Errors/////
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(true, true, false, false);
            
            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            //Add LQ
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            //Send CDP the LP pool assets to mimic a withdrawal
            app.send_tokens(
                Addr::unchecked("bigger_bank"),
                Addr::unchecked(cdp_contract.clone().addr()),
                &vec![coin(1388, "base"), coin(1388, "quote")],
            )
            .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();    
           
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(98181720000000000));

            //Assert sell wall was sent assets all Assets
            //For $2777 worth of liquidations
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![coin(1388, "base"), coin(1388, "quote")]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(13_880000000000, "lp_denom")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(100000, "debit"), coin(416_400000000000, "lp_denom")]
            );

            //Assert neither module was sent any due to the Error
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(2225, "credit_fulldenom")]
            );
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![]
            );
        }

        #[test]
        fn liquidate_bignums() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, true);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call LiqRepay to mimic a successfull SP Liquidate call
            let msg = ExecuteMsg::LiqRepay {};

            app.send_tokens(
                Addr::unchecked("coin_God"),
                Addr::unchecked(sp_addr.clone()),
                &vec![coin(222_222_222_222, "credit_fulldenom")],
            )
            .unwrap();
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(222_222_222_222, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(sp_addr.clone()), cosmos_msg)
                .unwrap();

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: "bigger_bank".to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.collateral_assets[0].asset.amount,
                Uint128::new(97_288_888_888_893)
            );

            //Assert sell wall wasn't sent assets
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(22_222_222_222, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(444_444_544_444, "debit")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![
                    coin(2225, "credit_fulldenom"),
                    coin(244_444_444_444, "debit")
                ]
            );
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(1_999_999_999_997, "debit")]
            );
        }

        #[test]
        fn liquidate_minimums() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Lower debt minimum
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                stability_pool: None,
                dex_router: None,
                osmosis_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: None,
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                debt_minimum: Some(Uint128::new(500u128)),
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(USER.to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt to a point where a liquidations:
            //1. Liquidates less than the debt_minimum
            //2. Brings the position below the minimum debt
            //..Which then results in a full liquidation
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(999u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call LiqRepay to mimic a successfull SP Liquidate call
            let msg = ExecuteMsg::LiqRepay {};

            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(499, "credit_fulldenom")])
                .unwrap();
            app.execute(Addr::unchecked(sp_addr.clone()), cosmos_msg)
                .unwrap();

            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(98744));

            //Assert sell wall wasn't sent assets
            assert_eq!(
                app.wrap().query_all_balances(router_addr.clone()).unwrap(),
                vec![]
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(9, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000, "2nddebit"), coin(199, "debit")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(sp_addr.clone()).unwrap(),
                vec![coin(1726, "credit_fulldenom"), coin(548, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(500, "debit")]
            );
        }

        #[test]
        fn debt_caps() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Edit initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //1st Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::new(10_000_000),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();          

            ///Successful increase over the cap
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(249_997u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //2nd Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: Some(Uint128::from(1u128)),
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(10_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(83333), cap: Uint128(99998) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(166664), cap: Uint128(199996) }]")
            );

            //Completely withdraw 1st Deposit
            let assets: Vec<Asset> = vec![Asset {
                info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                amount: Uint128::new(10_000_000),
            }];

            let msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets,
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(249996), cap: Uint128(299995) }]")
            );
        }

        #[test]
        fn stability_pool_based_debt_caps() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add general parameters and SupplyCaps
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "double_debit".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
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
                        stability_pool_ratio_for_debt_cap: Some( Decimal::percent(33) ),
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "double_debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(50),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    }
                ]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::new(10_000_000),
                        },
                        Coin {
                            denom: "double_debit".to_string(),
                            amount: Uint128::new(10_000_000),
                        }
                        ],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful debt increase to update basket tally
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();
            
            //Query Basket Debt Caps
            //Debit is based on SP liquidity
            //double_debit is half of total debt cap
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(1000), cap: Uint128(16500) }, DebtCap { collateral: NativeToken { denom: \"double_debit\" }, debt_total: Uint128(1000), cap: Uint128(141747) }]")
            );

        }

        #[test]
        fn bad_debt() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Deposit #1
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("big_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(10_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg)
                .unwrap();

            //Deposit #2
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("little_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(1_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("little_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt for 1 position
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg)
                .unwrap();

            //Query for BadDebt from 1 position w/o debt and 1 position with
            let query_msg = QueryMsg::GetBasketBadDebt { };
            let res: BadDebtResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            //Assert no bad debt
            assert_eq!(res.has_bad_debt, vec![]);
        }

        #[test]
        fn insolvency_checks() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Deposit #1
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("big_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(10_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg)
                .unwrap();

            //Deposit #2
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("little_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(1_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("little_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt for 1 position
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg)
                .unwrap();
            
            //Query the indebted position
            let query_msg = QueryMsg::GetPositionInsolvency {
                position_id: Uint128::new(1),
                position_owner: String::from("big_bank"),
            };
            let res: InsolvencyResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            //Assert no insolvencies
            assert_eq!(
                res.insolvent_positions,
                vec![InsolventPosition {
                    insolvent: false,
                    position_info: UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: String::from("big_bank"),
                    },
                    current_LTV: Decimal::percent(5) * Decimal::percent(10),
                    available_fee: Uint128::zero(),
                }]
            );
        }

        #[test]
        fn two_collateral_cdp_LTV_tests() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "double_debit".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
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
                            denom: "double_debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("big_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(10_000u128),
                        },
                        Coin {
                            denom: "double_debit".to_string(),
                            amount: Uint128::from(10_000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(4999u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg)
                .unwrap();

            //Query for Insolvency
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1),
                position_owner: String::from("big_bank"),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            //Assert LTVs
            assert_eq!(res.avg_borrow_LTV.to_string(), String::from("0.55"));//increased LTV due to supply ratio
            assert_eq!(res.avg_max_LTV.to_string(), String::from("0.65"));
        }

        #[test]
        fn two_collateral_cdp_LTV_tests_bignums() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "double_debit".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
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
                            denom: "double_debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(10_000_000_000_000u128),
                        },
                        Coin {
                            denom: "double_debit".to_string(),
                            amount: Uint128::from(10_000_000_000_000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(249_995u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Query for Insolvency
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1),
                position_owner: String::from("bigger_bank"),
            };
            let res: PositionResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            //Assert LTVs
            assert_eq!(res.avg_borrow_LTV.to_string(), String::from("0.55")); //increased LTV due to supply ratio
            assert_eq!(res.avg_max_LTV.to_string(), String::from("0.65"));
        }

        #[test]
        fn collateral_supply_caps() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Edit initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
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
                        supply_cap_ratio: Decimal::percent(99),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(49),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(49),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(99),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Successful Deposit, even tho over supply cap bc there is no debt so it doesnt count to the cap
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(10_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Errored once debt is taken
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(100u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Supply cap ratio for debit is over the limit (1 > 0.99)\""));

            
            //Successful Deposit, user must deposit both to escape caps
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: Some(Uint128::from(1u128)),
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "lp_denom".to_string(),
                            amount: Uint128::from(10_000_000_000_000_000_000_000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Success even once debt is taken
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps { };
            let res: Vec<DebtCap> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res),
                String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(666), cap: Uint128(99998) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(1333), cap: Uint128(199996) }]")
            );

            //Successful Withdraw uneffected by caps
            let msg = ExecuteMsg::Withdraw {
                position_id: Uint128::new(1u128),
                assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "lp_denom".to_string(),
                    },
                    amount: Uint128::from(10_000_000_000_000u128),
                }],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
        }

        #[test]
        fn multi_collateral_caps() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Edit initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
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
                        supply_cap_ratio: Decimal::percent(50),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(50),
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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: Some(vec![
                    MultiAssetSupplyCap {
                        assets: vec![
                            AssetInfo::NativeToken {denom: "base".to_string()},
                            AssetInfo::NativeToken {denom: "quote".to_string()},
                            ],
                        supply_cap_ratio: Decimal::percent(50),
                    }
                ]),
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Successful Deposit, over supply cap
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(10_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            
            //Successful Deposit, but will error after since over multi-asset cap
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: Some(Uint128::new(1)),
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "base".to_string(),
                            amount: Uint128::from(6_000u128),
                        },
                        Coin {
                            denom: "quote".to_string(),
                            amount: Uint128::from(6_000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            
            //Errored once debt is taken
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Multi-Asset supply cap ratio for [NativeToken { denom: \\\"base\\\" }, NativeToken { denom: \\\"quote\\\" }] is over the limit (0.545454545454545454 > 0.5)\""));

            //Withdraw quote
            let msg = ExecuteMsg::Withdraw { 
                position_id: Uint128::new(1),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::from(6_000u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Deposit to 50% cap
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![
                        Coin {
                            denom: "base".to_string(),
                            amount: Uint128::from(4_000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful debt increase at cap
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();
            
            
        }


        #[test]
        fn LP_oracle() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("lp_tester".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("lp_tester"), cosmos_msg)
                .unwrap();

            //The value of the position should be 200_000_000_000_000_000
            //So at 40% borrow LTV I should be able to borrow 80_000
            //We'll error at the edge first to confirm
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(80_001u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("lp_tester"), cosmos_msg)
                .unwrap_err();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(80_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("lp_tester"), cosmos_msg)
                .unwrap();
        }

        ///Contract Test Migration
        #[test]
        fn misc_query() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);
          
            //Edit Basket 1
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial deposit to Basket 1
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: Some(String::from("sender88")),
                position_id: None,
            };
            app.send_tokens(
                Addr::unchecked("little_bank"),
                Addr::unchecked("sender88"),
                &vec![coin(22, "debit")],
            )
            .unwrap();
            let cosmos_msg = cdp_contract
                .call(exec_msg, vec![coin(11, "debit")])
                .unwrap();
            let res = app
                .execute(Addr::unchecked("sender88"), cosmos_msg)
                .unwrap();

            //Query BasketPositions
            let msg = QueryMsg::GetBasketPositions {
                start_after: Some(String::from("sender88")),
                limit: None,
            };

            let resp: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &msg.clone())
                .unwrap();
            assert_eq!(
                resp.len().to_string(),
                String::from("0"),
            );

            //Query UserPositions
            let msg = QueryMsg::GetUserPositions { 
                user: String::from("sender88"), 
                limit: None, 
            };
            let resp: Vec<PositionResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &msg.clone())
                .unwrap();
            assert_eq!(
                resp[0],
                PositionResponse { 
                    position_id: Uint128::new(1), 
                    collateral_assets: vec![
                        cAsset {
                            asset: Asset {
                                info: AssetInfo::NativeToken {
                                    denom: "debit".to_string(),
                                },
                                amount: Uint128::from(11u128),
                            },
                            max_borrow_LTV: Decimal::percent(50),
                            max_LTV: Decimal::percent(70),
                            pool_info: None,
                            rate_index: Decimal::one(),
                        }
                    ], 
                    cAsset_ratios: vec![Decimal::one()], 
                    credit_amount: Uint128::zero(), 
                    basket_id: Uint128::new(1), 
                    avg_borrow_LTV: Decimal::percent(50),
                    avg_max_LTV: Decimal::percent(70),
                },
            );

            //Update Config
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: Some(String::from("new_owner")), 
                stability_pool: Some(String::from("new_sp")), 
                dex_router: Some(String::from("new_router")),  
                osmosis_proxy: Some(String::from("new_op")),  
                debt_auction: Some(String::from("new_auction")),  
                staking_contract: Some(String::from("new_staking")),  
                oracle_contract: Some(String::from("new_oracle")),  
                liquidity_contract: Some(String::from("new_liq_check")),
                discounts_contract: Some( String::from("new_dc")),
                liq_fee: Some(Decimal::percent(13)), 
                debt_minimum: Some(Uint128::zero()), 
                base_debt_cap_multiplier: Some(Uint128::new(48497)), 
                oracle_time_limit: Some(33u64), 
                credit_twap_timeframe: Some(33u64), 
                collateral_twap_timeframe: Some(33u64), 
                cpc_multiplier: Some(Decimal::percent(50)),
                rate_slope_multiplier: Some(Decimal::percent(2)), 
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            let resp: Config = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {  }.clone())
            .unwrap();

            assert_eq!(
                resp,
                Config { 
                    owner: Addr::unchecked(ADMIN), 
                    stability_pool: Some( Addr::unchecked("new_sp")), 
                    dex_router: Some( Addr::unchecked("new_router")),  
                    osmosis_proxy: Some( Addr::unchecked("new_op")),  
                    debt_auction: Some( Addr::unchecked("new_auction")),  
                    staking_contract: Some( Addr::unchecked("new_staking")),  
                    oracle_contract: Some( Addr::unchecked("new_oracle")),  
                    liquidity_contract: Some( Addr::unchecked("new_liq_check")),
                    discounts_contract: Some( Addr::unchecked("new_dc")),
                    liq_fee: Decimal::percent(13), 
                    debt_minimum: Uint128::zero(), 
                    base_debt_cap_multiplier: Uint128::new(48497), 
                    oracle_time_limit: 33u64, 
                    credit_twap_timeframe: 33u64, 
                    collateral_twap_timeframe: 33u64, 
                    cpc_multiplier: Decimal::percent(50),
                    rate_slope_multiplier: Decimal::percent(2), 
                }
            );

            //Update owner after new owner calls the function
            //Update Config
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
                owner: None, 
                stability_pool: None, 
                dex_router: None, 
                osmosis_proxy: None, 
                debt_auction: None, 
                staking_contract: None, 
                oracle_contract: None, 
                liquidity_contract: None, 
                discounts_contract: None, 
                liq_fee: None, 
                debt_minimum: None, 
                base_debt_cap_multiplier: None, 
                oracle_time_limit: None, 
                credit_twap_timeframe: None, 
                collateral_twap_timeframe: None, 
                cpc_multiplier: None, 
                rate_slope_multiplier: Some(Decimal::percent(3)), 
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("new_owner"), cosmos_msg).unwrap();

            let resp: Config = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {  }.clone())
            .unwrap();

            assert_eq!(
                resp,
                Config { 
                    owner: Addr::unchecked("new_owner"), 
                    stability_pool: Some( Addr::unchecked("new_sp")), 
                    dex_router: Some( Addr::unchecked("new_router")),  
                    osmosis_proxy: Some( Addr::unchecked("new_op")),  
                    debt_auction: Some( Addr::unchecked("new_auction")),  
                    staking_contract: Some( Addr::unchecked("new_staking")),  
                    oracle_contract: Some( Addr::unchecked("new_oracle")),  
                    liquidity_contract: Some( Addr::unchecked("new_liq_check")),
                    discounts_contract: Some( Addr::unchecked("new_dc")),
                    liq_fee: Decimal::percent(13), 
                    debt_minimum: Uint128::zero(), 
                    base_debt_cap_multiplier: Uint128::new(48497), 
                    oracle_time_limit: 33u64, 
                    credit_twap_timeframe: 33u64, 
                    collateral_twap_timeframe: 33u64, 
                    cpc_multiplier: Decimal::percent(50),
                    rate_slope_multiplier: Decimal::percent(3), 
                }
            );

        }

        #[test]
        fn edit_cAsset() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            //Invalid Asset
            let edit_msg = ExecuteMsg::EditcAsset {
                asset: AssetInfo::NativeToken {
                    denom: "not_debit".to_string(),
                },
                max_borrow_LTV: None,
                max_LTV: None,
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Successfull edit
            let edit_msg = ExecuteMsg::EditcAsset {
                asset: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                max_borrow_LTV: Some(Decimal::percent(82)),
                max_LTV: Some(Decimal::percent(83)),
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Query Basket
            let resp: Basket = app
                .wrap()
                .query_wasm_smart(
                    cdp_contract.addr(),
                    &QueryMsg::GetBasket { },
                )
                .unwrap();

            assert_eq!(
                resp.collateral_types[0].max_borrow_LTV,
                Decimal::percent(82)
            );
            assert_eq!(resp.collateral_types[0].max_LTV, Decimal::percent(83));

            //Error: Borrow LTV too high
            let edit_msg = ExecuteMsg::EditcAsset {
                asset: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                max_borrow_LTV: Some(Decimal::percent(100)),
                max_LTV: Some(Decimal::percent(100)),
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

            //Successfull edit
            let edit_msg = ExecuteMsg::EditcAsset {
                asset: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                max_borrow_LTV: None,
                max_LTV: Some(Decimal::percent(100)),
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
        }

        #[test]
        fn open_position_deposit() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            //Add supply caps and a new cAsset (2nddebit)
            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                    rate_index: Decimal::one(),
                }),
                liq_queue: None,
                credit_pool_infos: None,
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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Testing Position creation

            //Invalid id test
            let error_exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: Some(Uint128::from(3u128)),
            };

            //Fail due to a non-existent position
            //First msg deposits since no positions were initially found, meaning the _id never got tested
            app.send_tokens(
                Addr::unchecked("little_bank"),
                Addr::unchecked("owner"),
                &vec![coin(22, "debit")],
            )
            .unwrap();
            let cosmos_msg = cdp_contract
                .call(error_exec_msg, vec![coin(11, "debit")])
                .unwrap();
            app.execute(Addr::unchecked("owner"), cosmos_msg.clone())
                .unwrap();
            app.execute(Addr::unchecked("owner"), cosmos_msg)
                .unwrap_err();

            //Fail due to invalid collateral
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg, vec![coin(666, "fake_debit")])
                .unwrap();
            app.execute(Addr::unchecked("faker"), cosmos_msg)
                .unwrap_err();

            //Successful attempt
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            app.send_tokens(
                Addr::unchecked(USER),
                Addr::unchecked("owner"),
                &vec![coin(11, "2nddebit"), coin(11, "debit")],
            )
            .unwrap();
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(11, "debit"), coin(11, "2nddebit")])
                .unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg).unwrap();

            let response = res
                .events
                .into_iter()
                .find(|e| e.attributes.iter().any(|attr| attr.key == "method" && attr.value == "deposit"))
                .ok_or_else(|| panic!("unable to find deposit event"))
                .unwrap();

            assert_eq!(
                response.attributes[1..4],
                vec![
                    attr("method", "deposit"),
                    attr("position_owner", "owner"),
                    attr("position_id", "2"),
                ]
            );
            assert_eq!(response.attributes[4].value, String::from("[Asset { info: NativeToken { denom: \"debit\" }, amount: Uint128(11) }, Asset { info: NativeToken { denom: \"2nddebit\" }, amount: Uint128(11) }]") );

            //Test max Position amount
            let cosmos_msg = cdp_contract
                .call(exec_msg, vec![coin(1, "debit")])
                .unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap();
            //This one should fail
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap_err();
        }

        #[test]
        fn repay() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //NoUserPositions Error
            let repay_msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract.call(repay_msg, vec![]).unwrap();
            let res = app
                .execute(Addr::unchecked("sender88"), cosmos_msg)
                .unwrap_err();

            //Initial deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, coins(11, "debit")).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Invalid Collateral Error
            let repay_msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: Some(USER.to_string()),
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(repay_msg, vec![coin(666, "fake_debit")])
                .unwrap();
            let res = app
                .execute(Addr::unchecked("faker"), cosmos_msg)
                .unwrap_err();

            //NonExistent Position Error
            let repay_msg = ExecuteMsg::Repay {
                position_id: Uint128::from(3u128),
                position_owner: Some(USER.to_string()),
                send_excess_to: None,
            };
            let cosmos_msg = cdp_contract
                .call(repay_msg, vec![coin(111, "credit_fulldenom")])
                .unwrap();
            let res = app
                .execute(Addr::unchecked("coin_God"), cosmos_msg)
                .unwrap_err();
        }

        #[test]
        fn increase_debt() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //NoUserPositions Error
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(1u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

            //Initial deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg, vec![coin(11_000, "debit")])
                .unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //NonExistentPosition Error
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(3u128),
                amount: Some(Uint128::from(1u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Increase_debt by LTV: Insolvent Error
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: None,
                LTV: Some(Decimal::percent(100)),
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Increase_debt by LTV: No amount inputs
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: None,
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Increase_debt by LTV: Success
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: None,
                LTV: Some(Decimal::percent(40)),
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

           //Query indebted position
           let query_msg = QueryMsg::GetPositionInsolvency {
            position_id: Uint128::new(1),
            position_owner: String::from(USER),
            };
            let res: InsolvencyResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            //Assert LTV
            assert_eq!(
                res.insolvent_positions,
                vec![InsolventPosition {
                    insolvent: false,
                    position_info: UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: String::from(USER),
                    },
                    current_LTV: Decimal::percent(40),
                    available_fee: Uint128::zero(),
                }]
            );
        }

        #[test]
        fn withdrawal_errors() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
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
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            let valid_assets: Vec<Asset> = vec![Asset {
                info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                amount: Uint128::from(5u128),
            }];

            //User has no positions in the basket error
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: valid_assets.clone(),
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

            //Initial Deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, coins(11, "debit")).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Non-existent position error but user still has positions in the basket
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(3u128),
                assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    amount: Uint128::zero(),
                }],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Invalid collateral fail
            let assets: Vec<Asset> = vec![Asset {
                info: AssetInfo::NativeToken {
                    denom: "notdebit".to_string(),
                },
                amount: Uint128::from(10u128),
            }];

            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: assets.clone(),
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Withdrawing too much error
            let assets: Vec<Asset> = vec![Asset {
                info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                amount: Uint128::from(333333333u128),
            }];

            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets,
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn asset_expunge(){
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            //Add supply caps & a 2nd cAsset
            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset:  Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                    rate_index: Decimal::one(),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: Some(vec![SupplyCap {
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
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();           

            //Initial deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, vec![coin(11, "debit"), coin(11, "2nddebit")]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Set debit supply caps to 0 
            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(0),
                    lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap(); 

            //Attempt to withdraw 2nddebit only: Error
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(11u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Attempt to withdraw both but not debit fully: Error
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(11u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(1u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            
            //Withdraw only debit partially: Successful
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(5u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Withdraw both: Successful
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(11u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(6u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query Position assets to assert full withdrawal
            let query_msg = QueryMsg::GetPosition {
                position_id: Uint128::new(1u128),
                position_owner: USER.to_string(),
            };
            app
                .wrap()
                .query_wasm_smart::<PositionResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap_err();
        }

        #[test]
        #[allow(dead_code)]
        fn close_position(){
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

                let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let sp_addr = res.stability_pool.unwrap();
            let router_addr = res.dex_router.unwrap();
            let staking_contract = res.staking_contract.unwrap();
            
            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            //Add LP asset
            //Set supply caps
            //Set general parameters
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            //Current Position: 100_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase
            //Current Position: 100_000_000_000_000_000 lp_denom -> 100_000 credit_fulldenom: 50% LTV
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(100_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Close Position: Unauthorized
            let msg = ExecuteMsg::ClosePosition { 
                position_id: Uint128::from(1u128),
                max_spread: Decimal::percent(1),
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("smaller_bank"), cosmos_msg)
                .unwrap_err();

            //Send assets to mimic LP split
            app.send_tokens(
                Addr::unchecked("bigger_bank"),
                cdp_contract.addr(),
                &[coin(100_000, "base"), coin(100_000, "quote"), coin(100_000, "credit_fulldenom")],
            ).unwrap();

            //Close Position: Make sure huge spread doesn't over sell
            let msg = ExecuteMsg::ClosePosition { 
                position_id: Uint128::from(1u128),
                max_spread: Decimal::percent(10000_00),
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            //Execute
            let res = app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Assert Position was deleted after Closing
            app
                .wrap()
                .query_wasm_smart::<PositionResponse>(&cdp_contract.addr(), &QueryMsg::GetPosition { 
                    position_id: Uint128::one(), 
                    position_owner: String::from("bigger_bank")
                })
                .unwrap_err();

            //Initial Deposit
            //Current Position: 100_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("bigger_bank".to_string()),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase
            //Current Position: 100_000_000_000_000_000 lp_denom -> 100_000 credit_fulldenom: 50% LTV
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(100_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();

            //Send assets to mimic LP split
            app.send_tokens(
                Addr::unchecked("bigger_bank"),
                cdp_contract.addr(),
                &[coin(100_000, "base"), coin(100_000, "quote"), coin(100_000, "credit_fulldenom")],
            ).unwrap();

            //Close Position: Success.
            let msg = ExecuteMsg::ClosePosition { 
                position_id: Uint128::from(2u128),
                max_spread: Decimal::percent(1),
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            //Execute
            let res = app.execute(Addr::unchecked("bigger_bank"), cosmos_msg)
                .unwrap();
            //Assert Position was deleted after Closing
            app
                .wrap()
                .query_wasm_smart::<PositionResponse>(&cdp_contract.addr(), &QueryMsg::GetPosition { 
                    position_id: Uint128::new(2), 
                    position_owner: String::from("bigger_bank")
                })
                .unwrap_err();
            
        }

        #[test]
        fn edit_redemption_info(){
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            //Add supply caps and a new cAsset (2nddebit)
            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                    rate_index: Decimal::one(),
                }),
                liq_queue: None,
                credit_pool_infos: None,
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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Successful deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000, "debit")])
                .unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Successful deposit: #2
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000, "2nddebit")])
                .unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt: #2
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(50_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            ////Set Redemption Info///
            //Error: Premium too high
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![], 
                redeemable: Some(true), 
                premium: Some(100), 
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Premium can't be greater than 99\""));
            
            //Error: Loan repayment too high
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![], 
                redeemable: Some(true), 
                premium: Some(99), 
                max_loan_repayment: Some(Decimal::percent(101)),
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Max loan repayment can't be greater than 100%\""));

            //Success, set redeemable
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: Some(true), 
                premium: Some(10), 
                max_loan_repayment: None, 
                restricted_collateral_assets: Some(vec![String::from("debit")]),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query Basket Redeemability
            let query_msg = QueryMsg::GetBasketRedeemability { 
                position_owner: None,
                start_after: None, 
                limit: None 
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].remaining_loan_repayment, Uint128::new(50_000));
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].position_id, Uint128::one());
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].restricted_collateral_assets, vec![String::from("debit")]);
            assert_eq!(res.premium_infos[0].premium, 10u128);
            assert_eq!(res.premium_infos[0].users_of_premium.len(), 1);
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos.len(), 1);
            //Success, turn restrictions off
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: None,
                premium: None,
                max_loan_repayment: None, 
                restricted_collateral_assets: Some(vec![]),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query Basket Redeemability
            let query_msg = QueryMsg::GetBasketRedeemability { 
                position_owner: Some(USER.to_string()),
                start_after: None, 
                limit: None
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].restricted_collateral_assets.len(), 0);

            //////Edit Redeemability/////
            //Error: Not the owner of said position or it doesnt exist in the User's redemption list
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::zero()], 
                redeemable: None, 
                premium: Some(2),
                max_loan_repayment: Some(Decimal::one()), 
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Generic error: User does not own position id: 0"));
            
            //No ID specified
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![], 
                redeemable: None, 
                premium: None,
                max_loan_repayment: None, 
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Position IDs must be specified\""));
            
            //Repetitive IDs
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one(), Uint128::one()], 
                redeemable: None, 
                premium: None,
                max_loan_repayment: None, 
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Position IDs must be unique\""));           

            //Toggle Redeemability (to and fro)
            //Off
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: Some(false), 
                premium: None,
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Assert Redeemability change
            let query_msg = QueryMsg::GetBasketRedeemability { 
                position_owner: None,
                start_after: None,
                limit: None 
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            //None are added to the Response bc there are no premiums that aren't empty
            assert_eq!(res.premium_infos.len(), 0);
            //Error bc no premium set when setting to TRUE
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: Some(true), 
                premium: None,
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //On
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: Some(true), 
                premium: Some(10),
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Assert Redeemability change
            let query_msg = QueryMsg::GetBasketRedeemability { 
                position_owner: None,
                start_after: None, 
                limit: None 
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.premium_infos[0].users_of_premium.len(), 1);


            //Only Edit Premium 1 and Set premium of Position 2
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one(), Uint128::new(2)], 
                redeemable: None,
                premium: Some(20),
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Assert Redeemability change
            let query_msg = QueryMsg::GetBasketRedeemability { 
                position_owner: None,
                start_after: None, 
                limit: None 
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            
            assert_eq!(res.premium_infos.len(), 1);
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].position_id, Uint128::one());
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[1].position_id, Uint128::new(2));
            assert_eq!(res.premium_infos[0].premium, 20u128);

            ////Add a position to an existing user premium list///
            //turn redeemability Off for 1
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: Some(false), 
                premium: None,
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: None,
                premium: Some(20),
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            ////Edit Max Loan Repayment && restricted_assets///
            //Error: Invalid restriciton
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one(), Uint128::new(2)], 
                redeemable: None,
                premium: None,
                max_loan_repayment: Some(Decimal::percent(50)),
                restricted_collateral_assets: Some(
                    vec![String::from("I_don't_want_this_asset")]
                ),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Invalid restricted asset, only collateral assets are viable to restrict\""));           
            //Success
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one(), Uint128::new(2)], 
                redeemable: None,
                premium: None,
                max_loan_repayment: Some(Decimal::percent(50)),
                restricted_collateral_assets: Some(
                    vec![String::from("debit")]
                ),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Assert Redeemability change
            let query_msg = QueryMsg::GetBasketRedeemability { 
                position_owner: None,
                start_after: None, 
                limit: None 
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].remaining_loan_repayment, Uint128::new(25000));
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[1].remaining_loan_repayment, Uint128::new(25000));
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].restricted_collateral_assets, vec![String::from("debit")]);
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[1].restricted_collateral_assets, vec![String::from("debit")]);
        }

        #[test]
        fn redemption_w_multiple_premiums(){
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            //Add supply caps and a new cAsset (2nddebit)
            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                    rate_index: Decimal::one(),
                }),
                liq_queue: None,
                credit_pool_infos: None,
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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Successful deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000, "debit")])
                .unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Successful deposit: #2
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000, "2nddebit")])
                .unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt: #2
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(50_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Set #1 to 10% premium
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: Some(true), 
                premium: Some(10),
                max_loan_repayment: Some(Decimal::percent(10)),
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Set #2 to 20% premium
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::new(2)], 
                redeemable: Some(true), 
                premium: Some(20),
                max_loan_repayment: Some(Decimal::percent(20)),
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            ////Redeem///// (15k max currently)
            //Error: Wrong asset
            let redemption_msg = ExecuteMsg::RedeemCollateral { max_collateral_premium: None };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![coin(1, "not_redeemable")]).unwrap();
            let err = app.execute(Addr::unchecked("redeemer"), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Must send only the debt token: credit_fulldenom\""));           


            //Success, but Send too much (15k max currently)
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![coin(100_000, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked("redeemer"), cosmos_msg).unwrap();
            //Assert that the collateral & excess was sent to the redeemer
            assert_eq!(
                app.wrap().query_all_balances(Addr::unchecked("redeemer")).unwrap(),
                vec![
                    coin(8000, "2nddebit"),
                    coin(85000, "credit_fulldenom"),  
                    coin(4500, "debit"), 
                    coin(1, "not_redeemable")]
            );

            //Assert Positions were updated
            let position: PositionResponse = app
                .wrap()
                .query_wasm_smart(&cdp_contract.addr(), &QueryMsg::GetPosition { 
                    position_id: Uint128::one(), 
                    position_owner: String::from(USER)
                })
                .unwrap();
            assert_eq!(position.collateral_assets[0].asset.amount, Uint128::new(95_500));
            assert_eq!(position.credit_amount, Uint128::new(45000));

            let position_2: PositionResponse = app
                .wrap()
                .query_wasm_smart(&cdp_contract.addr(), &QueryMsg::GetPosition { 
                    position_id: Uint128::new(2), 
                    position_owner: String::from(USER)
                })
                .unwrap();
            assert_eq!(position_2.collateral_assets[0].asset.amount, Uint128::new(92_000));
            assert_eq!(position_2.credit_amount, Uint128::new(40000));
        }

        //Redemption test with multiple collateral in the position (debit & LP bc they r priced differently)
        //Tests max premium as well
        #[test]
        fn redemption_w_multiple_collateral(){
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add supply caps and a new cAsset (lp_denom)            
            let msg = ExecuteMsg::EditBasket(EditBasket {
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
                }),
                liq_queue: None,
                credit_pool_infos: None,
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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Send lp_denom
            app.send_tokens(
                Addr::unchecked("bigger_bank"),
                Addr::unchecked(USER),
                &[coin(100_000_000_000_000_000, "lp_denom")],
            )
            .unwrap();

            //Successful deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000, "debit"), coin(100_000_000_000_000_000, "lp_denom")])
                .unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000u128)),
                LTV: None,
                mint_to_addr: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Set #1 to 10% premium
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: Some(true), 
                premium: Some(10),
                max_loan_repayment: Some(Decimal::percent(10)),
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            ////Redeem from a multi-collateral position///
            let redemption_msg = ExecuteMsg::RedeemCollateral { max_collateral_premium: Some(9) };          
            //Error: Nothing redeemed since premium is too low
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![coin(100_000, "credit_fulldenom")]).unwrap();
            let err = app.execute(Addr::unchecked("redeemer"), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"No collateral to redeem with a max premium of: 9\""));           

            //Successful redemption
            let redemption_msg = ExecuteMsg::RedeemCollateral { max_collateral_premium: Some(10) };          
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![coin(100_000, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked("redeemer"), cosmos_msg).unwrap();
            //Assert that the collateral & excess was sent to the redeemer
            assert_eq!(
                app.wrap().query_all_balances(Addr::unchecked("redeemer")).unwrap(),
                vec![
                    coin(95000, "credit_fulldenom"), //excess
                    coin(1499, "debit"),   //user pays 4500 for it (1499)
                    coin(1499_999_999_999_999, "lp_denom"), //(1499 * 2)
                    coin(1, "not_redeemable")]
            );

            //Assert Positions were updated
            let position: PositionResponse = app
                .wrap()
                .query_wasm_smart(&cdp_contract.addr(), &QueryMsg::GetPosition { 
                    position_id: Uint128::one(), 
                    position_owner: String::from(USER)
                })
                .unwrap();
            assert_eq!(position.collateral_assets[0].asset.amount, Uint128::new(98_501));
            assert_eq!(position.collateral_assets[1].asset.amount, Uint128::new(98_500_000_000_000_001));
            assert_eq!(position.credit_amount, Uint128::new(45000));  

            //Assert remaining loan repayment is 0'd
            let query_msg = QueryMsg::GetBasketRedeemability { 
                position_owner: None,
                start_after: None, 
                limit: None 
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            //Bc the remaining loan repayment is 0'd, the position is not redeemable
            assert_eq!(res.premium_infos, vec![]);

            //Reset #1's loan repayment cap but restrict the collateral to only debit
            let redemption_msg = ExecuteMsg::EditRedeemability { 
                position_ids: vec![Uint128::one()], 
                redeemable: Some(true),
                premium: Some(10),
                max_loan_repayment: Some(Decimal::percent(10)),
                restricted_collateral_assets: Some(vec![String::from("lp_denom")]),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert the reset loan repayment cap
            let query_msg = QueryMsg::GetBasketRedeemability { 
                position_owner: None,
                start_after: None, 
                limit: None 
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].remaining_loan_repayment, Uint128::new(4500));

            //Successful restricted redemption
            let redemption_msg = ExecuteMsg::RedeemCollateral { max_collateral_premium: None };          
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![coin(4_000, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked("redeemer"), cosmos_msg).unwrap();
            //Assert that the collateral & excess was sent to the redeemer
            assert_eq!(
                app.wrap().query_all_balances(Addr::unchecked("redeemer")).unwrap(),
                vec![
                    coin(91000, "credit_fulldenom"),  
                    coin(5099, "debit"), //5850 - 2250 = 3600 new debit redeemed
                    coin(1499_999_999_999_999, "lp_denom"), 
                    coin(1, "not_redeemable")]
            );

            //Assert Positions were updated
            let position: PositionResponse = app
                .wrap()
                .query_wasm_smart(&cdp_contract.addr(), &QueryMsg::GetPosition { 
                    position_id: Uint128::one(), 
                    position_owner: String::from(USER)
                })
                .unwrap();
            assert_eq!(position.collateral_assets[0].asset.amount, Uint128::new(94_901));
            assert_eq!(position.collateral_assets[1].asset.amount, Uint128::new(98_500_000_000_000_001));
            assert_eq!(position.credit_amount, Uint128::new(41000));  

            //Assert remaining loan repayment is updated from a partial full
            let query_msg = QueryMsg::GetBasketRedeemability { 
                position_owner: None,
                start_after: None, 
                limit: None 
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].remaining_loan_repayment, Uint128::new(500));
        }
    }
}
