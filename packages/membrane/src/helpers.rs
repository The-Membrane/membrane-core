use cosmwasm_std::{CosmosMsg, StdResult, Decimal, Binary, to_binary, WasmMsg, coin, StdError, Addr, Coin, BankMsg, Uint128, MessageInfo, Api, QuerierWrapper, Env, WasmQuery, QueryRequest};
use osmosis_std::types::osmosis::gamm::v1beta1::MsgExitPool;

use crate::types::{AssetInfo, Asset, PoolStateResponse, AssetPool}; 
use crate::apollo_router::{ExecuteMsg as RouterExecuteMsg, SwapToAssetsInput};
use crate::osmosis_proxy::QueryMsg as OsmoQueryMsg;
use crate::liquidity_check::QueryMsg as LiquidityQueryMsg;
use crate::stability_pool::QueryMsg as SP_QueryMsg;

//Constants
pub const SECONDS_PER_YEAR: u64 = 31_536_000u64;

/// Returns asset liquidity from the liquidity check contract
pub fn get_asset_liquidity(
    querier: QuerierWrapper,
    liquidity_contract: String,
    asset_info: AssetInfo,
) -> StdResult<Uint128> {
    let total_pooled: Uint128 = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: liquidity_contract,
        msg: to_binary(&LiquidityQueryMsg::Liquidity { asset: asset_info })?,
    }))?;

    Ok(total_pooled)   
}

/// Query Osmosis proxy for pool state then create & return LP withdraw msg
pub fn pool_query_and_exit(
    querier: QuerierWrapper,
    env: Env,
    osmosis_proxy: String,
    pool_id: u64,
    amount_to_withdraw: Uint128,
) -> StdResult<(CosmosMsg, Vec<osmosis_std::types::cosmos::base::v1beta1::Coin>)>{

    //Query total share asset amounts
    let share_asset_amounts: Vec<osmosis_std::types::cosmos::base::v1beta1::Coin> = querier
    .query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: osmosis_proxy,
        msg: to_binary(&OsmoQueryMsg::PoolState {
            id: pool_id,
        })?,
    }))?
    .shares_value(amount_to_withdraw);
    
    //Create LP withdraw msg
    let mut token_out_mins: Vec<osmosis_std::types::cosmos::base::v1beta1::Coin> = vec![];
    for token in share_asset_amounts.clone() {
        token_out_mins.push(osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: token.denom,
            amount: token.amount.to_string(),
        });
    }

    Ok((MsgExitPool {
        sender: env.contract.address.to_string(),
        pool_id,
        share_in_amount: amount_to_withdraw.to_string(),
        token_out_mins,
    }
    .into(), share_asset_amounts))

}

/// Returns [`PoolStateResponse`] from Osmosis proxy
pub fn get_pool_state_response(
    querier: QuerierWrapper,
    osmosis_proxy: String,
    pool_id: u64
) -> StdResult<PoolStateResponse>{
    //Query Pool State
    querier.query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: osmosis_proxy,
        msg: to_binary(&OsmoQueryMsg::PoolState {
            id: pool_id,
        })?,
    }))
}

/// Creates router swap msg between native assets
pub fn router_native_to_native(
    router_addr: String,
    asset_to_sell: AssetInfo,
    asset_to_buy: AssetInfo,
    max_spread: Option<Decimal>,
    recipient: Option<String>,
    hook_msg: Option<Binary>,
    amount_to_sell: u128,
) -> StdResult<CosmosMsg>{
    if let AssetInfo::NativeToken { denom } = asset_to_sell {
        if let AssetInfo::NativeToken { denom:_ } = asset_to_buy {

            let router_msg = RouterExecuteMsg::Swap {
                to: SwapToAssetsInput::Single(asset_to_buy), //Buy
                max_spread, 
                recipient,
                hook_msg,
            };
    
            let payment = coin(
                amount_to_sell,
                denom,
            );
    
            let msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: router_addr,
                msg: to_binary(&router_msg)?,
                funds: vec![payment],
            });
    
            Ok(msg)            
        } else {
            Err(StdError::GenericErr { msg: String::from("Native assets only") })
        }
    } else {
        Err(StdError::GenericErr { msg: String::from("Native assets only") })
    }
}

