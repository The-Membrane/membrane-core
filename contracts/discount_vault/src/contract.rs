use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coin, to_binary, Addr, Api, BankMsg, Binary, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Uint128, WasmMsg, QueryRequest, WasmQuery, QuerierWrapper, Coin,
};
use cw2::set_contract_version;
use cw20::Cw20ExecuteMsg;

use membrane::positions::QueryMsg as CDPQueryMsg;
use membrane::helpers::{assert_sent_native_token_balance, validate_position_owner, asset_to_coin, withdrawal_msg, multi_native_withdrawal_msg, get_pool_state_response};
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::governance::{QueryMsg as Gov_QueryMsg, ProposalListResponse, ProposalStatus};
use membrane::discount_vault::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, UserResponse};
use membrane::vesting::{QueryMsg as Vesting_QueryMsg, RecipientsResponse};
use membrane::types::{Asset, AssetInfo, FeeEvent, LiqAsset, StakeDeposit, VaultedLP, VaultUser, LPPoolInfo, Basket};
use membrane::math::decimal_division;

use crate::error::ContractError;
use crate::state::{CONFIG, USERS};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:discount_vault";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_PER_DAY: u64 = 86_400u64;

// Pagination defaults
const PAGINATION_DEFAULT_LIMIT: u32 = 10;
const PAGINATION_MAX_LIMIT: u32 = 30;


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
            osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
            accepted_LPs: vec![],
        };
    } else {
        config = Config {
            owner: info.sender,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
            accepted_LPs: vec![],
        };
    }

    //Set Accepted LPs
    config.accepted_LPs = msg.accepted_LPs
        .into_iter()
        .map(|pool_id| {
            create_and_validate_LP_object(querier, pool_id, config.clone().osmosis_proxy, config.clone().positions_contract)?
        })
        .collect::<Vec<LPPoolInfo>>();

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;
    
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address))
}

fn create_and_validate_LP_object(    
    querier: QuerierWrapper,
    pool_id: u64,
    positions_contract: Addr,
    osmosis_proxy: Addr,
) -> StdResult<LPPoolInfo>{
    let res = get_pool_state_response(querier, osmosis_proxy, pool_id.clone())?;
    let share_token = AssetInfo::NativeToken { denom: res.shares.denom };
    
    //Get debt token
    let debt_token = querier.query_wasm_smart::<Basket>(positions_contract, &CDPQueryMsg::GetBasket{  })?.credit_asset.info;

    if false = res.clone().assets.into_iter().any(|deposit| deposit.denom == debt_token.to_string()){
        return Err(StdError::GenericErr { msg: format!("LP dosn't contain the debt token: {}", debt_token) })
    }

    Ok(LPPoolInfo { share_token, pool_id })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Deposit {  } => deposit(deps, env, info),
        ExecuteMsg::Withdraw { withdrawal_assets } => withdraw(deps, env, info, withdrawal_assets),
        ExecuteMsg::ChangeOwner { owner } => change_owner(deps, env, info, owner),
        ExecuteMsg::EditAcceptedLPs { pool_id, remove } => edit_LPs(deps, env, info, lp, remove),
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

fn change_owner(    
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
) -> Result<Response, ContractError>{
    let mut config = CONFIG.load(deps.storage)?;

    //Validate Authority
    if info.clone().sender != config.clone().owner{ return Err(ContractError::Unauthorized {  }) }

    //Validate owner
    let valid_owner = deps.api.addr_validate(&owner)?;

    //Set owner
    config.owner = valid_owner.clone();

    //Save config
    CONFIG.save(deps.storage, &config);

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "change_owner"),
            attr("new_owner", valid_owner)]),
    )
}

