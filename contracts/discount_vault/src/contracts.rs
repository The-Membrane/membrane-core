use std::env;
use std::str::FromStr;

#[cfg(not(feature = "library"))]
use cw_storage_plus::Bound;
use cosmwasm_std::{entry_point, Attribute};
use cosmwasm_std::{
    attr, to_binary, Addr, Binary, Deps, Order,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult, Uint128, QuerierWrapper, Coin,
};
use cw2::set_contract_version;

use membrane::cdp::QueryMsg as CDPQueryMsg;
use membrane::helpers::{multi_native_withdrawal_msg, get_pool_state_response, accrue_user_positions};
use membrane::discount_vault::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, UserResponse};
use membrane::types::{Asset, AssetInfo, VaultedLP, VaultUser, LPPoolInfo, Basket};

use crate::error::ContractError;
use crate::state::{CONFIG, USERS, OWNERSHIP_TRANSFER};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:discount_vault";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
pub const SECONDS_PER_DAY: u64 = 86_400u64;

// Pagination defaults
const PAGINATION_DEFAULT_LIMIT: u64 = 10;
const PAGINATION_MAX_LIMIT: u64 = 30;


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
            deposits_enabled: true,
        };
    } else {
        config = Config {
            owner: info.sender,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
            accepted_LPs: vec![],
            deposits_enabled: true,
        };
    }
    let mut err: Option<StdError> = None;

    //Set Accepted LPs
    config.accepted_LPs = msg.accepted_LPs
        .into_iter()
        .map(|pool_id| {
            match create_and_validate_LP_object(deps.querier, pool_id, config.clone().positions_contract, config.clone().osmosis_proxy){
                Ok(pool) => pool,
                Err(error) => {
                    err = Some(error);
                    LPPoolInfo {
                        share_token: AssetInfo::NativeToken { denom: String::from("")},
                        pool_id: 0,
                    }
                }
            }
        })
        .collect::<Vec<LPPoolInfo>>();
    if let Some(err) = err{ return Err(ContractError::Std(err)) }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;
    
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address))
}

/// Add a new LP to the accepted LPs list with the given pool id.
/// Query info from Osmosis Proxy and validate that the LP contains the debt token.
fn create_and_validate_LP_object(    
    querier: QuerierWrapper,
    pool_id: u64,
    positions_contract: Addr,
    osmosis_proxy: Addr,
) -> StdResult<LPPoolInfo>{
    let res = get_pool_state_response(querier, osmosis_proxy.to_string(), pool_id.clone())?;
    let share_token = AssetInfo::NativeToken { denom: res.clone().shares.denom };
    
    //Get debt token
    let basket_res: Basket = querier.query_wasm_smart(positions_contract, &CDPQueryMsg::GetBasket{  })?;
    let debt_token = basket_res.credit_asset.info;

    if let false = res.clone().assets.into_iter().any(|deposit| deposit.denom == debt_token.to_string()){
        return Err(StdError::GenericErr { msg: format!("LP doesn't contain the debt token: {}", debt_token) })
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
        ExecuteMsg::Withdraw { withdrawal_assets } => withdraw(deps, info, withdrawal_assets),
        ExecuteMsg::ChangeOwner { owner } => change_owner(deps, info, owner),
        ExecuteMsg::EditAcceptedLPs { pool_ids, remove } => edit_LPs(deps, info, pool_ids, remove),
        ExecuteMsg::ToggleDeposits { enable } => toggle_deposits(deps, info, enable),
    }
}

/// Deposit accepted LPs into the vault.
fn deposit(    
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError>{
    let config = CONFIG.load(deps.storage)?;
    let valid_assets = validate_assets(info.clone().funds, config.clone().accepted_LPs)?;
    if valid_assets.len() < info.clone().funds.len(){ return Err(ContractError::InvalidAsset {  }) }

    //Add deposits to User
    match USERS.load(deps.storage, info.clone().sender){
        Ok(_user) => {
            //Add to user
            USERS.update(deps.storage, info.clone().sender, |user| -> StdResult<VaultUser>{
                let mut user = user.unwrap();

                //Push deposits
                for asset in valid_assets.clone() {                    
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
            let vaulted_lps = valid_assets.clone()
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
            })?;
        },
    };
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "deposit"),
            attr("user", info.clone().sender),
            attr("deposits", format!("{:?}", valid_assets)),
        ]))
}

