use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coin, to_binary, Addr, Api, BankMsg, Binary, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Uint128, WasmMsg, QueryRequest, WasmQuery, QuerierWrapper, Coin,
};
use cw2::set_contract_version;
use cw20::Cw20ExecuteMsg;

use membrane::apollo_router::{Cw20HookMsg as RouterCw20HookMsg, ExecuteMsg as RouterExecuteMsg, SwapToAssetsInput};
use membrane::helpers::{assert_sent_native_token_balance, validate_position_owner, asset_to_coin, withdrawal_msg, multi_native_withdrawal_msg};
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::governance::{QueryMsg as Gov_QueryMsg, ProposalListResponse, ProposalStatus};
use membrane::discount_vault::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::vesting::{QueryMsg as Vesting_QueryMsg, RecipientsResponse};
use membrane::types::{Asset, AssetInfo, FeeEvent, LiqAsset, StakeDeposit, VaultedLP, VaultUser};
use membrane::math::decimal_division;

use crate::error::ContractError;
use crate::state::{CONFIG, USERS};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:discount_vault";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_PER_DAY: u64 = 86_400u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config: Config;

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            accepted_LPs: msg.accepted_LPs,
        };
    } else {
        config = Config {
            owner: info.sender,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            accepted_LPs: msg.accepted_LPs,
        };
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;
    
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address))
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
    }
}

fn deposit(    
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError>{
    let config = CONFIG.load(deps.storage)?;
    let valid_assets = validate_assets(info.clone().funds, config.clone().accepted_LPs);

    //Add deposits to User
    match USERS.load(deps.storage, info.clone().sender){
        Ok(_user) => {
            //Add to user
            USERS.update(deps.storage, info.clone().sender, |user| -> StdResult<VaultUser>{
                let mut user = user.unwrap();

                //Push deposits
                for asset in valid_assets {                    
                    user.vaulted_lps.push(
                        VaultedLP {
                            gamm: asset.info,
                            amount: asset.amount,
                            deposit_time: env.block.time.seconds(),
                        }
                    );
                }
                Ok(user)
            })?;
        },
        Err(_err) => {
            //Create list of vaulted LPs
            let vaulted_lps = valid_assets
                .into_iter()
                .map(|asset| VaultedLP {
                    gamm: asset.info,
                    amount: asset.amount,
                    deposit_time: env.block.time.seconds(),
                })
                .collect::<Vec<VaultedLP>>();

            //Create & save new User
            USERS.save(deps.storage, info.clone().sender, &VaultUser {
                user: info.clone().sender,
                vaulted_lps,
            });
        },
    };
    
    Ok(Response::new().add_message(withdraw_msg)
        .add_attributes(vec![
            attr("method", "deposit"),
            attr("user", info.clone().sender),
            attr("deposits", format!("{:?}", valid_assets)),
        ]))
}

fn withdraw(    
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mut withdrawal_assets: Vec<Asset>,
) -> Result<Response, ContractError>{
    let mut user = USERS.load(deps.storage, info.clone().sender)?;

    //Remove invalid & unowned assets
    for (index, asset) in withdrawal_assets.clone().into_iter().enumerate(){
        if false = user.clone().vaulted_lps.into_iter().any(|deposit| deposit.gamm.equal(&asset.info)){
            withdrawal_assets.remove(index);
        }
    }    

    //Update deposits
    for mut withdrawal_asset in withdrawal_assets.clone().into_iter(){
        //Comb thru deposits
        for (i, deposit) in user.clone().vaulted_lps.into_iter().enumerate(){
            //If the withdrawl_asset == the deposited asset
            if withdrawal_asset.info.equal(&deposit.gamm) && withdrawal_asset != Uint128::zero(){
                //Remove claims from deposit
                if withdrawal_asset.amount >= deposit.amount{
                    withdrawal_asset.amount -= deposit.amount;
                    user.vaulted_lps[i].amount = Uint128::zero();
                } else {                    
                    user.vaulted_lps[i].amount -= withdrawal_asset.amount;
                    withdrawal_asset.amount = Uint128::zero();
                }
            }
        }
    }
    //Save updated deposits for User
    USERS.save(deps.storage, info.clone().sender, &user);

    //If any withdrawal_assets aren't 0 then error
    for asset in withdrawal_assets.clone(){
        if asset.amount != Uint128::zero(){
            return Err(ContractError::InvalidWithdrawal { val: asset })
        }
    }    

    //Create withdrawl_msgs
    let withdraw_msg = multi_native_withdrawal_msg(withdrawal_assets, info.clone().sender)?;

    Ok(Response::new().add_message(withdraw_msg)
        .add_attributes(vec![
            attr("method", "withdraw"),
            attr("user", info.clone().sender),
            attr("withdrawals", format!("{:?}", withdrawal_assets)),
        ]))
}



fn validate_assets(
    funds: Vec<Coin>,
    accepted_LPs: Vec<AssetInfo>,
) -> Vec<Asset>{
    funds
        .into_iter()
        .filter(|coin| accepted_LPs.clone().iter().any(|lp| lp == AssetInfo::NativeToken { denom: coin.denom } ))
        .map(|coin| Asset {
            amount: coin.amount,
            info: AssetInfo::NativeToken { denom: coin.denom },
        })
        .collect::<Vec<Asset>>()    
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
    }
}

