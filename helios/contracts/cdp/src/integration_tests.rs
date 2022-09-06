#[cfg(test)]
mod tests {
    
    use crate::helpers::{ LQContract, CDPContract };
        
    use cosmwasm_bignumber::Uint256;
    use cw20::BalanceResponse;
    use membrane::oracle::{PriceResponse, AssetResponse};
    use membrane::positions::{ InstantiateMsg, QueryMsg, ExecuteMsg };
    use membrane::liq_queue::{ LiquidatibleResponse as LQ_LiquidatibleResponse};
    use membrane::stability_pool::{ LiquidatibleResponse as SP_LiquidatibleResponse, PoolResponse };
    use membrane::osmosis_proxy::{ GetDenomResponse };
    use membrane::types::{AssetInfo, Asset, cAsset, LiqAsset, TWAPPoolInfo, AssetOracleInfo};

    
    use osmo_bindings::{ SpotPriceResponse, PoolStateResponse, ArithmeticTwapToNowResponse };
    use cosmwasm_std::{Addr, Coin, Empty, Uint128, Decimal, Response, StdResult, Binary, to_binary, coin, attr, StdError };
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor, BankKeeper};
    use schemars::JsonSchema;
    use serde::{ Deserialize, Serialize };


    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //CDP Contract
    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        ).with_reply(crate::contract::reply);
        Box::new(contract)
    }

    //Mock LQ Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum LQ_MockExecuteMsg {
        Liquidate {
            credit_price: Decimal, //Sent from Position's contract
            collateral_price: Decimal, //Sent from Position's contract
            collateral_amount: Uint256,
            bid_for: AssetInfo,
            bid_with: AssetInfo,   
            basket_id: Uint128,
            position_id: Uint128,
            position_owner: String, 
        }, 
        AddQueue{    
            bid_for: AssetInfo,
            bid_asset: AssetInfo,
            max_premium: Uint128,
            bid_threshold: Uint256,
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
        }
    }


    pub fn liq_queue_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price,
                        collateral_price,
                        collateral_amount,
                        bid_for,
                        bid_with,
                        basket_id,
                        position_id,
                        position_owner,
                    } => {
                        
                        match bid_for{
                            AssetInfo::Token { address: _ } => {
                                
                                return Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("denom", bid_with.to_string()),
                                    attr("repay_amount", Uint128::new(2_000u128).to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "token"),
                                    attr("collateral_amount", collateral_amount),
                                ]))
                            },
                            
                            AssetInfo::NativeToken { denom: _ } => {
                                
                                if collateral_amount.to_string() != String::from("2000") && collateral_amount.to_string() != String::from("22000") && collateral_amount.to_string() != String::from("4222"){
                                    panic!("{}", collateral_amount.to_string());
                                }


                                return Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("denom", bid_with.to_string()),
                                    attr("repay_amount", Uint128::new(2_000u128).to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "native_token"),
                                    attr("collateral_amount", collateral_amount),
                                ]))
                            }
                        }
                    }, 
                    LQ_MockExecuteMsg::AddQueue { 
                        bid_for, 
                        bid_asset, 
                        max_premium, 
                        bid_threshold 
                    } => {
                        Ok( Response::new() )
                    },
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible { 
                        bid_for, 
                        collateral_price, 
                        collateral_amount, 
                        credit_info, 
                        credit_price 
                    } => Ok(
                        to_binary(
                            &LQ_LiquidatibleResponse {
                                leftover_collateral: "222".to_string(),
                                total_credit_repaid: "2000".to_string(),
                            })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn liq_queue_contract_bignums()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price,
                        collateral_price,
                        collateral_amount,
                        bid_for,
                        bid_with,
                        basket_id,
                        position_id,
                        position_owner,
                    } => {
                        
                        match bid_for{
                            AssetInfo::Token { address: _ } => {
                                
                                return Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("denom", bid_with.to_string()),
                                    attr("repay_amount", Uint128::new(2_000_000_000_000u128).to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "token"),
                                    attr("collateral_amount", collateral_amount),
                                ]))
                            },
                            
                            AssetInfo::NativeToken { denom: _ } => {
                                
                                // if collateral_amount.to_string() != String::from("2000") && collateral_amount.to_string() != String::from("22000") && collateral_amount.to_string() != String::from("4222"){
                                //     panic!("{}", collateral_amount.to_string());
                                // }


                                return Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("denom", bid_with.to_string()),
                                    attr("repay_amount", Uint128::new(2_000_000_000_000u128).to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "native_token"),
                                    attr("collateral_amount", collateral_amount),
                                ]))
                            }
                        }
                    },
                    LQ_MockExecuteMsg::AddQueue { 
                        bid_for, 
                        bid_asset, 
                        max_premium, 
                        bid_threshold 
                    } => {
                        Ok( Response::new() )
                    },
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible { 
                        bid_for, 
                        collateral_price, 
                        collateral_amount, 
                        credit_info, 
                        credit_price 
                    } => Ok(
                        to_binary(
                            &LQ_LiquidatibleResponse {
                                leftover_collateral: "222222222225".to_string(),
                                total_credit_repaid: "2000000000000".to_string(),
                            })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn liq_queue_contract_errors()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price,
                        collateral_price,
                        collateral_amount,
                        bid_for,
                        bid_with,
                        basket_id,
                        position_id,
                        position_owner,
                    } => {
                        Err( StdError::GenericErr { msg: "no siree".to_string() })
                    },
                    LQ_MockExecuteMsg::AddQueue { 
                        bid_for, 
                        bid_asset, 
                        max_premium, 
                        bid_threshold 
                    } => {
                        Ok( Response::new() )
                    },
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible { 
                        bid_for, 
                        collateral_price, 
                        collateral_amount, 
                        credit_info, 
                        credit_price 
                    } => Ok(
                        to_binary(
                            &LQ_LiquidatibleResponse {
                                leftover_collateral: "222".to_string(),
                                total_credit_repaid: "2000".to_string(),
                            })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn liq_queue_contract_minimumliq()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price,
                        collateral_price,
                        collateral_amount,
                        bid_for,
                        bid_with,
                        basket_id,
                        position_id,
                        position_owner,
                    } => {
                        
                        match bid_for{
                            AssetInfo::Token { address: _ } => {
                                
                                return Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("denom", bid_with.to_string()),
                                    attr("repay_amount", Uint128::new(500u128).to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "token"),
                                    attr("collateral_amount", collateral_amount),
                                ]))
                            },
                            
                            AssetInfo::NativeToken { denom: _ } => {
                                
                                return Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("denom", bid_with.to_string()),
                                    attr("repay_amount", Uint128::new(500u128).to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "native_token"),
                                    attr("collateral_amount", collateral_amount),
                                ]))
                            }
                        }
                    },
                    LQ_MockExecuteMsg::AddQueue { 
                        bid_for, 
                        bid_asset, 
                        max_premium, 
                        bid_threshold 
                    } => {
                        Ok( Response::new() )
                    },
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible { 
                        bid_for, 
                        collateral_price, 
                        collateral_amount, 
                        credit_info, 
                        credit_price 
                    } => Ok(
                        to_binary(
                            &LQ_LiquidatibleResponse {
                                leftover_collateral: "499".to_string(),
                                total_credit_repaid: "500".to_string(),
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
        Liquidate {
            credit_asset: LiqAsset, 
        },
        Distribute {
            distribution_assets: Vec<Asset>,
            distribution_asset_ratios: Vec<Decimal>,
            credit_asset: AssetInfo,
            distribute_for: Uint128,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct SP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum SP_MockQueryMsg {
        CheckLiquidatible {
            asset: LiqAsset
        },
        AssetPool {
            asset_info: AssetInfo 
        },
    }

    pub fn stability_pool_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate {
                        credit_asset
                    } => {
                        if credit_asset.to_string() != "222.222225 credit_fulldenom".to_string() && credit_asset.to_string() != "2000 credit_fulldenom".to_string() && credit_asset.to_string() != "22222.22225 credit_fulldenom".to_string() && credit_asset.to_string() != "20222.22225 credit_fulldenom".to_string(){
                            panic!("{}", credit_asset.to_string());
                        }
                        
                        Ok(Response::new()
                            .add_attribute("method", "liquidate")
                            .add_attribute("leftover_repayment", "0"))
                    }
                    SP_MockExecuteMsg::Distribute { 
                        distribution_assets,
                        distribution_asset_ratios, 
                        credit_asset, 
                        distribute_for } => {
                        
                        if distribution_assets != vec![
                                        Asset { 
                                            info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                                            amount: Uint128::new(244) 
                                        }]
                            && distribution_assets != vec![
                                Asset { 
                                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                                    amount: Uint128::new(2447) 
                                }]
                            &&
                            distribution_assets != vec![
                                Asset { 
                                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                                    amount: Uint128::new(55000) 
                                }]{
                                            assert_ne!(distribution_assets, distribution_assets);
                                        }

                        Ok(Response::new()
                            .add_attribute("method", "distribute")
                            .add_attribute("credit_asset", "cdl"))
                    },
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { 
                        asset,
                    } => Ok(
                        to_binary(
                            &SP_LiquidatibleResponse {
                                leftover: Decimal::zero(),
                            })?),
                    SP_MockQueryMsg::AssetPool { asset_info 
                    } => Ok(
                        to_binary(&PoolResponse {
                            credit_asset: Asset {
                                info: AssetInfo::NativeToken { denom: "cdl".to_string() },
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

    pub fn stability_pool_contract_bignums()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate {
                        credit_asset
                    } => {
                        // if credit_asset.to_string() != "222.222225 credit_fulldenom".to_string() && credit_asset.to_string() != "2000 credit_fulldenom".to_string() && credit_asset.to_string() != "22222.22225 credit_fulldenom".to_string() && credit_asset.to_string() != "20222.22225 credit_fulldenom".to_string(){
                        //     panic!("{}", credit_asset.to_string());
                        // }
                        
                        Ok(Response::new()
                            .add_attribute("method", "liquidate")
                            .add_attribute("leftover_repayment", "0"))
                    }
                    SP_MockExecuteMsg::Distribute { 
                        distribution_assets,
                        distribution_asset_ratios, 
                        credit_asset, 
                        distribute_for } => {
                        
                        // if distribution_assets != vec![
                        //                 Asset { 
                        //                     info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        //                     amount: Uint128::new(244) 
                        //                 }]
                        //     && distribution_assets != vec![
                        //         Asset { 
                        //             info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        //             amount: Uint128::new(2447) 
                        //         }]
                        //     &&
                        //     distribution_assets != vec![
                        //         Asset { 
                        //             info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        //             amount: Uint128::new(55000) 
                        //         }]{
                        //                     assert_ne!(distribution_assets, distribution_assets);
                        //                 }

                        Ok(Response::new()
                            .add_attribute("method", "distribute")
                            .add_attribute("credit_asset", "cdl"))
                    },
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { 
                        asset,
                    } => Ok(
                        to_binary(
                            &SP_LiquidatibleResponse {
                                leftover: Decimal::zero(),
                            })?),
                    SP_MockQueryMsg::AssetPool { asset_info 
                    } => Ok(
                        to_binary(&PoolResponse {
                            credit_asset: Asset {
                                info: AssetInfo::NativeToken { denom: "cdl".to_string() },
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

    pub fn stability_pool_contract_errors()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate {
                        credit_asset
                    } => {
                        
                        Err( StdError::GenericErr { msg: "no siree".to_string() })
                    }
                    SP_MockExecuteMsg::Distribute { 
                        distribution_assets, 
                        distribution_asset_ratios,
                        credit_asset, 
                        distribute_for } => {

                                                
                        Ok(Response::new()
                            .add_attribute("method", "distribute")
                            .add_attribute("credit_asset", "cdl"))
                    },
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { 
                        asset,
                    } => Ok(
                        to_binary(
                            &SP_LiquidatibleResponse {
                                leftover: Decimal::zero(),
                            })?),
                    SP_MockQueryMsg::AssetPool { asset_info 
                    } => Ok(
                        to_binary(&PoolResponse {
                            credit_asset: Asset {
                                info: AssetInfo::NativeToken { denom: "cdl".to_string() },
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

    pub fn stability_pool_contract_minimumliq()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate {
                        credit_asset
                    } => {
                                                
                        Ok(Response::new()
                            .add_attribute("method", "liquidate")
                            .add_attribute("leftover_repayment", "0"))
                    }
                    SP_MockExecuteMsg::Distribute { 
                        distribution_assets,
                        distribution_asset_ratios, 
                        credit_asset, 
                        distribute_for } => {
                        
                       
                        Ok(Response::new()
                            .add_attribute("method", "distribute")
                            .add_attribute("credit_asset", "cdl"))
                    },
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { 
                        asset,
                    } => Ok(
                        to_binary(
                            &SP_LiquidatibleResponse {
                                leftover: Decimal::zero(),
                            })?),
                    SP_MockQueryMsg::AssetPool { asset_info 
                    } => Ok(
                        to_binary(&PoolResponse {
                            credit_asset: Asset {
                                info: AssetInfo::NativeToken { denom: "cdl".to_string() },
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
            basket_id: String,
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
        SpotPrice {
            asset: String,
        },
        PoolState {
            id: u64,
        },
        GetDenom {
            creator_address: String,
            subdenom: String,
        },
        ArithmeticTwapToNow {
            id: u64,
            quote_asset_denom: String,
            base_asset_denom: String,
            start_time: i64,
        },
    }

    pub fn osmosis_proxy_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens { 
                            denom, 
                            amount, 
                            mint_to_address
                     } => {
                        if amount == Uint128::new(1428u128){
                            assert_eq!( String::from("credit_fulldenom 1428 fee_collector"), format!("{} {} {}", denom, amount.to_string(), mint_to_address) );
                        }

                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::BurnTokens {
                        denom,
                        amount,
                        burn_from_address,
                    } => {
                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::CreateDenom { 
                        subdenom,
                        basket_id,
                        max_supply,
                        liquidity_multiplier,
                    } => {

                        Ok(Response::new().add_attributes(vec![
                            attr("basket_id", "1"),
                            attr("subdenom", "credit_fulldenom"),
                            attr("max_supply", max_supply.unwrap_or_else(|| Uint128::zero()).to_string()),
                            attr("liquidity_multiplier", liquidity_multiplier.unwrap_or_else(|| Decimal::zero()).to_string()),
                            ]
                        ))
                    }
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::SpotPrice { 
                        asset,
                    } => 
                        Ok(
                            to_binary(&SpotPriceResponse {
                                price: Decimal::one(),
                            })?
                        ),
                    Osmo_MockQueryMsg::PoolState { id } => 
                    if id == 99u64 {
                        Ok(
                            to_binary(&PoolStateResponse {
                                assets: vec![ coin( 100_000_000 , "base" ), coin( 100_000_000 , "quote" ) ],
                                shares: coin( 100_000_000, "lp_denom" ),
                            }

                            )?
                        )
                    } else {
                        Ok(
                            to_binary(&PoolStateResponse {
                                assets: vec![ coin( 49_999 , "credit_fulldenom" ) ],
                                shares: coin( 0, "shares" ),
                            }

                            )?
                        )
                    },
                    Osmo_MockQueryMsg::GetDenom { 
                        creator_address, 
                        subdenom 
                    } => {
                        Ok(
                            to_binary(&GetDenomResponse {
                                denom: String::from( "credit_fulldenom" ),
                            })?
                        )
                    },
                    Osmo_MockQueryMsg::ArithmeticTwapToNow { 
                        id, 
                        quote_asset_denom, 
                        base_asset_denom, 
                        start_time 
                    } => {
                        if base_asset_denom == String::from("base") {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(100),
                                })?
                            )

                        } else {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(100),
                                })?
                            )

                        }
                    }
                }},
        );
        Box::new(contract)
    }

    pub fn osmosis_proxy_contract_bignums()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens { 
                            denom, 
                            amount, 
                            mint_to_address
                     } => {
                        println!( "{}", format!("{} {} {}", denom, amount.to_string(), mint_to_address) );
                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::BurnTokens {
                        denom,
                        amount,
                        burn_from_address,
                    } => {
                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::CreateDenom { 
                        subdenom,
                        basket_id,
                        max_supply,
                        liquidity_multiplier,
                    } => {

                        Ok(Response::new().add_attributes(vec![
                            attr("basket_id", "1"),
                            attr("subdenom", "credit_fulldenom"),
                            attr("max_supply", max_supply.unwrap_or_else(|| Uint128::zero()).to_string()),
                            attr("liquidity_multiplier", liquidity_multiplier.unwrap_or_else(|| Decimal::zero()).to_string()),
                            ]
                        ))
                    }
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::SpotPrice { 
                        asset,
                    } => 
                        Ok(
                            to_binary(&SpotPriceResponse {
                                price: Decimal::one(),
                            })?
                        ),
                    Osmo_MockQueryMsg::PoolState { id } => {
                        if id == 99u64 {
                            Ok(
                                to_binary(&PoolStateResponse {
                                    assets: vec![ coin( 100_000_000 , "base" ), coin( 100_000_000 , "quote" ) ],
                                    shares: coin( 100_000_000, "lp_denom" ),
                                }
    
                                )?
                            )
                        } else {
                            Ok(
                                to_binary(&PoolStateResponse {
                                    assets: vec![ coin( 5_000_000_000_000 , "credit_fulldenom" ) ],
                                    shares: coin( 0, "shares" ),
                                }
    
                                )?
                            )
                        }
                    },
                    Osmo_MockQueryMsg::GetDenom { 
                        creator_address, 
                        subdenom 
                    } => {
                        Ok(
                            to_binary(&GetDenomResponse {
                                denom: String::from( "credit_fulldenom" ),
                            })?
                        )
                    },
                    Osmo_MockQueryMsg::ArithmeticTwapToNow { 
                        id, 
                        quote_asset_denom, 
                        base_asset_denom, 
                        start_time 
                    } => {
                        if base_asset_denom == String::from("base") {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(100),
                                })?
                            )

                        } else {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(100),
                                })?
                            )

                        }
                    }
                }},
        );
        Box::new(contract)
    }

    //Mock Router Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Router_MockExecuteMsg {
        SwapFromNative {
            to: AssetInfo,
            max_spread: Option<Decimal>,
            recipient: Option<String>,
            hook_msg: Option<Binary>,
            split: Option<bool>,
        },
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

    pub fn router_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Router_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Router_MockExecuteMsg::SwapFromNative { 
                        to, 
                        max_spread, 
                        recipient, 
                        hook_msg, 
                        split } => {
                        
                        Ok(Response::default())
                    },
                }
            },
            |_, _, _, _: Router_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Router_MockQueryMsg| -> StdResult<Binary> { Ok( to_binary(&MockResponse {})? ) },
        );
        Box::new(contract)
    }

    //Mock Auction Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Auction_MockExecuteMsg {
        StartAuction {
            basket_id: Uint128,
            position_id: Uint128,
            position_owner: String,
            debt_amount: Uint128,
        }
    }
    
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Auction_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Auction_MockQueryMsg {}

    pub fn auction_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Auction_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Auction_MockExecuteMsg::StartAuction { 
                        basket_id, 
                        position_id, 
                        position_owner, 
                        debt_amount }  => {
                        
                        Ok(Response::default())
                    },
                }
            },
            |_, _, _, _: Auction_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Auction_MockQueryMsg| -> StdResult<Binary> { Ok( to_binary(&MockResponse {})? ) },
        );
        Box::new(contract)
    }

    //Mock Staking Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockExecuteMsg {
        DepositFee {}
    }
    
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Staking_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockQueryMsg {}

    pub fn staking_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Staking_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Staking_MockExecuteMsg::DepositFee {}  => {                        
                        Ok(Response::default())
                    },
                }
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Staking_MockQueryMsg| -> StdResult<Binary> { Ok( to_binary(&MockResponse {})? ) },
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
        }
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
        Asset {
            asset_info: AssetInfo,
        },
    }

    pub fn oracle_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset { 
                        asset_info, 
                        oracle_info, 
                    }  => {
                        
                        Ok(Response::default())
                    },
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Price { 
                        asset_info,
                        twap_timeframe,
                        basket_id 
                    } => {

                        if basket_id.is_some(){
                            if basket_id.unwrap() == Uint128::new(2u128){              
                                Ok( to_binary(&PriceResponse { 
                                    prices: vec![],
                                    avg_price: Decimal::percent(500),
                                })? )
                            } else {
                                Ok( to_binary(&PriceResponse { 
                                    prices: vec![],
                                    avg_price: Decimal::one(),
                                })? )
                            }
                        } else if asset_info.to_string() == String::from("credit_fulldenom"){
                                
                            Ok( to_binary(&PriceResponse { 
                                prices: vec![],
                                avg_price: Decimal::percent(98),
                            })? )
                        } else {
                            Ok( to_binary(&PriceResponse { 
                                prices: vec![],
                                avg_price: Decimal::one(),
                            })? )
                        }
                        
                        
                    },
                    Oracle_MockQueryMsg::Asset { asset_info } => {
                        Ok( to_binary(&AssetResponse { 
                            asset_info: AssetInfo::NativeToken { denom: String::from("denom") },
                            oracle_info: AssetOracleInfo {
                                osmosis_pool_for_twap: TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                },
                            },
                        })? )
                    },
                }  },
        );
        Box::new(contract)
    }

    //Mock Cw20 Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Cw20_MockExecuteMsg {
        Transfer {
            recipient: String,
            amount: Uint128,
        }
    }
    
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Cw20_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Cw20_MockQueryMsg {
        Balance{
            address: String,
        }
    }

    pub fn cw20_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Cw20_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Cw20_MockExecuteMsg::Transfer { 
                        recipient, 
                        amount }  => {
                        
                        Ok(Response::default())
                    },
                }
            },
            |_, _, _, _: Cw20_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Cw20_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Cw20_MockQueryMsg::Balance { address } => {
                        Ok( to_binary(&BalanceResponse { balance: Uint128::zero()})? )
                    }
                }  },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
            AppBuilder::new().build(|router, _, storage| {
                                    
                let bank = BankKeeper::new();

                bank.init_balance(storage, &Addr::unchecked(USER), vec![coin(100_000, "debit"), coin(100_000, "2nddebit")])
                .unwrap();
                bank.init_balance(storage, &Addr::unchecked("contract1"), vec![coin(2225, "credit_fulldenom")])
                .unwrap(); //contract1 = Stability Pool contract
                bank.init_balance(storage, &Addr::unchecked("test"), vec![coin(50_000, "credit_fulldenom"), coin(100_000, "debit")])
                .unwrap(); 
                bank.init_balance(storage, &Addr::unchecked("sender"), vec![coin(50_001, "credit_fulldenom")])
                .unwrap(); 
                bank.init_balance(storage, &Addr::unchecked("big_bank"),  vec![coin(10_000_000, "debit"), coin(10_000_000, "double_debit")])
                .unwrap();
                bank.init_balance(storage, &Addr::unchecked("bigger_bank"),  vec![coin(100_000_000_000_000, "debit"), coin(100_000_000_000_000, "double_debit"), coin(100_000_000_000_000, "lp_denom"), coin(100_000_000_000_000, "credit_fulldenom")])
                .unwrap(); 
                bank.init_balance(storage, &Addr::unchecked("little_bank"),  vec![coin(1_000, "debit")])
                .unwrap(); 
                bank.init_balance(storage, &Addr::unchecked("coin_God"), vec![coin(2_250_000_000_000, "credit_fulldenom")])
                .unwrap();
                bank.init_balance(storage, &Addr::unchecked("lp_tester"), vec![coin(100_000_000, "lp_denom")])
                .unwrap();
                bank.init_balance(storage, &Addr::unchecked("faker"), vec![coin(666, "fake_debit")])
                .unwrap();

                router
                    .bank = bank;
                    
            })
        }

    fn proper_instantiate( sp_error: bool, lq_error: bool, liq_minimum: bool, bignums: bool ) -> (App, CDPContract, LQContract, Addr) {
        let mut app = mock_app();

        //Instantiate Cw20
        let cw20_id = app.store_code(cw20_contract());
        let cw20_contract_addr = app
            .instantiate_contract(
                cw20_id, 
                Addr::unchecked(ADMIN), 
                &Cw20_MockInstantiateMsg {},
                &[], 
                "test",
                None).unwrap();
        
        //Instanitate SP
        let mut sp_id: u64;
        if sp_error{
            sp_id = app.store_code(stability_pool_contract_errors());
        }else if liq_minimum{
            sp_id = app.store_code(stability_pool_contract_minimumliq());
        }else if bignums{
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
        let mut lq_id: u64;
        if lq_error{
            lq_id = app.store_code(liq_queue_contract_errors());
        }else if liq_minimum{
            lq_id = app.store_code(liq_queue_contract_minimumliq());
        }else if bignums{
            lq_id = app.store_code(liq_queue_contract_bignums());
        }else{
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
        let mut proxy_id: u64;
        if bignums{
            proxy_id = app.store_code(osmosis_proxy_contract_bignums());
        }else{
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

        //Instantiate CDP contract
        let cdp_id = app.store_code(cdp_contract());

        let msg = 
            InstantiateMsg {
                owner: Some(ADMIN.to_string()),
                liq_fee: Decimal::percent(1),
                stability_pool: Some( sp_contract_addr.to_string() ),
                dex_router: Some( router_contract_addr.to_string() ),
                staking_contract: Some( staking_contract_addr.to_string() ),
                oracle_contract: Some( oracle_contract_addr.to_string() ),
                interest_revenue_collector: Some( "fee_collector".to_string()),
                osmosis_proxy: Some( osmosis_proxy_contract_addr.to_string() ),   
                debt_auction: Some( auction_contract_addr.to_string() ),
                oracle_time_limit: 60u64,
                debt_minimum: Uint128::new(2000u128),
                twap_timeframe: 90u64,
        };

        

        let cdp_contract_addr = app
            .instantiate_contract(
                cdp_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "test",
                None,
            )
            .unwrap();

        let cdp_contract = CDPContract(cdp_contract_addr);

        let msg = ExecuteMsg::CreateBasket {
            owner: Some("owner".to_string()),
            collateral_types: vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                       } 
            ],
            credit_asset: Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(0u128),
            },
            credit_price: Decimal::percent(100),
            base_interest_rate: None,
            desired_debt_cap_util: None,
            credit_pool_ids: vec![],
            liquidity_multiplier_for_debt_caps: None,
            liq_queue: None,
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        let msg = ExecuteMsg::EditBasket {
            basket_id: Uint128::from(1u128), 
            added_cAsset: None, 
            owner: None,  
            liq_queue: None,  
            pool_ids: None,  
            liquidity_multiplier: None,  
            collateral_supply_caps: None,  
            base_interest_rate: None,  
            desired_debt_cap_util: None,  
            credit_asset_twap_price_source: Some( 
                TWAPPoolInfo {
                    pool_id: 0u64,
                    base_asset_denom: String::from("base"),
                    quote_asset_denom: String::from("quote"),
            } ) 
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        (app, cdp_contract, lq_contract, cw20_contract_addr)
    }

    


    mod cdp {
        
        use super::*;
        use cosmwasm_std::{BlockInfo, coins};
        use cw20::Cw20ReceiveMsg;
        use membrane::{positions::{ExecuteMsg, ConfigResponse, PropResponse, PositionResponse, BasketResponse, DebtCapResponse, BadDebtResponse, InsolvencyResponse, PositionsResponse, Cw20HookMsg}, types::{UserInfo, InsolventPosition, PositionUserInfo, TWAPPoolInfo, PoolInfo, SupplyCap, LPAssetInfo}};

        #[test]
        fn withdrawal() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
             
            //Edit Basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "2nddebit".to_string() },
                            amount: Uint128::from(0u128),
                        }, 
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                } ), 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },  
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: Some( Decimal::percent(10) ),
                desired_debt_cap_util: None,
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
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
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert asset tally is working
            let query_msg = QueryMsg::GetBasket { 
                basket_id: Uint128::new(1u128), 
            };
            let res: BasketResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_supply_caps[0].current_supply, Uint128::new(100000));
            assert_eq!(res.collateral_supply_caps[1].current_supply, Uint128::new(100000));

            //Query BasketPositions
            let msg = QueryMsg::GetBasketPositions { 
                basket_id: Uint128::from(1u128), 
                start_after: None,
                limit: None,
            };

            let resp: Vec<PositionsResponse> = app.wrap().query_wasm_smart(cdp_contract.addr(),&msg.clone() ).unwrap();
            assert_eq!(resp[0].positions[0].collateral_assets[0].asset.amount.to_string(), String::from("100000"));
            assert_eq!(resp[0].positions[0].collateral_assets[1].asset.amount.to_string(), String::from("100000"));
            assert_eq!(resp.len().to_string(), String::from("1"));

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            
            //Insolvent withdrawal error
            let msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets: vec![Asset { 
                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                    amount: Uint128::from(100_000u128)
                }, Asset { 
                    info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, 
                    amount: Uint128::from(100_000u128)
                }],
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
                        
            //Successful attempt
            let withdrawal_msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets: vec![Asset { 
                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                    amount: Uint128::from(90_000u128)
                }, Asset { 
                    info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, 
                    amount: Uint128::from(90_000u128)
                }], 
            };

            let cosmos_msg = cdp_contract.call( withdrawal_msg, vec![] ).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query Position assets to assert withdrawal
            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  USER.to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(10000));
            assert_eq!(res.collateral_assets[1].asset.amount, Uint128::new(10000));

            //Assert withdrawal was sent.
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 90000, "2nddebit"), coin( 90000, "debit")]);

            //Assert asset tally and CreateDenom is working
            let query_msg = QueryMsg::GetBasket { 
                basket_id: Uint128::new(1u128), 
            };
            let res: BasketResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_supply_caps[0].current_supply, Uint128::new(10000));
            assert_eq!(res.collateral_supply_caps[1].current_supply, Uint128::new(10000));
            //Assert Denom change
            assert_eq!( res.credit_asset.info.to_string(), "credit_fulldenom".to_string() );          

        }

        #[test]
        fn cw20_withdrawal() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
             
            //Edit Basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::Token { address: cw20_addr.clone() },
                            amount: Uint128::from(0u128),
                        }, 
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                } ), 
                owner: None,  
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },  
                    SupplyCap { 
                        asset_info: AssetInfo::Token { address: cw20_addr.clone() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: Some( Decimal::percent(10) ),
                desired_debt_cap_util: None,
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Initial Deposit
            let exec_msg = ExecuteMsg::Receive( Cw20ReceiveMsg {
                sender: String::from("sender88"),
                amount: Uint128::new(1),
                msg: to_binary(&Cw20HookMsg::Deposit{
                        position_owner: None,
                        basket_id: Uint128::from(1u128),
                        position_id: None,       
                }).unwrap(),
            });
            let cosmos_msg = cdp_contract
                .call(exec_msg,vec![])
                    .unwrap();
            app.execute(cw20_addr.clone(), cosmos_msg).unwrap();

            //Successful attempt
            let withdrawal_msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets: vec![Asset { 
                    info: AssetInfo::Token { address: cw20_addr.clone() }, 
                    amount: Uint128::from(1u128)
                }], 
            };

            let cosmos_msg = cdp_contract.call( withdrawal_msg, vec![] ).unwrap();
            app.execute(Addr::unchecked("sender88"), cosmos_msg).unwrap();

        }

        #[test]
        fn increase_debt__repay() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
              
            //Edit Basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "test".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();
            
            //Insolvent position error
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(50_001u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap_err();

            //Minimum Debt Error
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(1u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap_err();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(50_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Send credit
            app.send_tokens(Addr::unchecked("sender"), Addr::unchecked("test"), &[ coin(50_001, "credit_fulldenom") ]).unwrap();
        
            //Error on Partial Repayment under config.debt_minimum 
            let msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                position_owner: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![coin(49_901, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap_err();


            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  "test".to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.credit_amount, String::from("50000"));

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps {
                basket_id: Uint128::new(1u128), 
            };
            let res: DebtCapResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.caps, String::from("debit: 50000/249995, ") );

            //Excess Repayment
            let msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                position_owner: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![coin(50_001, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap_err();

            //Successful repayment
            let repay_msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128), 
                position_id: Uint128::from(1u128), 
                position_owner:  Some("test".to_string()), 
            };
            let cosmos_msg = cdp_contract.call(repay_msg, vec![ coin(50_000, "credit_fulldenom") ]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  "test".to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.credit_amount, String::from("0"));
           
        }

        #[test]
        fn accrue_debt() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, true, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            

             //Add LP pool assets first
             let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "base".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: None,
                        
                    }
                ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: None,  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "quote".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(60),
                        max_LTV: Decimal::percent(80),
                        pool_info: None,
                        
                    }
                ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: None,  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
              
            //Edit Basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "lp_denom".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: Some( PoolInfo {
                            pool_id: 99u64,
                            asset_infos: vec![
                                LPAssetInfo { 
                                    info:AssetInfo::NativeToken { denom: String::from("base") }, 
                                    decimals: 6u64, 
                                    ratio: Decimal::percent(50), 
                                },
                                LPAssetInfo { 
                                    info:AssetInfo::NativeToken { denom: String::from("quote") }, 
                                    decimals: 6u64, 
                                    ratio: Decimal::percent(50), 
                                },
                                ],
                        } ),
                        
                    }
                ), 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },  
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "base".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "quote".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "lp_denom".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    } ]),
                base_interest_rate: Some( Decimal::percent(10) ),
                desired_debt_cap_util: None,
                credit_asset_twap_price_source: None, 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "bigger_bank".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "lp_denom".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(99_999u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();
            
            //Insolvent position error
            ///Expected to Error due to accrued interest
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(1u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap_err();

            
            //Successful repayment that will leave the accrued interest left 
            let msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                position_owner: Some( String::from("bigger_bank") ),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![coin(99_000, "credit_fulldenom")]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  "bigger_bank".to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            ///999 leftover + 5833 debt 
            assert_eq!(res.credit_amount, String::from("6832"));

             //Insolvent withdrawal error
             ////This should be solvent if there wasn't accrued interest
             let msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets: vec![Asset { 
                    info: AssetInfo::NativeToken { denom: "lp_denom".to_string() }, 
                    amount: Uint128::from(93_168u128)
                }],
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap_err();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                basket_id: Uint128::new(1u128), 
                position_id: Uint128::new(1u128), 
                position_owner: "bigger_bank".to_string(), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Successful LiqRepay
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset {
                    info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                    amount: Uint128::new(499),
                }
            };
            let cosmos_msg = cdp_contract.call(msg, vec![ coin(499, "credit_fulldenom") ]).unwrap();
            app.execute(Addr::unchecked(sp_addr.clone()), cosmos_msg).unwrap();  

            //Would normally liquidate and leave 96143 "lp_denom"
            // but w/ accrued interest its leaving 96050
            let query_msg = QueryMsg::GetUserPositions { 
                basket_id: None, 
                user: String::from("bigger_bank"), 
                limit: None,
            };
            
            let res: Vec<PositionResponse> = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res[0].collateral_assets[0].asset.amount, Uint128::new(96050));

             //Assert sell wall wasn't sent Assets
             assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![]);

             //Assert fees were sent.
             assert_eq!(app.wrap().query_all_balances(staking_contract.clone()).unwrap(), vec![coin( 11, "debit")]);
             //The fee is 227
             assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 100000, "2nddebit"), coin( 100_227, "debit")]);
 
             
             assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin( 1726 , "credit_fulldenom"), coin( 577, "debit")]);
             assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![coin( 637, "debit")]);
 
           
        }

        #[test]
        fn accrue_credit_repayment_price() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, true, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
              
            //Edit Basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None,
                credit_asset_twap_price_source: None, 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "test".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(49_999u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();
            //Send credit
            app.send_tokens(Addr::unchecked("sender"), Addr::unchecked("test"), &[ coin(49_999, "credit_fulldenom") ]).unwrap();
            
            //Insolvent position error
            ///Expected to Error due to a greater repayment price
            /// //otherwise this would be solvent and a valid increase
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(1u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap_err();

                            
            //Successful repayment of the full position
            //With only repayment price increases, the amount being repaid doesn't change..
            //..but the amount that results in minimum debt errors decreases
            let msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                position_owner: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![coin(49_501, "credit_fulldenom")]).unwrap();
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Assert Increased credit price is saved correctly
            let query_msg = QueryMsg::GetBasket { 
                basket_id: Uint128::new(1u128), 
            };
            let res: BasketResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.credit_price, String::from("1.020816326"));

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  "test".to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.credit_amount, String::from("498"));


             //Insolvent withdrawal at that brings position to previous debt minimum
             ////This wouldn't insolvent if there wasn't an increased repayment price
             /// 498 backed by 996: 50% borrow LTV so would've been solvent at $1 credit
             let msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets: vec![Asset { 
                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                    amount: Uint128::from(99_004u128)
                }],
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap_err();


            //Successful Increase just so the liquidation works
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(2u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                basket_id: Uint128::new(1u128), 
                position_id: Uint128::new(1u128), 
                position_owner: "test".to_string(), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Would normally liquidate and leave 99863 "debit"
            // but w/ accrued interest its leaving 99782
            let query_msg = QueryMsg::GetUserPositions { 
                basket_id: None, 
                user: String::from("test"), 
                limit: None,
            };
            
            let res: Vec<PositionResponse> = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res[0].collateral_assets[0].asset.amount, Uint128::new(99782));
           
        }

        #[test]
        fn revenue() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, true, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
              
            //Edit Basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: Some( Decimal::percent(10) ),
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "test".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(49_999u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();
            //Send credit
            app.send_tokens(Addr::unchecked("sender"), Addr::unchecked("test"), &[ coin(49_999, "credit_fulldenom") ]).unwrap();
                       
            //Successful repayment that will leave the accrued interest left 
            let msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                position_owner: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![coin(49_000, "credit_fulldenom")]).unwrap();
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasket { basket_id: Uint128::new(1u128) };   
            let res: BasketResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            ///1428 revenue 
            assert_eq!(res.pending_revenue.to_string(), String::from("1428"));

            //Successful Mint
            let msg = ExecuteMsg::MintRevenue { 
                basket_id: Uint128::from(1u128), 
                send_to: None, 
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

            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, true, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None,
                credit_asset_twap_price_source: None, 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "test".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

             /////Liq Repay///
            /// 
            /// //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(50_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Successful liquidation
            let msg = ExecuteMsg::Liquidate { 
                basket_id: Uint128::new(1u128), 
                position_id: Uint128::new(1u128), 
                position_owner: "test".to_string(), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Unauthorized
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset {
                    info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                    amount: Uint128::new(50_000),
                }
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap_err();            


            //Send SP funds to liquidate
            app.send_tokens(Addr::unchecked("sender"), Addr::unchecked(sp_addr.clone()), &[ coin(50_000, "credit_fulldenom") ]).unwrap();

            //Successful LiqRepay
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset {
                    info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                    amount: Uint128::new(20222),
                }
            };
            let cosmos_msg = cdp_contract.call(msg, vec![ coin(50_000, "credit_fulldenom") ]).unwrap();
            app.execute(Addr::unchecked(sp_addr), cosmos_msg).unwrap();  
            
            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  "test".to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.credit_amount, String::from("0"));           
            
        }

        
        #[test]
        fn liquidate() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: USER.to_string(), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Call LiqRepay to mimic a successfull SP Liquidate call
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                    amount: Uint128::new(222),
                    } };

            let cosmos_msg = cdp_contract.call(msg, vec![coin( 222, "credit_fulldenom")]).unwrap();
            app.execute( Addr::unchecked(sp_addr.clone()), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  USER.to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97290));

            //Assert sell wall wasn't sent assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![]);

            //Assert fees were sent.
            assert_eq!(app.wrap().query_all_balances(staking_contract.clone()).unwrap(), vec![coin( 22, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 100000, "2nddebit"), coin( 444, "debit")]);

            //Assert collateral to be liquidated was sent 
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin(2003, "credit_fulldenom"), coin( 244, "debit")]);
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![coin( 2000, "debit")]);


            /////////SP Errors////
            /// 
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( true, false, false, false);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: USER.to_string(), 
                };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            
            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  USER.to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97312));

            //Assert sell wall was sent assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![coin( 222, "debit")]);

            //Assert fees were sent.
            assert_eq!(app.wrap().query_all_balances(staking_contract.clone()).unwrap(), vec![coin( 22, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 100000, "2nddebit"), coin( 444, "debit")]);

            //Assert collateral to be liquidated was sent 
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![coin( 2000, "debit")]);
            //Assert SP wasn't sent any due to the Error
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin( 2225 , "credit_fulldenom")]);
            
            //////LQ Errors///
            /// 
            
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, true, false, false);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //app.wrap().query_wasm_smart(cdp_contract.addr(),QueryMsg:: )

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: USER.to_string(), 
                };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call LiqRepay to mimic a successfull SP Liquidate call by both the initial SP and then LQ reply
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                    amount: Uint128::new(2225),
                    } };

            let cosmos_msg = cdp_contract.call(msg, vec![coin( 2225, "credit_fulldenom")]).unwrap();
            app.execute( Addr::unchecked(sp_addr.clone()), cosmos_msg).unwrap();

            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97087));

            //Assert sell wall wasn't sent assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![]);

            //Assert fees were sent. 
            assert_eq!(app.wrap().query_all_balances(staking_contract.clone()).unwrap(), vec![coin( 22, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 100000, "2nddebit"),coin( 444, "debit")]);

            //Assert collateral to be liquidated was sent 
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin( 2447 , "debit")]);
            //Assert LQ wasn't sent any due to the Error
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![]);
            

            //////All Errors/////
            /// 
                
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( true, true, false, false);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //app.wrap().query_wasm_smart(cdp_contract.addr(),QueryMsg:: )

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: USER.to_string(), 
                };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  USER.to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97312));

            //Assert sell wall was sent assets all Assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![coin( 2222, "debit")]);

            //Assert fees were sent.
            assert_eq!(app.wrap().query_all_balances(staking_contract.clone()).unwrap(), vec![coin( 22, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 100000, "2nddebit"),coin( 444, "debit")]);

            //Assert neither module was sent any due to the Error
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin( 2225 , "credit_fulldenom")]);
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![]);

            //Assert asset tally is working
            let query_msg = QueryMsg::GetBasket { 
                basket_id: Uint128::new(1u128), 
            };
            let res: BasketResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_supply_caps[0].current_supply, Uint128::new(97312));
        }

        #[test]
        fn liquidate_bignums() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, true);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "bigger_bank".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000_000_000_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000_000_000_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: "bigger_bank".to_string(), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Call LiqRepay to mimic a successfull SP Liquidate call
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                    amount: Uint128::new(222_222_222_222),
                    } };
            
            app.send_tokens(Addr::unchecked("coin_God"), Addr::unchecked(sp_addr.clone()), &vec![coin( 222_222_222_222, "credit_fulldenom")] ).unwrap();
            let cosmos_msg = cdp_contract.call(msg, vec![coin( 222_222_222_222, "credit_fulldenom")]).unwrap();
            app.execute( Addr::unchecked(sp_addr.clone()), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  "bigger_bank".to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97_288_888_885_531));

            //Assert sell wall wasn't sent assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![]);

            //Assert fees were sent.
            assert_eq!(app.wrap().query_all_balances(staking_contract.clone()).unwrap(), vec![coin( 22_222_222_250, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 100000, "2nddebit"), coin( 444_444_545_000, "debit")]);

            //Assert collateral to be liquidated was sent 
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin(2225, "credit_fulldenom"), coin( 244_444_444_444, "debit")]);
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![coin( 2_000_000_002_775, "debit")]);

        }

        #[test]
        fn liquidate_minimums() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, true, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(999u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: USER.to_string(), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Call LiqRepay to mimic a successfull SP Liquidate call
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                    amount: Uint128::new(499),
                    } };

            let cosmos_msg = cdp_contract.call(msg, vec![coin( 499, "credit_fulldenom")]).unwrap();
            app.execute( Addr::unchecked(sp_addr.clone()), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                position_owner:  USER.to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(98744));

            //Assert sell wall wasn't sent assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![]);

            //Assert fees were sent.
            assert_eq!(app.wrap().query_all_balances(staking_contract.clone()).unwrap(), vec![coin( 9, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 100000, "2nddebit"), coin( 199, "debit")]);

            //Assert collateral to be liquidated was sent 
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin(1726, "credit_fulldenom"), coin( 548, "debit")]);
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![coin( 500, "debit")]);
        }

        #[test]
        fn debt_caps() {

            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, true, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "base".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: None,
                        
                    }
                ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: None,  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "quote".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: None,
                        
                    }
                ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: None,  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            //Edit initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "lp_denom".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: Some( PoolInfo {
                            pool_id: 99u64,
                            asset_infos: vec![
                                LPAssetInfo { 
                                    info:AssetInfo::NativeToken { denom: String::from("base") }, 
                                    decimals: 6u64, 
                                    ratio: Decimal::percent(50), 
                                },
                                LPAssetInfo { 
                                    info:AssetInfo::NativeToken { denom: String::from("quote") }, 
                                    decimals: 6u64, 
                                    ratio: Decimal::percent(50), 
                                },
                                ],
                        } ),
                        
                    }
                ),
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },  
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "base".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "quote".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "lp_denom".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    } ]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "bigger_bank".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(10_000_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            ///Over Cap error
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(249_996u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap_err();

             ///Successful increase on the cap
             let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(249_995u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //2nd Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "bigger_bank".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: Some( Uint128::from(1u128) ),
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "lp_denom".to_string(),
                            amount: Uint128::from(10_000_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps {
                basket_id: Uint128::new(1u128), 
            };
            let res: DebtCapResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.caps, String::from("debit: 83332/83331, base: 83331/83331, quote: 83331/83331, ") );

            //Partially withdraw 1st Deposit
            let assets: Vec<Asset> = vec![
                Asset { info: AssetInfo::NativeToken { denom: "debit".to_string() },
                amount: Uint128::new(10_000_000),}
            ];

            let msg = ExecuteMsg::Withdraw { basket_id: Uint128::from(1u128), position_id: Uint128::from(1u128), assets };
            let cosmos_msg = cdp_contract.call( msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps {
                basket_id: Uint128::new(1u128), 
            };
            let res: DebtCapResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.caps, String::from("debit: 0/0, base: 124996/124997, quote: 124996/124997, ") );

        
        }
        
        #[test]
        fn bad_debt() {

            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, true, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
            
            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Deposit #1
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "big_bank".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(10_000_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg).unwrap();

            //Deposit #2
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "little_bank".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(1_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("little_bank"), cosmos_msg).unwrap();

            
            //Increase Debt for 1 position
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(50_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg).unwrap();

            //Query for BadDebt from 1 position w/o debt and 1 position with
            let query_msg = QueryMsg::GetBasketBadDebt { basket_id: Uint128::new(1) };
            let res: BadDebtResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            //Assert no bad debt
            assert_eq!( res.has_bad_debt, vec![] );

        }
        
        #[test]
        fn insolvency_checks() {

            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, true, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
            
            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Deposit #1
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "big_bank".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(10_000_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg).unwrap();

            //Deposit #2
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "little_bank".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(1_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("little_bank"), cosmos_msg).unwrap();

            
            //Increase Debt for 1 position
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(50_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg).unwrap();

            //Query for Insolvency from 1 position w/o debt and 1 position with
            let query_msg = QueryMsg::GetBasketInsolvency { basket_id: Uint128::new(1), start_after: None, limit: None };
            let res: InsolvencyResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            //Assert no insolvencies
            assert_eq!( res.insolvent_positions, vec![] );

            //Query the indebted position 
            let query_msg = QueryMsg::GetPositionInsolvency { basket_id: Uint128::new(1), position_id:  Uint128::new(1), position_owner: String::from("big_bank") };
            let res: InsolvencyResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            //Assert no insolvencies
            assert_eq!( res.insolvent_positions, vec![ 
                InsolventPosition { 
                    insolvent: false, 
                    position_info: UserInfo { 
                        basket_id: Uint128::new(1), 
                        position_id: Uint128::new(1), 
                        position_owner:  String::from("big_bank"), 
                    }, 
                    current_LTV: Decimal::percent(5) * Decimal::percent(10), 
                    available_fee: Uint128::zero(),
                 }] );
        }

        #[test]
        fn two_collateral_cdp_LTV_tests() {

            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, true, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
            
            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "double_debit".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: None,
                    }
                 ), 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps:  Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "double_debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "big_bank".to_string() ),
                basket_id: Uint128::from(1u128),
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
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg).unwrap();

           //Increase Debt
           let msg = ExecuteMsg::IncreaseDebt{
            basket_id: Uint128::from(1u128),
            position_id: Uint128::from(1u128),
            amount: Uint128::from(4999u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("big_bank"), cosmos_msg).unwrap();

            //Query for Insolvency 
            let query_msg = QueryMsg::GetPosition { 
                basket_id: Uint128::new(1),
                position_id: Uint128::new(1),
                position_owner: String::from("big_bank"),
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            //Assert LTVs
            assert_eq!( res.avg_borrow_LTV.to_string(), String::from("0.45") );
            assert_eq!( res.avg_max_LTV.to_string(), String::from("0.65") );
        }
        
        #[test]
        fn two_collateral_cdp_LTV_tests_bignums() {

            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
            
            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "double_debit".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: None,
                        
                    }
                 ), 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "double_debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "bigger_bank".to_string() ),
                basket_id: Uint128::from(1u128),
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
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

           //Increase Debt
           let msg = ExecuteMsg::IncreaseDebt{
            basket_id: Uint128::from(1u128),
            position_id: Uint128::from(1u128),
            amount: Uint128::from(249_995u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();

            //Query for Insolvency 
            let query_msg = QueryMsg::GetPosition { 
                basket_id: Uint128::new(1),
                position_id: Uint128::new(1),
                position_owner: String::from("bigger_bank"),
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            //Assert LTVs
            assert_eq!( res.avg_borrow_LTV.to_string(), String::from("0.45") );
            assert_eq!( res.avg_max_LTV.to_string(), String::from("0.65") );
        }
    

    #[test]
        fn collateral_supply_caps() {

            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "base".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: None,
                        
                    }
                ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: None,  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "quote".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: None,
                        
                    }
                ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: None,  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Edit initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "lp_denom".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: Some( PoolInfo {
                            pool_id: 99u64,
                            asset_infos: vec![
                                LPAssetInfo { 
                                    info:AssetInfo::NativeToken { denom: String::from("base") }, 
                                    decimals: 6u64, 
                                    ratio: Decimal::percent(50), 
                                },
                                LPAssetInfo { 
                                    info:AssetInfo::NativeToken { denom: String::from("quote") }, 
                                    decimals: 6u64, 
                                    ratio: Decimal::percent(50), 
                                },
                                ],
                        } ),
                        
                    }
                ),
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(99),
                        lp: false, 
                    },  
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "base".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(49),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "quote".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(49),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "lp_denom".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    } ]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Errored Deposit, over supply cap
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "bigger_bank".to_string() ),
                basket_id: Uint128::from(1u128),
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
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap_err();

            //Deposit would work if LP assets aren't counted correctly
            //Bc the individual assets are capped at 49, the LP can't be added alone.
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "bigger_bank".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "lp_denom".to_string(),
                            amount: Uint128::from(10_000_000_000_000u128),
                            },
                        ])
                    .unwrap();
            let err = app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Supply cap ratio for base is over the limit (0.5 > 0.49)\""));

            //Successful Deposit, user must deposit both to escape caps
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "bigger_bank".to_string() ),
                basket_id: Uint128::from(1u128),
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
                            denom: "lp_denom".to_string(),
                            amount: Uint128::from(10_000_000_000_000u128),
                            },
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("bigger_bank"), cosmos_msg).unwrap();


            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps {
                basket_id: Uint128::new(1u128), 
            };
            let res: DebtCapResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.caps, String::from("debit: 0/83331, base: 0/83331, quote: 0/83331, lp_denom: 0/0, ") );


           
        }

        #[test]
        fn LP_oracle() {

            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let staking_contract = res.staking_contract;
            
            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "base".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: None,
                        
                    }
                ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: None,  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "quote".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: None,
                        
                    }
                ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: None,  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            //Add LP to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( 
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "lp_denom".to_string() },
                                amount: Uint128::zero(),
                            }, 
                        max_borrow_LTV: Decimal::percent(40),
                        max_LTV: Decimal::percent(60),
                        pool_info: Some( PoolInfo {
                            pool_id: 99u64,
                            asset_infos: vec![
                                LPAssetInfo { 
                                    info:AssetInfo::NativeToken { denom: String::from("base") }, 
                                    decimals: 6u64, 
                                    ratio: Decimal::percent(50), 
                                },
                                LPAssetInfo { 
                                    info:AssetInfo::NativeToken { denom: String::from("quote") }, 
                                    decimals: 6u64, 
                                    ratio: Decimal::percent(50), 
                                },
                                ],
                        } ),
                        
                    }
                ), 
                owner: None, 
                liq_queue: Some( lq_contract.addr().to_string() ),
                liquidity_multiplier: Some( Decimal::percent( 500 ) ),
                pool_ids: Some( vec![ 1u64 ] ),
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },  
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "base".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "quote".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "lp_denom".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    } ]),
                base_interest_rate: None,
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit { 
                position_owner: Some( "lp_tester".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "lp_denom".to_string(),
                            amount: Uint128::from(100_000u128),
                            },
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("lp_tester"), cosmos_msg).unwrap();

           //The value of the position should be 200_000
           //So at 40% borrow LTV I should be able to borrow 80_000
           //We'll error at the edge first to confirm
           let msg = ExecuteMsg::IncreaseDebt { 
                basket_id: Uint128::from(1u128), 
                position_id: Uint128::from(1u128), 
                amount: Uint128::from(80_001u128), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("lp_tester"), cosmos_msg).unwrap_err();
            
            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt { 
                basket_id: Uint128::from(1u128), 
                position_id: Uint128::from(1u128), 
                amount: Uint128::from(80_000u128), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("lp_tester"), cosmos_msg).unwrap();
           
        }

        ///contract_test Migration
         #[test]
        fn cw20_deposit(){
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

           
            //Add Basket
            let create_basket_msg = ExecuteMsg::CreateBasket {
                owner: Some("owner".to_string()),
                collateral_types: vec![
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::Token { address: cw20_addr.clone() },
                                amount: Uint128::from(0u128),
                            },
                        max_borrow_LTV: Decimal::percent(50),
                        max_LTV: Decimal::percent(90),
                        pool_info: None,
                        } 
                ],
                credit_asset: Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::from(0u128),
                },
                credit_price: Decimal::percent(100),
                base_interest_rate: None,
                desired_debt_cap_util: None,
                credit_pool_ids: vec![],
                liquidity_multiplier_for_debt_caps: None,
                liq_queue: None,
            };
            let cosmos_msg = cdp_contract.call(create_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Edit Basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(2u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: Some( vec![
                    SupplyCap { 
                        asset_info: AssetInfo::Token { address: cw20_addr.clone() },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                }] ),  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Deposit
            let exec_msg = ExecuteMsg::Receive( Cw20ReceiveMsg {
                sender: String::from("sender88"),
                amount: Uint128::new(1),
                msg: to_binary(&Cw20HookMsg::Deposit{
                        position_owner: Some( "owner".to_string() ),
                        basket_id: Uint128::from(2u128),
                        position_id: None,       
                }).unwrap(),
            });
            let cosmos_msg = cdp_contract.call(exec_msg, vec![]).unwrap();
            let res = app.execute(cw20_addr, cosmos_msg).unwrap();

            let response = res.events
                .into_iter()
                .find(|e| e.attributes
                    .iter()
                    .any(|attr| attr.key == "basket_id")
                )
                .ok_or_else(|| panic!("unable to find cw20_deposit event"))
                .unwrap();
            
            assert_eq!(
                response.attributes[1..],
                vec![
                attr("method", "deposit"),
                attr("basket_id", "2"),
                attr("position_owner","owner"),
                attr("position_id", "1"),
                attr("assets", "1 contract0"),
                ]
            );
            
        }

            
        #[test]
        fn misc_query() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);
            
            
            //Edit Admin
            let msg = ExecuteMsg::EditAdmin { owner: String::from("owner") };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Add 2ndary basket
            let create_basket_msg = ExecuteMsg::CreateBasket {
                owner: Some("owner".to_string()),
                collateral_types: vec![
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "debit".to_string() },
                                amount: Uint128::from(0u128),
                            },
                        max_borrow_LTV: Decimal::percent(50),
                        max_LTV: Decimal::percent(90),
                        pool_info: None,
                        } 
                ],
                credit_asset: Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::from(0u128),
                },
                credit_price: Decimal::percent(100),
                base_interest_rate: None,
                desired_debt_cap_util: None,
                credit_pool_ids: vec![],
                liquidity_multiplier_for_debt_caps: None,
                liq_queue: None,
            };
            let cosmos_msg = cdp_contract.call(create_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked("owner"), cosmos_msg).unwrap();

            //Edit Basket 2
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(2u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: Some( vec![
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                }] ),  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("owner"), cosmos_msg).unwrap();

            //Edit Basket 1
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: None, 
                pool_ids: None,  
                liquidity_multiplier: None,  
                collateral_supply_caps: Some( vec![
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                }] ),  
                base_interest_rate: None,  
                desired_debt_cap_util: None,  
                credit_asset_twap_price_source: None,  
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("owner"), cosmos_msg).unwrap();

            //Initial deposit to Basket 1
            let exec_msg = ExecuteMsg::Deposit { 
                position_owner: Some( String::from("sender88") ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            app.send_tokens(Addr::unchecked("little_bank"), Addr::unchecked( "sender88" ), &vec![coin( 22, "debit")] ).unwrap();
            let cosmos_msg = cdp_contract.call(exec_msg, vec![ coin(11, "debit") ]).unwrap();
            let res = app.execute(Addr::unchecked("sender88"), cosmos_msg).unwrap();

            //Initial deposit to Basket 2        
            let exec_msg = ExecuteMsg::Deposit { 
                position_owner: Some( String::from("sender88") ),
                basket_id: Uint128::from(2u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, vec![ coin(11, "debit") ]).unwrap();
            let res = app.execute(Addr::unchecked("sender88"), cosmos_msg).unwrap();

                    
            //Query AllBaskets
            let msg = QueryMsg::GetAllBaskets { 
                start_after: None,
                limit: None,
            };
            let resp: Vec<BasketResponse> = app.wrap().query_wasm_smart(cdp_contract.addr(),&msg.clone() ).unwrap();

            assert_eq!( resp[0].basket_id, String::from(Uint128::from(1u128)) );
            assert_eq!( resp[1].basket_id, String::from(Uint128::from(2u128)) );
            assert_eq!(resp.len().to_string(), String::from("2"));   

        }

        #[test]
        fn edit_cAsset() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);
            
            //Add Basket
            let create_basket_msg = ExecuteMsg::CreateBasket {
                owner: Some("owner".to_string()),
                collateral_types: vec![
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "debit".to_string() },
                                amount: Uint128::from(0u128),
                            },
                        max_borrow_LTV: Decimal::percent(50),
                        max_LTV: Decimal::percent(90),
                        pool_info: None,
                        } 
                ],
                credit_asset: Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::from(0u128),
                },
                credit_price:  Decimal::percent(100),
                base_interest_rate: None,
                desired_debt_cap_util: None,
                credit_pool_ids: vec![],
                liquidity_multiplier_for_debt_caps: None,
                liq_queue: None,
            };
            let cosmos_msg = cdp_contract.call(create_basket_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Invalid Basket
            let edit_msg = ExecuteMsg::EditcAsset { 
                basket_id: Uint128::new(0u128), 
                asset: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                max_borrow_LTV: None, 
                max_LTV: None, 
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

            //Invalid Asset
            let edit_msg = ExecuteMsg::EditcAsset { 
                basket_id: Uint128::new(1u128), 
                asset: AssetInfo::NativeToken { denom: "not_debit".to_string() }, 
                max_borrow_LTV: None, 
                max_LTV: None,  
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

            //Successfull edit
            let edit_msg = ExecuteMsg::EditcAsset { 
                basket_id: Uint128::new(1u128), 
                asset: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                max_borrow_LTV: Some( Decimal::percent(99) ), 
                max_LTV: Some( Decimal::percent(100) ), 
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
        
            //Query Basket
            let resp: BasketResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),
            &QueryMsg::GetBasket { basket_id: Uint128::new(1u128) } 
            ).unwrap();

            assert_eq!( resp.collateral_types[0].max_borrow_LTV,  Decimal::percent(99) );
            assert_eq!( resp.collateral_types[0].max_LTV,  Decimal::percent(100) );
        }

        #[test]
        fn open_position_deposit(){           
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            
            let edit_basket_msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "2nddebit".to_string() },
                            amount: Uint128::from(0u128),
                        },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                       }  ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: None, 
                liquidity_multiplier: None, 
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]), 
                base_interest_rate: None, 
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None, 
            };
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
          

            //Testing Position creation

            //Invalid id test
            let error_exec_msg = ExecuteMsg::Deposit { 
                position_owner: None,
                basket_id: Uint128::from(1u128),
                position_id: Some(Uint128::from(3u128)),
            };

            //Fail due to a non-existent position
            //First msg deposits since no positions were initially found, meaning the _id never got tested
            app.send_tokens(Addr::unchecked("little_bank"), Addr::unchecked( "owner" ), &vec![coin( 22, "debit")] ).unwrap();
            let cosmos_msg = cdp_contract.call(error_exec_msg, vec![ coin(11, "debit") ]).unwrap();
            app.execute(Addr::unchecked("owner"), cosmos_msg.clone()).unwrap();
            app.execute(Addr::unchecked("owner"), cosmos_msg).unwrap_err();


            //Fail for invalid collateral
            let exec_msg = ExecuteMsg::Deposit { 
                position_owner: None,
                basket_id: Uint128::from(1u128),
                position_id: None,
            };

            //fail due to invalid collateral
            let cosmos_msg = cdp_contract.call(exec_msg, vec![ coin(666, "fake_debit") ]).unwrap();
            app.execute(Addr::unchecked("faker"), cosmos_msg).unwrap_err(); 

            //Successful attempt
            let exec_msg = ExecuteMsg::Deposit { 
                position_owner: None,
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            app.send_tokens(Addr::unchecked(USER), Addr::unchecked( "owner" ), &vec![coin( 11, "2nddebit")] ).unwrap();
            let cosmos_msg = cdp_contract.call(exec_msg, vec![ coin(11, "debit"), coin(11, "2nddebit") ]).unwrap();
            let res = app.execute(Addr::unchecked("owner"), cosmos_msg).unwrap();

            let response = res.events
                .into_iter()
                .find(|e| e.attributes
                    .iter()
                    .any(|attr| attr.key == "basket_id")
                )
                .ok_or_else(|| panic!("unable to find deposit event"))
                .unwrap();

            assert_eq!(
                response.attributes[1..],
                vec![
                attr("method", "deposit"),
                attr("basket_id", "1"),
                attr("position_owner","owner"),
                attr("position_id", "2"),
                attr("assets", "11 debit"),
                attr("assets", "11 2nddebit"),
                ]
            );

        }

        #[test]
        fn repay(){
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let edit_basket_msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: None, 
                pool_ids: None, 
                liquidity_multiplier: None, 
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]), 
                base_interest_rate: None, 
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None, 
            };
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();


            //NoUserPositions Error
            let repay_msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128), 
                position_id: Uint128::from(1u128), 
                position_owner:  None, 
            };
            let cosmos_msg = cdp_contract.call(repay_msg, vec![  ]).unwrap();
            let res = app.execute(Addr::unchecked("sender88"), cosmos_msg).unwrap_err();
            
            //Initial deposit
            let exec_msg = ExecuteMsg::Deposit { 
                position_owner: None,
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, coins(11, "debit")).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Invalid Collateral Error
            let repay_msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128), 
                position_id: Uint128::from(1u128), 
                position_owner:  Some(USER.to_string()), 
            };
            let cosmos_msg = cdp_contract.call(repay_msg, vec![ coin(666, "fake_debit") ]).unwrap();
            let res = app.execute(Addr::unchecked("faker"), cosmos_msg).unwrap_err();

            //NonExistent Basket Error
            let repay_msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(3u128), 
                position_id: Uint128::from(1u128), 
                position_owner: Some(USER.to_string()), 
            };
            let cosmos_msg = cdp_contract.call(repay_msg, vec![ coin(111, "credit_fulldenom") ]).unwrap();
            let res = app.execute(Addr::unchecked("coin_God"), cosmos_msg).unwrap_err();

            
            //NonExistent Position Error
            let repay_msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128), 
                position_id: Uint128::from(3u128), 
                position_owner: Some(USER.to_string()), 
            };
            let cosmos_msg = cdp_contract.call(repay_msg, vec![ coin(111, "credit_fulldenom") ]).unwrap();
            let res = app.execute(Addr::unchecked("coin_God"), cosmos_msg).unwrap_err();
            
        }

        #[test]
        fn increase_debt() {
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let edit_basket_msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: None, 
                pool_ids: None, 
                liquidity_multiplier: None, 
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]), 
                base_interest_rate: None, 
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None, 
            };
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //NoUserPositions Error
            let increase_debt_msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(1u128),
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![  ]).unwrap();
            let res = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();
            
        
            //Initial deposit
            let exec_msg = ExecuteMsg::Deposit { 
                position_owner: None,
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, vec![ coin(11, "debit") ]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //NonExistentPosition Error
            let increase_debt_msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(3u128),
                amount: Uint128::from(1u128),
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![  ]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //NonExistentBasket Error
            let increase_debt_msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(3u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(1u128),
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![  ]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

        }
        
        #[test]
        fn withdrawal_errors(){
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            let edit_basket_msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                liq_queue: None, 
                pool_ids: None, 
                liquidity_multiplier: None, 
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]), 
                base_interest_rate: None, 
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None, 
            };
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            let valid_assets: Vec<Asset> = vec![
                Asset {
                    info: AssetInfo::NativeToken { denom: "debit".to_string() },
                    amount: Uint128::from(5u128),
                }
            ];

            //User has no positions in the basket error
            let withdrawal_msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets: valid_assets.clone(), 
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![  ]).unwrap();
            let res = app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

            //Initial Deposit
            let exec_msg = ExecuteMsg::Deposit { 
                position_owner: None,
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, coins(11, "debit")).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();


            //Non-existent position error but user still has positions in the basket
            let withdrawal_msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(3u128),
                assets: vec![ Asset {
                    info: AssetInfo::NativeToken { denom: "debit".to_string() },
                    amount: Uint128::zero(),
                }], 
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![] ).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Invalid collateral fail
            let assets: Vec<Asset> = vec![
                Asset {
                    info: AssetInfo::NativeToken { denom: "notdebit".to_string() },
                    amount: Uint128::from(10u128),
                }
            ];

            let withdrawal_msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets: assets.clone(), 
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![] ).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            
            //Withdrawing too much error
            let assets: Vec<Asset> = vec![
                Asset {
                    info: AssetInfo::NativeToken { denom: "debit".to_string() },
                    amount: Uint128::from(333333333u128),
                }
            ];

            let withdrawal_msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![] ).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            
        }


        #[test]
        fn clone_basket__contract_credit_limit(){           
            let (mut app, cdp_contract, lq_contract, cw20_addr) = proper_instantiate( false, false, false, false);

            //Add a second asset
            let edit_basket_msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: Some( cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "2nddebit".to_string() },
                            amount: Uint128::from(0u128),
                        },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                       }  ), 
                owner: None, 
                liq_queue: None, 
                pool_ids: Some( vec![ 1u64 ] ),
                liquidity_multiplier: None, 
                collateral_supply_caps: Some( vec![ 
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    },
                    SupplyCap { 
                        asset_info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, 
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(), 
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false, 
                    }]), 
                base_interest_rate: None, 
                desired_debt_cap_util: None, 
                credit_asset_twap_price_source: None, 
            };
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Initial Deposit
            let exec_msg = ExecuteMsg::Deposit { 
                position_owner: None,
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, coins(11, "debit")).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query debt cap
            //Query Basket Debt Caps
            let query_msg = QueryMsg::GetBasketDebtCaps {
                basket_id: Uint128::new(1u128), 
            };
            let res: DebtCapResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.caps, String::from("debit: 0/49999, 2nddebit: 0/0, ") );

            //Clone Basket
            let msg = ExecuteMsg::CloneBasket { basket_id: Uint128::new(1u128) };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query that it was saved correctly, price as well            
            let query_msg = QueryMsg::GetBasket {
                basket_id: Uint128::new(2u128), 
            };
            let res: BasketResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.credit_price, String::from("5") );

            //Initial Deposit to basket 2
            let exec_msg = ExecuteMsg::Deposit { 
                position_owner: None,
                basket_id: Uint128::from(2u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, coins(200, "debit")).unwrap();
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query Basket Debt Caps
            //Basket 2 has over 90% of the cap
            let query_msg = QueryMsg::GetBasketDebtCaps {
                basket_id: Uint128::new(2u128), 
            };
            let res: DebtCapResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.caps, String::from("debit: 0/47392, 2nddebit: 0/0, ") );

            //Query Basket Debt Caps
            //Has less than minimum, ~2000, so gets 20000
            let query_msg = QueryMsg::GetBasketDebtCaps {
                basket_id: Uint128::new(1u128), 
            };
            let res: DebtCapResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.caps, String::from("debit: 0/20000, 2nddebit: 0/0, ") );
        
        }
    }

}