/// Withdraw LPs from the vault.
fn withdraw(    
    deps: DepsMut,
    info: MessageInfo,
    mut withdrawal_assets: Vec<Asset>,
) -> Result<Response, ContractError>{
    let config = CONFIG.load(deps.storage)?;
    let mut user = USERS.load(deps.storage, info.clone().sender)?;

    //Remove unowned assets
    for (index, asset) in withdrawal_assets.clone().into_iter().enumerate(){
        if let false = user.clone().vaulted_lps.into_iter().any(|deposit| deposit.gamm.equal(&asset.info)){
            withdrawal_assets.remove(index);
        }
    }    

    //Update deposits
    for (index, mut withdrawal_asset) in withdrawal_assets.clone().into_iter().enumerate(){
        //Comb thru deposits
        for (i, deposit) in user.clone().vaulted_lps.into_iter().enumerate(){
            //If the withdrawl_asset == the deposited asset
            if withdrawal_asset.info.equal(&deposit.gamm) && withdrawal_asset.amount != Uint128::zero(){
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
        //If any withdrawals aren't fulfilled, i.e. amount != 0, then error  
        if withdrawal_asset.amount != Uint128::zero(){
            return Err(ContractError::InvalidWithdrawal { val: withdrawal_assets[index].clone() })
        }
    }
    //Save updated deposits for User
    USERS.save(deps.storage, info.clone().sender, &user)?;  

    //Create withdrawl_msgs
    let withdraw_msg = multi_native_withdrawal_msg(withdrawal_assets.clone(), info.clone().sender)?;

    //Create Position accrual msgs to lock in user discounts before withdrawing
    let accrual_msg = accrue_user_positions(
        deps.querier, 
        config.positions_contract.to_string(),
        info.clone().sender.to_string(), 
        PAGINATION_MAX_LIMIT as u32
    )?;

    Ok(Response::new()
        .add_message(accrual_msg)
        .add_message(withdraw_msg)
        .add_attributes(vec![
            attr("method", "withdraw"),
            attr("user", info.clone().sender),
            attr("withdrawals", format!("{:?}", withdrawal_assets)),
        ]))
}

/// Change the owner of the contract.
fn change_owner(    
    deps: DepsMut,
    info: MessageInfo,
    owner: String,
) -> Result<Response, ContractError>{
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs: Vec<Attribute> = vec![attr("method", "change_owner")];
    
    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;

            //Save new owner
            CONFIG.save(deps.storage, &config)?;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    //Validate owner
    let valid_addr = deps.api.addr_validate(&owner)?;

    //Set owner transfer state
    OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
    attrs.push(attr("owner_transfer", valid_addr));  


    Ok(Response::new()
        .add_attributes(attrs),
    )
}

/// Add or remove LPs from list of accepted LPs.
fn edit_LPs(    
    deps: DepsMut,
    info: MessageInfo,
    pool_ids: Vec<u64>,
    remove: bool,
) -> Result<Response, ContractError>{
    let mut config = CONFIG.load(deps.storage)?;

    //Validate Authority
    if info.clone().sender != config.clone().owner{ return Err(ContractError::Unauthorized {  }) }

    //Update LPs
    if remove {
        for id in pool_ids.clone() {
            
            if let Some((index, _LP)) = config.clone().accepted_LPs
            .into_iter()
            .enumerate()
            .find(|(_i, LP)| LP.pool_id == id)
            {
                //Remove
                config.accepted_LPs.remove(index);
            }
        }
    } else {
        for id in pool_ids.clone() {            
            config.accepted_LPs.push(create_and_validate_LP_object(deps.querier, id, config.clone().positions_contract, config.clone().osmosis_proxy)?);
        }
    }

    //Save config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "edit_LPs"),
            attr("edited_pools", format!("{:?}", pool_ids)),
            attr("removed", remove.to_string())]),
    )
}

/// Toggle the ability to deposit LPs.
fn toggle_deposits(    
    deps: DepsMut,
    info: MessageInfo,
    toggle: bool,
) -> Result<Response, ContractError>{
    let mut config = CONFIG.load(deps.storage)?;

    //Validate Authority
    if info.clone().sender != config.clone().owner{ return Err(ContractError::Unauthorized {  }) }

    //Toggle
    config.deposits_enabled = toggle;

    //Save config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "toggle_deposit"),
            attr("toggle", toggle.to_string())]),
    )
}