/// Returns Stability Pool liq premium
pub fn query_stability_pool_fee(
    querier: QuerierWrapper,
    stability_pool: String,
) -> StdResult<Decimal> {
    let resp: AssetPool = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: stability_pool,
        msg: to_binary(&SP_QueryMsg::AssetPool { 
            user: None,
            deposit_limit: 1.into(),
            start_after: None,
        })?,
    }))?;

    Ok(resp.liq_premium)
}

/// Get contract balances for list of assets
pub fn get_contract_balances(
    querier: QuerierWrapper,
    env: Env,
    assets: Vec<AssetInfo>,
) -> StdResult<Vec<Uint128>> {
    let mut balances = vec![];

    for asset in assets {
        if let AssetInfo::NativeToken { denom } = asset {
            balances.push(
                querier
                    .query_balance(env.clone().contract.address, denom)?
                    .amount,
            );
        }        
    }

    Ok(balances)
}

/// Build withdraw msg for native assets
pub fn withdrawal_msg(asset: Asset, recipient: Addr) -> StdResult<CosmosMsg> {
    if let AssetInfo::NativeToken { denom: _ } = asset.clone().info {
        let coin: Coin = asset_to_coin(asset)?;
        let message = CosmosMsg::Bank(BankMsg::Send {
            to_address: recipient.to_string(),
            amount: vec![coin],
        });
        Ok(message)        
    } else {
        Err(StdError::GenericErr { msg: String::from("Native assets only") })
    }
}

/// Builds withdraw msg for multiple native assets
pub fn multi_native_withdrawal_msg(assets: Vec<Asset>, recipient: Addr) -> StdResult<CosmosMsg> {    
    let coins: Vec<Coin> = assets
        .into_iter()
        .map(native_asset_to_coin)
        .collect::<Vec<Coin>>();
    let message = CosmosMsg::Bank(BankMsg::Send {
        to_address: recipient.to_string(),
        amount: coins,
    });
    Ok(message)   
}

/// Converts native Asset to Coin
pub fn native_asset_to_coin(asset: Asset) -> Coin {    
    Coin {
        denom: asset.info.to_string(),
        amount: asset.amount,
    }    
}

/// Converts Asset to Coin
pub fn asset_to_coin(asset: Asset) -> StdResult<Coin> {
    match asset.info {
        //
        AssetInfo::Token { address: _ } => {
            Err(StdError::GenericErr {
                msg: String::from("CW20 Assets can't be converted into Coin"),
            })
        }
        AssetInfo::NativeToken { denom } => Ok(Coin {
            denom,
            amount: asset.amount,
        }),
    }
}
/// Asserts balance of native tokens sent to the contract
/// Refactored Terraswap function.
pub fn assert_sent_native_token_balance(
    asset_info: AssetInfo,
    message_info: &MessageInfo,
) -> StdResult<Asset> {
    let asset: Asset;

    if let AssetInfo::NativeToken { denom } = &asset_info {
        match message_info.funds.iter().find(|x| x.denom == *denom) {
            Some(coin) => {
                if coin.amount > Uint128::zero() {
                    asset = Asset {
                        info: asset_info,
                        amount: coin.amount,
                    };
                } else {
                    return Err(StdError::generic_err("You gave me nothing to deposit"));
                }
            }
            None => {
                return Err(StdError::generic_err(
                    "Incorrect denomination, sent asset denom and asset.info.denom differ",
                ))
            }
        }
    } else {
        return Err(StdError::generic_err(
            "Asset type not native, check Msg schema and use AssetInfo::Token{ address: Addr }",
        ));
    }

    Ok(asset)
}

/// Returns valid addr for contract usage
pub fn validate_position_owner(
    deps: &dyn Api,
    info: MessageInfo,
    recipient: Option<String>,
) -> StdResult<Addr> {
    recipient.map_or_else(|| Ok(info.sender), |x| deps.addr_validate(&x))
}

/// Accumulate interest to a given base amount
pub fn accumulate_interest(base: Uint128, rate: Decimal, time_elapsed: u64) -> StdResult<Uint128> {
    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    Ok(base * applied_rate)
}