fn edit_LPs(    
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    pool_id: u64,
    remove: bool,
) -> Result<Response, ContractError>{
    let mut config = CONFIG.load(deps.storage)?;

    //Validate Authority
    if info.clone().sender != config.clone().owner{ return Err(ContractError::Unauthorized {  }) }

    //Update LPs
    if remove {
        if let Some(index, LP) = config.clone().accepted_LPs
            .into_iter()
            .enumerate()
            .find(|(i, LP)| LP.pool_id.equal(&pool_id))
            {
                //Remove
                config.accepted_LPs.remove(index);
            }
    } else {
        config.accepted_LPs.push(create_and_validate_LP_object(querier, pool_id, config.clone().osmosis_proxy, config.clone().positions_contract));
    }

    //Save config
    CONFIG.save(deps.storage, &config);

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "edit_LPs"),
            attr("edited_LP", lp)
            attr("removed", remove),]),
    )
}

fn validate_assets(
    funds: Vec<Coin>,
    accepted_LPs: Vec<LPPoolInfo>,
) -> Vec<Asset>{
    let accepted_LPs = accepted_LPs.into_iter().map(|pool| pool.share_token).collect::<Vec<AssetInfo>>();

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
        QueryMsg::User { user, minimum_deposit_time } => to_binary(&get_user_response(deps, env, msg, user, minimum_deposit_time)?),
        QueryMsg::Deposits { limit, start_after } => to_binary(&get_deposits(deps, env, msg, limit, start_after)?),
    }
}

fn get_user_response(
    deps: Deps, 
    env: Env, 
    msg: QueryMsg,
    user: String,
    minimum_deposit_time: Option<u64>, //in days
) -> StdResult<UserResponse>{
    let config = CONFIG.load(deps.storage)?;
    let minimum_deposit_time = minimum_deposit_time.unwrap_or_else(|| 0u64) * SECONDS_PER_DAY;
    let mut vault_user = USERS.load(deps.storage, deps.api.addr_validate(&user)?)?;

    //Enforce minimum_deposit_time
    vault_user.vaulted_lps = vault_user.clone().vaulted_lps
        .into_iter()
        .filter(|deposit| deposit.deposit_time - env.block.time.seconds() >= minimum_deposit_time)
        .collect::<Vec<VaultedLP>>();
    
    //Get Positions Basket
    let basket: Basket = deps.querier
        .query_wasm_smart::<Basket>(config.clone().positions_contract, &CDPQueryMsg::GetBasket{  })?;


    let mut LP_value = Uint128::zero();
    //Calculate total vaulted value
    for lp in vault_user.clone().vaulted_lps{
        //Find the LPPoolInfo that matches the share token
        if let Some(pool_info) = config.clone().accepted_LPs.into_iter().find(|info| info.share_token.equal(&lp.gamm)){
            //Query total share asset amounts
            let share_asset_amounts: Vec<osmosis_std::types::cosmos::base::v1beta1::Coin> = querier
                .query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                    contract_addr: config.clone().osmosis_proxy.into(),
                    msg: to_binary(&OsmoQueryMsg::PoolState {
                        id: pool_info.pool_id,
                    })?,
                }))?
                .shares_value(lp.amount);
            //Add the share asset that is the debt token
            if let Some(coin) = share_asset_amounts.into_iter().find(|coin| coin.denom == basket.clone().credit_asset.info.to_string()){
                LP_value += coin.amount * basket.clone().credit_price;
            }
            
        }
    }
    //Multiply LP value by 2 to account for the non-debt side
    LP_value = LP_value * Uint128::new(2);

    Ok(UserResponse { user, deposits: vault_user.vaulted_lps, discount_value: LP_value })

    //Only counting LPs that match accepted LPs skips removed LPs.
    //Withdrawals of removed LPs still work tho
}

fn get_deposits(    
    deps: Deps, 
    env: Env, 
    msg: QueryMsg,
    limit: Option<u64>,
    start_after: Option<String>, //user
) -> StdResult<Vec<VaultedLP>>{

    let limit = option_limit
        .unwrap_or(PAGINATION_DEFAULT_LIMIT)
        .min(PAGINATION_MAX_LIMIT) as usize;
    
    let option_start = start_after.map(Bound::exclusive);

    Ok(USERS
        .range(deps.storage, option_start, None, Order::Ascending)
        .map(|user| {
            let (addr, user) = user.unwrap();
            
            user.vaulted_lps
        })
        .take(limit)
        .collect::<Vec<VaultedLP>>())
}