/// Validate assets and return only those that are accepted.
fn validate_assets(
    funds: Vec<Coin>,
    accepted_LPs: Vec<LPPoolInfo>,
) -> StdResult<Vec<Asset>>{
    let accepted_LPs = accepted_LPs.into_iter().map(|pool| pool.share_token).collect::<Vec<AssetInfo>>();

    let valid_assets: Vec<Asset> = funds.clone()
        .into_iter()
        .filter(|coin| accepted_LPs.clone().iter().any(|lp| lp.equal(&AssetInfo::NativeToken { denom: coin.clone().denom } )))
        .map(|coin| Asset {
            amount: coin.amount,
            info: AssetInfo::NativeToken { denom: coin.clone().denom },
        })
        .collect::<Vec<Asset>>();

    //Assert that all assets are accepted
    if funds.len() != valid_assets.len() {
        return Err(StdError::GenericErr{msg: "Invalid asset(s)".into()})
    }

    Ok(valid_assets)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::User { user, minimum_deposit_time } => to_binary(&get_user_response(deps, env, user, minimum_deposit_time)?),
        QueryMsg::Deposits { limit, start_after } => to_binary(&get_deposits(deps, limit, start_after)?),
    }
}

/// Return UserResponse for a given user. 
/// Return the LP value so that the System Discounts contract can calculate the discount.
/// LPs are assumed 50:50
fn get_user_response(
    deps: Deps, 
    env: Env, 
    user: String,
    minimum_deposit_time: Option<u64>, //in days
) -> StdResult<UserResponse>{
    let config = CONFIG.load(deps.storage)?;
    let minimum_deposit_time = minimum_deposit_time.unwrap_or_else(|| 0u64) * SECONDS_PER_DAY;
    let mut vault_user = USERS.load(deps.storage, deps.api.addr_validate(&user)?)?;

    //Enforce minimum_deposit_time
    vault_user.vaulted_lps = vault_user.clone().vaulted_lps
        .into_iter()
        .filter(|deposit| env.block.time.seconds() - deposit.deposit_time  >= minimum_deposit_time)
        .collect::<Vec<VaultedLP>>();
    
    //Get Positions Basket
    let basket: Basket = deps.querier
        .query_wasm_smart(config.clone().positions_contract, &CDPQueryMsg::GetBasket{  })?;


    let mut LP_value = Uint128::zero();
    //Calculate total vaulted value
    for lp in vault_user.clone().vaulted_lps{
        //Find the LPPoolInfo that matches the share token
        if let Some(pool_info) = config.clone().accepted_LPs.into_iter().find(|info| info.share_token.equal(&lp.gamm)){
            //Query total share asset amounts
            let share_asset_amounts: Vec<osmosis_std::types::cosmos::base::v1beta1::Coin> = 
            get_pool_state_response(deps.querier,  config.clone().osmosis_proxy.into(), pool_info.pool_id)?.shares_value(lp.amount);
            //Add the share asset that is the debt token
            if let Some(coin) = share_asset_amounts.into_iter().find(|coin| coin.denom == basket.clone().credit_asset.info.to_string()){
                LP_value += basket.clone().credit_price.get_value(Uint128::from_str(&coin.amount).unwrap())?.to_uint_floor();
            }
        }
    }
    //Multiply, LP value by 2 to account for the non-debt side
    //Assumption of a 50:50 LP, meaning unbalanced stableswaps are boosted
    //This could be a "bug" but for now it's a feature to benefit LPs during distressed times
    LP_value = LP_value * Uint128::new(2);

    Ok(UserResponse { user, deposits: vault_user.vaulted_lps, discount_value: LP_value })

    //Only counting LPs that match accepted LPs skips removed LPs.
    //Withdrawals of removed LPs still work tho
}

/// Return deposits for a given user.
fn get_deposits(    
    deps: Deps, 
    option_limit: Option<u64>,
    start_after: Option<String>, //user
) -> StdResult<Vec<VaultUser>>{

    let limit = option_limit
        .unwrap_or(PAGINATION_DEFAULT_LIMIT)
        .min(PAGINATION_MAX_LIMIT) as usize;
    
    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    } else {
        None
    };
    let mut lps: Vec<VaultUser> = vec![];

    let _iter = USERS
        .range(deps.storage, start, None, Order::Ascending)
        .map(|user| {
            let (_addr, user) = user.unwrap();
            
            lps.push(user);
        });
    lps = lps.clone()
        .into_iter()
        .take(limit)
        .collect::<Vec<VaultUser>>();

    Ok(lps)
}
