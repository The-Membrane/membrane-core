use cosmwasm_std::{CosmosMsg, StdResult, Decimal, Binary, to_binary, WasmMsg, coin, StdError, Addr, Coin, BankMsg, Uint128, MessageInfo, Api, QuerierWrapper, Env, WasmQuery, QueryRequest};
use osmosis_std::types::osmosis::gamm::v1beta1::MsgExitPool;

use crate::types::{AssetInfo, Asset, PoolStateResponse, AssetPool}; 
use crate::apollo_router::{ExecuteMsg as RouterExecuteMsg, SwapToAssetsInput};
use crate::osmosis_proxy::QueryMsg as OsmoQueryMsg;
use crate::stability_pool::QueryMsg as SP_QueryMsg;


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
        if let AssetInfo::NativeToken { denom:_ } = asset_to_buy.clone() {

            let router_msg = RouterExecuteMsg::Swap {
                to: SwapToAssetsInput::Single(asset_to_buy.clone()), //Buy
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
            return Err(StdError::GenericErr { msg: String::from("Native assets only") })
        }
    } else {
        return Err(StdError::GenericErr { msg: String::from("Native assets only") })
    }
}

pub fn query_stability_pool_fee(
    querier: QuerierWrapper,
    stability_pool: String,
) -> StdResult<Decimal> {
    let resp: AssetPool = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: stability_pool,
        msg: to_binary(&SP_QueryMsg::AssetPool {})?,
    }))?;

    Ok(resp.liq_premium)
}

pub fn withdrawal_msg(asset: Asset, recipient: Addr) -> StdResult<CosmosMsg> {
    if let AssetInfo::NativeToken { denom: _ } = asset.clone().info {
        let coin: Coin = asset_to_coin(asset)?;
        let message = CosmosMsg::Bank(BankMsg::Send {
            to_address: recipient.to_string(),
            amount: vec![coin],
        });
        Ok(message)        
    } else {
        return Err(StdError::GenericErr { msg: String::from("Native assets only") })
    }
}

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

//Refactored Terraswap function
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

//Validate Recipient
pub fn validate_position_owner(
    deps: &dyn Api,
    info: MessageInfo,
    recipient: Option<String>,
) -> StdResult<Addr> {
    let valid_recipient: Addr = if let Some(recipient) = recipient {
        deps.addr_validate(&recipient)?
    } else {
        info.sender
    };
    Ok(valid_recipient)
}
