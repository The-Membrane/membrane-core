use std::env;
use std::error::Error;
use std::ops::Index;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, StdError, Storage, Addr, Api, Uint128, CosmosMsg, BankMsg, WasmMsg, Coin, Decimal, BankQuery, BalanceResponse, QueryRequest, WasmQuery, QuerierWrapper, attr, from_binary};
use cw2::set_contract_version;
use membrane::positions::{ExecuteMsg as CDP_ExecuteMsg, Cw20HookMsg as CDP_Cw20HookMsg};
use membrane::stability_pool::{ExecuteMsg, InstantiateMsg, QueryMsg, LiquidatibleResponse, DepositResponse, ClaimsResponse, PoolResponse, Cw20HookMsg };
use membrane::apollo_router::{ ExecuteMsg as RouterExecuteMsg, Cw20HookMsg as RouterCw20HookMsg };
use membrane::osmosis_proxy::{ QueryMsg as OsmoQueryMsg, ExecuteMsg as OsmoExecuteMsg, TokenInfoResponse };
use membrane::types::{ Asset, AssetInfo, LiqAsset, AssetPool, Deposit, cAsset, UserRatio, User, PositionUserInfo, UserInfo };
use cw20::{Cw20ExecuteMsg, Cw20QueryMsg, Cw20ReceiveMsg};

use crate::error::ContractError;
use crate::math::{decimal_division, decimal_subtraction, decimal_multiplication};
use crate::state::{ ASSETS, CONFIG, Config, USERS, PROP, Propagation, INCENTIVES };

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:stability-pool";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Timeframe constants
const SECONDS_PER_YEAR: u64 = 31_536_000u64;
const SECONDS_PER_DAY: u64 = 86_400u64;

//FIFO Stability Pool

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {

    let mut config: Config;
    
    if msg.owner.is_some(){
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?, 
            incentive_rate: msg.incentive_rate.unwrap_or_else(|| Decimal::percent(0)),
            max_incentives: msg.max_incentives.unwrap_or_else(|| Uint128::new(70_000_000_000_000)),
            desired_ratio_of_total_credit_supply: msg.desired_ratio_of_total_credit_supply.unwrap_or_else(|| Decimal::percent(0)),
            unstaking_period: 1u64,
            mbrn_denom: msg.mbrn_denom,
            osmosis_proxy: deps.api.addr_validate( &msg.osmosis_proxy )?,
            positions_contract: deps.api.addr_validate( &msg.positions_contract )?,
            dex_router: None,
            max_spread: msg.max_spread,
        };
    }else{
        config = Config {
            owner: info.sender.clone(),
            incentive_rate: msg.incentive_rate.unwrap_or_else(|| Decimal::percent(0)),
            max_incentives: msg.max_incentives.unwrap_or_else(|| Uint128::new(70_000_000_000_000)),
            desired_ratio_of_total_credit_supply: msg.desired_ratio_of_total_credit_supply.unwrap_or_else(|| Decimal::percent(0)),
            unstaking_period: 1u64,
            mbrn_denom: msg.mbrn_denom,
            osmosis_proxy: deps.api.addr_validate( &msg.osmosis_proxy )?,
            positions_contract: deps.api.addr_validate( &msg.positions_contract )?,
            dex_router: None,  
            max_spread: msg.max_spread,
        };
    }

    // //Set optional config parameters
    match msg.dex_router {
        Some( address ) => {
            
            match deps.api.addr_validate( &address ){
                Ok( addr ) => config.dex_router = Some( addr ),
                Err(_) => {},
            }
        },
        None => {},
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    //Initialize the propagation object
    PROP.save( deps.storage, &Propagation { repaid_amount: Uint128::zero() })?;

    //Initialize Incentive Total
    INCENTIVES.save( deps.storage, &Uint128::zero() )?;
    
    if msg.asset_pool.is_some() {

        let mut pool = msg.asset_pool.unwrap();

        pool.deposits = vec![];

        ASSETS.save(deps.storage, &vec![pool])?;
    }
    
    let mut res = Response::new();
    let mut attrs = vec![];

    attrs.push(("method", "instantiate"));

    let c = &config.owner.to_string();
    attrs.push(("owner", c));

    
    Ok( res.add_attributes(attrs) )
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig { 
            owner, 
            incentive_rate,
            max_incentives, 
            desired_ratio_of_total_credit_supply,
            unstaking_period,
            mbrn_denom, 
            osmosis_proxy,
            positions_contract,
            dex_router, 
            max_spread 
        } => update_config(deps, info, owner, incentive_rate, max_incentives, desired_ratio_of_total_credit_supply, unstaking_period, mbrn_denom, osmosis_proxy, positions_contract, dex_router, max_spread),
        ExecuteMsg::Receive( msg ) => receive_cw20(deps, env, info, msg),
        ExecuteMsg::Deposit{ user, assets } => {
            //Outputs asset objects w/ correct amounts
            let valid_assets = validate_assets(deps.storage, assets.clone(), info.clone(), true)?;
            if valid_assets.len() == 0 { return Err( ContractError::CustomError { val: "No valid assets".to_string() } ) }

            deposit( deps, env, info, user, valid_assets )
        },
        ExecuteMsg::Withdraw{ assets }=> withdraw( deps, env, info, assets ),
        ExecuteMsg::Liquidate { credit_asset } => liquidate( deps, info, credit_asset ),
        ExecuteMsg::Claim { claim_as_native, claim_as_cw20, deposit_to } => claim( deps, info, claim_as_native, claim_as_cw20, deposit_to ),
        ExecuteMsg::AddPool { asset_pool } => add_asset_pool( deps, info, asset_pool.credit_asset, asset_pool.liq_premium ),
        ExecuteMsg::Distribute { distribution_assets, distribution_asset_ratios, credit_asset, distribute_for } => distribute_funds( deps, info, None, env, distribution_assets, distribution_asset_ratios, credit_asset, distribute_for ), 
        ExecuteMsg::Repay { user_info, repayment } => repay( deps, env, info, user_info, repayment ),
    }
}

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    incentive_rate: Option<Decimal>,
    max_incentives: Option<Uint128>,
    desired_ratio_of_total_credit_supply: Option<Decimal>,
    unstaking_period: Option<u64>,
    mbrn_denom: Option<String>,
    osmosis_proxy: Option<String>,
    positions_contract: Option<String>,
    dex_router: Option<String>,
    max_spread: Option<Decimal>,
) -> Result<Response, ContractError>{

    let mut config = CONFIG.load( deps.storage )?;

    //Assert Authority
    if info.sender != config.owner { return Err( ContractError::Unauthorized {  } ) }

    let mut attrs = vec![
        attr( "method", "update_config" ),  
    ];

    //Match Optionals
    if let Some( owner ) = owner { 
            let valid_addr = deps.api.addr_validate(&owner)?;
            config.owner = valid_addr.clone();
            attrs.push( attr("new_owner", valid_addr.to_string()) );
    }
    if let Some( mbrn_denom ) = mbrn_denom { 
            config.mbrn_denom = mbrn_denom.clone() ;
            attrs.push( attr("new_mbrn_denom", mbrn_denom.to_string()) );
    }
    if let Some( osmosis_proxy ) = osmosis_proxy { 
            let valid_addr = deps.api.addr_validate(&osmosis_proxy)?;
            config.osmosis_proxy = valid_addr.clone();
            attrs.push( attr("new_osmosis_proxy", valid_addr.to_string()) );
    }
    if let Some( positions_contract ) = positions_contract { 
            let valid_addr = deps.api.addr_validate(&positions_contract)?;
            config.positions_contract = valid_addr.clone();
            attrs.push( attr("new_positions_contract", valid_addr.to_string()) );
    }
    if let Some( dex_router ) = dex_router { 
            let valid_addr = deps.api.addr_validate(&dex_router)?;
            config.dex_router = Some( valid_addr.clone() );
            attrs.push( attr("new_dex_router", valid_addr.to_string()) );
    }
    if let Some( max_spread ) = max_spread { 
            config.max_spread = Some( max_spread.clone() );
            attrs.push( attr("new_max_spread", max_spread.to_string()) );
    }
    if let Some( incentive_rate ) = incentive_rate { 
            config.incentive_rate = incentive_rate.clone();
            attrs.push( attr("new_incentive_rate", incentive_rate.to_string()) );
    }
    if let Some( max_incentives ) = max_incentives { 
            config.max_incentives = max_incentives.clone();
            attrs.push( attr("new_max_incentives", max_incentives.to_string()) );
    }
    if let Some( desired_ratio_of_total_credit_supply ) = desired_ratio_of_total_credit_supply{ 
            config.desired_ratio_of_total_credit_supply = desired_ratio_of_total_credit_supply.clone();
            attrs.push( attr("new_desired_ratio_of_total_credit_supply", desired_ratio_of_total_credit_supply.to_string()) );
    }
    if let Some( new_unstaking_period ) = unstaking_period {
        config.unstaking_period = new_unstaking_period.clone();
        attrs.push( attr("new_unstaking_period", new_unstaking_period.to_string()) );
    }
    
    //Save new Config
    CONFIG.save(deps.storage, &config)?;

    Ok( Response::new().add_attributes( attrs ) )
    
}

//From a receive cw20 hook. Comes from the contract address so easy to validate sent funds. 
//Check if sent funds are equal to amount in msg so we don't have to recheck in the function
pub fn receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {

    let passed_asset: Asset = Asset {
        info: AssetInfo::Token {
            address: info.sender.clone(),
        },
        amount: cw20_msg.amount,
    };
    
    match from_binary(&cw20_msg.msg){
        Ok( Cw20HookMsg::Distribute {
                credit_asset,
                distribute_for,
            }) => {
            let distribution_assets = vec![ passed_asset ];
            let distribution_asset_ratios = vec![ Decimal::percent(100) ];
            distribute_funds(deps, info, Some(cw20_msg.sender), env, distribution_assets, distribution_asset_ratios, credit_asset, distribute_for )
        },
        Err(_) => Err(ContractError::Cw20MsgError {}),
    }

}

pub fn deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_owner: Option<String>,
    assets: Vec<Asset>,
) -> Result<Response, ContractError>{

    let valid_owner_addr = validate_position_owner(deps.api, info.clone(), position_owner)?;
        
    //Adding to Asset_Pool totals and deposit's list
    for asset in assets.clone(){
        let asset_pools = ASSETS.load(deps.storage)?;

        let deposit = Deposit {
            user: valid_owner_addr.clone(),
            amount: Decimal::from_ratio(asset.amount, Uint128::new(1u128)),
            deposit_time: env.block.time.seconds(),
            unstake_time: None,
        };

        
        match asset_pools.clone().into_iter().find(|mut x| x.credit_asset.info.equal(&asset.info)){
            Some(mut pool ) => {

                //Add user deposit to Pool totals
                pool.credit_asset.amount += asset.amount;
                //Add user deposit to deposits list
                pool.deposits.push(deposit);

                let mut temp_pools: Vec<AssetPool> = asset_pools.clone()
                .into_iter()
                .filter(|pool| !pool.credit_asset.info.equal(&asset.info))
                .collect::<Vec<AssetPool>>();

                temp_pools.push(pool);
                ASSETS.save(deps.storage, &temp_pools)?;
                
            },
            None => {
                //This doesn't get hit bc the asset object is validated beforehand. Instead, an invalid asset just won't get parsed thru.
                return Err(ContractError::InvalidAsset {  })
            }
        }
    }
    
    

    //Response build
    let response = Response::new();
    let mut attrs = vec![];

    attrs.push(("method", "deposit"));

    let v = &valid_owner_addr.to_string();
    attrs.push(("position_owner", v));

    let assets_as_string: Vec<String> = assets.iter().map(|x| x.to_string()).collect();
    for i in 0..assets.clone().len(){
        attrs.push(("deposited_assets", &assets_as_string[i]));    
    }

    Ok( response.add_attributes(attrs) )

}

//Get incentive rate and return accrued amount
fn accrue_incentives(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    config: Config,
    asset_pool: AssetPool,
    stake: Uint128,
    time_elapsed: u64,
) -> StdResult<Uint128> {

    let asset_current_supply = querier.query::<TokenInfoResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().osmosis_proxy.to_string(),
        msg: to_binary(&OsmoQueryMsg::GetTokenInfo { 
                denom: asset_pool.clone().credit_asset.info.to_string(),
            })?,
    }))?.current_supply;

    //Set Rate
    //The 2 slope model is based on total credit supply AFTER liquidations.
    //So the users who are distributed liq_funds will get rates based off the AssetPool's total AFTER their funds were used.
    let mut rate = config.clone().incentive_rate;
    if !config.clone().desired_ratio_of_total_credit_supply.is_zero(){

        let asset_util_ratio = decimal_division(Decimal::from_ratio(asset_pool.credit_asset.amount, Uint128::new(1u128) ), Decimal::from_ratio( asset_current_supply, Uint128::new(1u128) ) );
        let mut proportion_of_desired_util = decimal_division( asset_util_ratio, config.clone().desired_ratio_of_total_credit_supply );
        
        if proportion_of_desired_util.is_zero(){
            proportion_of_desired_util = Decimal::one();
        }

        let rate_multiplier = decimal_division( Decimal::one(), proportion_of_desired_util );

        rate = decimal_multiplication( config.clone().incentive_rate, rate_multiplier );
    }

    let mut incentives = accumulate_interest( stake, rate, time_elapsed )?;

    let mut total_incentives = INCENTIVES.load( storage )?;
    //Assert that incentives aren't over max, set 0 if so.
    if total_incentives + incentives > config.max_incentives {
        incentives = Uint128::zero();
    } else {
        total_incentives += incentives;
        INCENTIVES.save( storage, &total_incentives )?;
    }

    Ok( incentives )

}


fn accumulate_interest(
    stake: Uint128,
    rate: Decimal,
    time_elapsed: u64,
) -> StdResult<Uint128>{

    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    let accrued_interest = stake * applied_rate;

    Ok( accrued_interest )
}

//Withdraw / Unstake
pub fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    assets: Vec<Asset>,
) ->Result<Response, ContractError>{

    let config = CONFIG.load( deps.storage )?;

    let mut message: CosmosMsg;
    let mut msgs = vec![];       
    let mut attrs = vec![
        attr("method", "withdraw"),
        attr("position_owner", info.clone().sender.to_string()),
    ];

    //Each Asset
    for asset in assets.clone(){
        //We have to reload after every asset so we are using up to date data
        //Otherwise multiple withdrawal msgs will pass, being validated by unedited state data
        let asset_pools = ASSETS.load(deps.storage)?;

        //If the Asset has a pool, act
        match asset_pools.clone().into_iter().find(|mut asset_pool| asset_pool.credit_asset.info.equal(&asset.info)){
            
            //Some Asset
            Some( pool ) => {                
                
                //This forces withdrawals to be done by the info.sender 
                //so no need to check if the withdrawal is done by the position owner
                let user_deposits: Vec<Deposit> = pool.clone().deposits
                    .into_iter()
                    .filter(|deposit| deposit.user == info.sender)
                    .collect::<Vec<Deposit>>();

                let total_user_deposits: Decimal = user_deposits
                    .iter()
                    .map(|user_deposit| user_deposit.amount)
                    .collect::<Vec<Decimal>>()
                    .into_iter()
                    .sum();

                
                //Cant withdraw more than the total deposit amount
                if total_user_deposits < Decimal::from_ratio(asset.amount , Uint128::new(1u128)){
                    return Err(ContractError::InvalidWithdrawal {  })
                } else{

                    //Go thru each deposit and withdraw request from state
                    let ( withdrawable, new_pool) = withdrawal_from_state(
                        deps.storage,
                        deps.querier,
                        env.clone(),
                        config.clone(),
                        info.clone().sender, 
                        Decimal::from_ratio(asset.amount, Uint128::new(1u128)), 
                        pool,
                        false,
                    )?;
                   
                    
                    let mut temp_pools: Vec<AssetPool> = asset_pools.clone()
                        .into_iter()
                        .filter(|pool| !pool.credit_asset.info.equal(&asset.info))
                        .collect::<Vec<AssetPool>>();
                    temp_pools.push(new_pool.clone());

                    //Update pool
                    ASSETS.save(deps.storage, &temp_pools)?;

                    //If there is a withdrwable amount
                    if !withdrawable.is_zero() {
                        let withdrawable_asset = Asset {
                            amount: withdrawable,
                            ..asset
                        };

                        attrs.push( attr("withdrawn_asset", withdrawable_asset.to_string() ) );

                        //This is here in case there are multiple withdrawal messages created.
                        message = withdrawal_msg (
                            withdrawable_asset, 
                            info.sender.clone()
                        )?;
                        msgs.push(message);

                    }

                }

                                                             
            },
            None => return Err(ContractError::InvalidAsset {  })
        }
        
        
    }
        
    
    Ok( Response::new()
        .add_attributes(attrs)
        .add_messages(msgs)
    )
}

fn withdrawal_from_state(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    user: Addr,
    mut withdrawal_amount: Decimal,
    mut pool: AssetPool,
    skip_unstaking: bool,
) -> Result<(Uint128,AssetPool), ContractError>{

    let mut mbrn_incentives = Uint128::zero();

    let mut error: Option<StdError> = None;
    let mut is_user = false;
    let mut withdrawable = false;
    let mut withdrawable_amount = Uint128::zero();

    let new_deposits: Vec<Deposit> = pool.clone().deposits
        .into_iter()
        .map( |mut deposit_item| {
            
            //Only edit user deposits
            if deposit_item.user == user {
                is_user = true;
                
                /////Check if deposit is withdrawable
                if !skip_unstaking {
                    //If deposit has been "unstaked" ie previously withdrawn, assert the unstaking period has passed before withdrawing
                    if deposit_item.unstake_time.is_some() {
                        //If time_elapsed is >= unstaking period
                        if env.block.time.seconds() - deposit_item.unstake_time.unwrap() >= ( config.unstaking_period * SECONDS_PER_DAY ) {
                            withdrawable = true;
                        } 
                        //If unstaking period hasn't passed do nothing
                        
                    } else {
                        //Set unstaking time and don't withdraw anything
                        deposit_item.unstake_time = Some( env.block.time.seconds() );
                    }
                }

                //Subtract from each deposit until there is none left to withdraw
                        
                //If not withdrawable we only edit withdraw amount to make sure the deposits...
                //..that would get parsed through in a valid withdrawal get their unstaking_time set/checked           
                if withdrawal_amount != Decimal::zero() && deposit_item.amount > withdrawal_amount {

                                        
                    if withdrawable {
                        //Add to withdrawable
                        withdrawable_amount += withdrawal_amount * Uint128::new(1u128);

                        //Subtract from deposit.amount
                        deposit_item.amount -= withdrawal_amount;

                        //Calc incentives
                        let time_elapsed = deposit_item.unstake_time.unwrap() - deposit_item.deposit_time;
                        if time_elapsed != 0u64{
                            let accrued_incentives = match accrue_incentives( storage, querier, config.clone(), pool.clone(), withdrawal_amount * Uint128::new(1u128), time_elapsed ){
                                Ok( incentives ) => incentives,
                                Err( err ) => { 
                                    error = Some( err );
                                    Uint128::zero()
                                },
                            };
                            mbrn_incentives += accrued_incentives;
                        }
                    } 
                    //////
                    withdrawal_amount = Decimal::zero();
                    //////


                } else if withdrawal_amount != Decimal::zero() && deposit_item.amount <= withdrawal_amount {

                    //If it's less than amount, 0 the deposit and substract it from the withdrawal amount
                    withdrawal_amount -= deposit_item.amount;
                    //////
                    
                    
                    if withdrawable {
                        //Add to withdrawable_amount
                        withdrawable_amount += deposit_item.amount * Uint128::new(1u128);

                        //Calc incentives
                        let time_elapsed = deposit_item.unstake_time.unwrap() - deposit_item.deposit_time;
                        if time_elapsed != 0u64{
                            let accrued_incentives = match accrue_incentives( storage, querier, config.clone(), pool.clone(), deposit_item.amount * Uint128::new(1u128), time_elapsed ){
                                Ok( incentives ) => incentives,
                                Err( err ) => { 
                                    error = Some( err );
                                    Uint128::zero()
                                },
                            };
                            mbrn_incentives += accrued_incentives;
                        }                 

                        deposit_item.amount = Decimal::zero();
                    }
                    
                }

                withdrawable = false;
                
            }
            deposit_item

            })
        .collect::<Vec<Deposit>>()
        .into_iter()
        .filter( |deposit| deposit.amount != Decimal::zero())
        .collect::<Vec<Deposit>>();

    //Set new deposits
    pool.deposits = new_deposits;

    //Subtract withdrawable from total pool amount
    pool.credit_asset.amount -= withdrawable_amount;

    if error.is_some(){
        return Err( ContractError::CustomError { val: error.unwrap().to_string() } )
    }

    //If there are incentives
    if !mbrn_incentives.is_zero(){

        //Add incentives to User Claims
        USERS.update( storage, user, |user_claims| -> Result<User, ContractError> {
            match user_claims {
                Some( mut user ) => {
                    user.claimable_assets.push( 
                        Asset {
                            info: AssetInfo::NativeToken{ denom: config.clone().mbrn_denom },
                            amount: mbrn_incentives,
                    });
                    Ok( user )
                },
                None => {
                    if is_user {
                        Ok(
                            User {
                                claimable_assets: vec![ Asset {
                                    info: AssetInfo::NativeToken{ denom: config.clone().mbrn_denom },
                                    amount: mbrn_incentives,
                            } ]
                            }
                        )
                    } else {
                        return Err( ContractError::CustomError { val: String::from("Invalid user") } )
                    }
                }
            }
        })?;

    }
   

    Ok( ( withdrawable_amount, pool ) )
}

 /*
    - send repayments for the Positions contract
    - Positions contract sends back a distribute msg
    */
pub fn liquidate(
    deps: DepsMut,
    info: MessageInfo,
    credit_asset: LiqAsset,
) -> Result<Response, ContractError>{

    let config = CONFIG.load(deps.storage)?;

    if info.sender.clone() != config.positions_contract {
        return Err(ContractError::Unauthorized {})
    }
    
    let asset_pools = ASSETS.load(deps.storage)?;
    let mut asset_pool = match asset_pools.clone()
        .into_iter()
        .find(|x| x.credit_asset.info.equal(&credit_asset.info)){
            Some ( pool ) => { pool },
            None => {return Err(ContractError::InvalidAsset {  }) },
        };

    //Validate the credit asset
    //ie: the SP only repays for valid credit assets
    //The SP will allow any collateral assets
    validate_liq_assets(deps.storage, vec![credit_asset.clone()], info)?;

    let liq_amount = credit_asset.amount;

    //Assert repay amount or pay as much as possible
    let mut repay_asset = Asset{
        info: credit_asset.clone().info,
        amount: Uint128::new(0u128),
    };
    let mut leftover = Decimal::zero();

    if liq_amount > Decimal::from_ratio(asset_pool.credit_asset.amount, Uint128::new(1u128)){
        //If greater then repay what's possible 
        repay_asset.amount = asset_pool.credit_asset.amount;
        leftover = liq_amount - Decimal::from_ratio(asset_pool.credit_asset.amount, Uint128::new(1u128));

    }else{ //Pay what's being asked
        repay_asset.amount = liq_amount * Uint128::new(1u128); // * 1
    }

    //Save Repaid amount to Propagate
    let mut prop = PROP.load( deps.storage )?;
    prop.repaid_amount += repay_asset.amount;
    PROP.save( deps.storage, &prop)?;

    
    //Repay for the user
    let repay_msg = CDP_ExecuteMsg::LiqRepay { };

    let coin: Coin = asset_to_coin( repay_asset.clone() )?;

    let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.positions_contract.to_string(),
            msg: to_binary(&repay_msg)?,
            funds: vec![coin], 
    });

    //Subtract repaid_amount from totals
    asset_pool.credit_asset.amount -= repay_asset.amount;
    let mut temp_pools: Vec<AssetPool> = asset_pools.clone()
        .into_iter()
        .filter(| pool | !(pool.credit_asset.info.equal(&credit_asset.info)))
        .collect::<Vec<AssetPool>>();
    temp_pools.push(asset_pool.clone());
    ASSETS.save(deps.storage, &temp_pools)?;

    
    //Build the response
    //1) Repay message
    //2) Add position user info to response
    //3) Add potential leftover funds

    let mut res: Response = Response::new();
    
    Ok( res.add_message(message)
            .add_attributes(vec![
                attr( "method", "liquidate" ),
                attr("leftover_repayment", format!("{} {}", leftover, credit_asset.info))
            ]) )

   
}

//Calculate which and how much each user gets distributed from the liquidation
pub fn distribute_funds(
    deps: DepsMut,
    info: MessageInfo,
    cw20_sender: Option<String>,
    env: Env,
    mut distribution_assets: Vec<Asset>,
    distribution_asset_ratios: Vec<Decimal>,
    credit_asset: AssetInfo,
    distribute_for: Uint128, //How much repayment is this distributing for
) -> Result<Response, ContractError>{

    let config = CONFIG.load(deps.storage)?;

    //Can only be called by the positions contract
    if info.sender != config.positions_contract && cw20_sender.is_none(){
        return Err(ContractError::Unauthorized { })
    } else if cw20_sender.is_some() && cw20_sender.clone().unwrap() != config.positions_contract.to_string() {
        return Err(ContractError::Unauthorized { })
    }

    if distribution_assets.len() == 0 { return Err(ContractError::InsufficientFunds{ }) }

    let asset_pools = ASSETS.load(deps.storage)?;
    let mut asset_pool = match asset_pools.clone()
        .into_iter()
        .find(|pool| pool.credit_asset.info.equal(&credit_asset)){
            Some ( pool ) => { pool },
            None => {return Err(ContractError::InvalidAsset {  }) },
    };

    //Assert that the distributed assets were sent
    let mut assets: Vec<AssetInfo> = distribution_assets.clone()
        .into_iter()
        .map(|asset| asset.info )
        .collect::<Vec<AssetInfo>>();
    
    //This check is redundant for Cw20s and will fail bc validate_assets() only validates natives
    if cw20_sender.is_none() {
        let valid_assets = validate_assets(deps.storage, assets.clone(), info, false)?;
        
        if valid_assets.len() != distribution_assets.len() { return Err(ContractError::InvalidAssetObject{ }) }
        //Set distribution_assets to the valid_assets
        distribution_assets = valid_assets;
    }
    

    //Load repaid_amount
    //Liquidations are one msg at a time and PROP is always saved to first
    //so we can propagate without worry
    let mut prop = PROP.load( deps.storage )?;
    let mut repaid_amount: Uint128;
    //If this distribution is at most for the amount that was repaid
    if distribute_for <= prop.repaid_amount{
        repaid_amount = distribute_for;
        prop.repaid_amount -= distribute_for;
        PROP.save( deps.storage, &prop );
    }else{
        return Err( ContractError::CustomError { val: format!("Distribution attempting to distribute_for too much ( {} > {} )", distribute_for, prop.repaid_amount) } )
    }
   

    ///Calculate the user distributions
    let mut pool_parse = asset_pool.clone().deposits.into_iter();
    let mut distribution_list: Vec<Deposit> = vec![];
    let mut current_repay_total: Decimal = Decimal::percent(0);
    let repaid_amount_decimal = Decimal::from_ratio(repaid_amount, Uint128::new(1u128));

    
    while current_repay_total < repaid_amount_decimal{
        
        match pool_parse.next(){
            Some( mut deposit ) => {
                //panic!("{}", deposit.amount);
                                               
                //If greater, only add what's necessary and edit the deposit
                if  (current_repay_total + deposit.amount) > repaid_amount_decimal{
                    
                    //Subtract to calc what's left to repay
                    let remaining_repayment = repaid_amount_decimal - current_repay_total;

                    deposit.amount -= remaining_repayment;
                    current_repay_total += remaining_repayment;

                    //Add Deposit w/ amount = to remaining_repayment
                    //Splits original Deposit amount between both Vecs
                    distribution_list.push( 
                        Deposit {
                            amount: remaining_repayment,
                            ..deposit.clone()
                        } );

                    //Calc MBRN incentives
                    let time_elapsed = env.block.time.seconds() - deposit.deposit_time;
                    if time_elapsed != 0u64{
                        let accrued_incentives = accrue_incentives( deps.storage, deps.querier, config.clone(), asset_pool.clone(), remaining_repayment * Uint128::new(1u128), time_elapsed )?;
                    
                        //Add incentives to User Claims
                        USERS.update( deps.storage, deposit.user, |user_claims| -> Result<User, ContractError> {
                            match user_claims {
                                Some( mut user ) => {
                                    user.claimable_assets.push( 
                                        Asset {
                                            info: AssetInfo::NativeToken{ denom: config.clone().mbrn_denom },
                                            amount: accrued_incentives,
                                    });
                                    Ok( user )
                                },
                                None => return Err( ContractError::CustomError { val: String::from("Invalid user") } )
                            }
                        })?;
                    }

                }else{//Else, keep adding 
                    
                    current_repay_total += deposit.amount;

                    distribution_list.push( deposit.clone() );

                    //Calc MBRN incentives
                    let time_elapsed = env.block.time.seconds() - deposit.deposit_time;
                    if time_elapsed != 0u64{
                        let accrued_incentives = accrue_incentives( deps.storage, deps.querier, config.clone(), asset_pool.clone(), deposit.amount * Uint128::new(1u128), time_elapsed )?;
                        
                        //Add incentives to User Claims
                        USERS.update( deps.storage, deposit.user, |user_claims| -> Result<User, ContractError> {
                            match user_claims {
                                Some( mut user ) => {
                                    user.claimable_assets.push( 
                                        Asset {
                                            info: AssetInfo::NativeToken{ denom: config.clone().mbrn_denom },
                                            amount: accrued_incentives,
                                    });
                                    Ok( user )
                                },
                                None => return Err( ContractError::CustomError { val: String::from("Invalid user") } )
                            }
                        })?;
                    }
                }
                
                
            },
            None => {
               // panic!("None");
                //End of deposit list                
                //If it gets here and the repaid amount != current_repay_total, the state was mismanaged previously
                //since by now the funds have already been sent. 
                //For safety sake we'll set the values equal, as their job was to act as a limiter for the distribution list.
                current_repay_total = repaid_amount_decimal;                
            },
        }
       
    }

          
    //This doesn't filter partial uses
    let mut edited_deposits: Vec<Deposit> = asset_pool.clone().deposits
        .into_iter()
        .filter(|deposit| !deposit.equal(&distribution_list))
        .collect::<Vec<Deposit>>();   
    //If there is an overlap between the lists. meaning there was a partial usage
    if distribution_list.len() + edited_deposits.len() > asset_pool.deposits.len(){
        
       edited_deposits[0].amount -= distribution_list[ distribution_list.len()-1 ].amount;        
    }
        
    asset_pool.deposits = edited_deposits;
    
    let mut new_pools: Vec<AssetPool> = ASSETS.load(deps.storage)?
        .into_iter()
        .filter(|pool| !pool.credit_asset.info.equal(&credit_asset))
        .collect::<Vec<AssetPool>>();
    new_pools.push(asset_pool);
    //panic!("{:?}", new_pools);

    //Save pools w/ edited deposits to state
    ASSETS.save(deps.storage, &new_pools)?;
    

    //create function to find user ratios and distribute collateral based on them
    //Distribute 1 collateral at a time (not prorata) for gas and UX optimizations (ie if a user wants to sell they won't have to sell on 4 different pairs)
    //Also bc native tokens come in batches, CW20s come separately 
    let ( ratios, user_deposits ) = get_distribution_ratios(distribution_list.clone())?;
    

    let mut distribution_ratios: Vec<UserRatio> = user_deposits
        .into_iter()
        .enumerate()
        .map( |(index, deposit)| {
            UserRatio {
                user: deposit.user,
                ratio: ratios[index],
            }
        })
        .collect::<Vec<UserRatio>>();

        
    //1) Calc cAsset's ratios of total value
    //2) Split to users
    
    let mut cAsset_ratios = distribution_asset_ratios;
    //let messages: Vec<CosmosMsg> = vec![];

        
    for mut user_ratio in distribution_ratios{
                     
        for ( index, mut cAsset_ratio ) in cAsset_ratios.clone().into_iter().enumerate(){

            if cAsset_ratio == Decimal::zero(){ continue }

            if user_ratio.ratio == cAsset_ratio{
                
                //Add all of this asset to existing claims
                USERS.update(deps.storage, user_ratio.user, |user: Option<User>| -> Result<User, ContractError> {

                    match user{
                        Some ( mut some_user ) => {
                            //Find Asset in user state
                            match some_user.clone().claimable_assets.into_iter()
                                .find( | asset | asset.info.equal(&distribution_assets[index].info) ){

                                    Some( mut asset) => {
                                        //Add claim amount to the asset object
                                        asset.amount += distribution_assets[index].amount;
                                        
                                        //Create a replacement object for "user" since we can't edit in place
                                        let mut temp_assets: Vec<Asset> = some_user.clone().claimable_assets
                                            .into_iter()
                                            .filter(|claim| !claim.info.equal(&asset.info))
                                            .collect::<Vec<Asset>>();
                                        temp_assets.push( asset );

                                        some_user.claimable_assets = temp_assets;
                                    },
                                    None => {
                                        some_user.claimable_assets.push( Asset{
                                            info: distribution_assets[index].clone().info,
                                            amount: distribution_assets[index].clone().amount
                                        });
                                    }
                            }

                            Ok( some_user )
                        },
                        None => {
                            //Create object for user
                            Ok( User{
                                claimable_assets: vec![ Asset{
                                    info: distribution_assets[index].clone().info,
                                    amount: distribution_assets[index].clone().amount
                                }],
                                }
                            )
                        },
                    }
                })?;


                //Set cAsset_ratio to 0
                cAsset_ratios[index] = Decimal::zero();

                break;
            }else if user_ratio.ratio < cAsset_ratio {
                //Add full user ratio of the asset
                let send_ratio = decimal_division(user_ratio.ratio, cAsset_ratio);
                let send_amount = decimal_multiplication(send_ratio, Decimal::from_ratio(distribution_assets[index].amount, Uint128::new(1u128))) * Uint128::new(1u128);
                
                //Add to existing user claims
                USERS.update(deps.storage, user_ratio.clone().user, |user| -> Result<User, ContractError> {

                    match user{
                        Some ( mut user ) => {
                            //Find Asset in user state
                            match user.clone().claimable_assets.into_iter()
                                .find( | asset | asset.info.equal(&distribution_assets[index].info) ){

                                    Some( mut asset) => {
                                        //Add amounts
                                        asset.amount += send_amount;

                                        //Create a replacement object for "user" since we can't edit in place
                                        let mut temp_assets: Vec<Asset> = user.clone().claimable_assets
                                            .into_iter()
                                            .filter(|claim| !claim.info.equal(&asset.info))
                                            .collect::<Vec<Asset>>();
                                        temp_assets.push( asset );

                                        user.claimable_assets = temp_assets;
                                    },
                                    None => {
                                        user.claimable_assets.push( Asset{
                                            amount: send_amount,
                                            info: distribution_assets[index].clone().info,
                                        });
                                    }
                            }

                            Ok( user )
                        },
                        None => {
                            //Create object for user
                            Ok( User{
                                claimable_assets: vec![ Asset{
                                    amount: send_amount,
                                    info: distribution_assets[index].clone().info,
                                }],
                            })
                        },
                    }
                })?;

                //Set cAsset_ratio to the difference
                cAsset_ratio = decimal_subtraction(cAsset_ratio, user_ratio.ratio);

                break;
            }else if user_ratio.ratio > cAsset_ratio{
                //Add all of this asset
                 //Add to existing user claims
                 USERS.update(deps.storage, user_ratio.clone().user, |user| -> Result<User, ContractError> {

                    match user{
                        Some (mut user ) => {
                            //Find Asset in user state
                            match user.clone().claimable_assets.into_iter()
                                .find( | asset | asset.info.equal(&distribution_assets[index].info) ){

                                    Some( mut asset) => {
                                        asset.amount += distribution_assets[index].amount;

                                        //Create a replacement object for "user" since we can't edit in place
                                        let mut temp_assets: Vec<Asset> = user.clone().claimable_assets
                                            .into_iter()
                                            .filter(|claim| !claim.info.equal(&asset.info))
                                            .collect::<Vec<Asset>>();
                                        temp_assets.push( asset );

                                        user.claimable_assets = temp_assets;
                                    },
                                    None => {
                                        user.claimable_assets.push( Asset{
                                            info: distribution_assets[index].clone().info,
                                            amount: distribution_assets[index].clone().amount
                                        });
                                    }
                            }

                            Ok( user )
                        },
                        None => {
                            //Create object for user
                            Ok( User{
                                claimable_assets: vec![ Asset{
                                    info: distribution_assets[index].clone().info,
                                    amount: distribution_assets[index].clone().amount
                                }],
                            })
                        },
                    }
                })?;

                //Set user_ratio as leftover
                user_ratio.ratio = decimal_subtraction( user_ratio.ratio, cAsset_ratios[0] );
                
                //Set cAsset_ratio to 0
                cAsset_ratios[index] = Decimal::zero();
                //continue loop
            }
        }
    }  

    //Response Builder
    let res = Response::new()
    .add_attribute("method", "distribute")
    .add_attribute("credit_asset", credit_asset.to_string());

    let mut attrs = vec![];
    let assets_as_string: Vec<String> = distribution_assets.iter().map(|x| x.to_string()).collect();
    for i in 0..assets.clone().len(){
        attrs.push(("distribution_assets", &assets_as_string[i]));    
    }

    Ok( res.add_attributes(attrs) )

}

fn repay(
    deps: DepsMut, 
    env: Env,
    info: MessageInfo,
    user_info: UserInfo,
    repayment: Asset,
) -> Result<Response, ContractError>{

    let config = CONFIG.load( deps.storage )?;
    
    let mut msgs = vec![];       
    let mut attrs = vec![
        attr("method", "repay"),
        attr("user_info", user_info.clone().to_string()),
    ];

    let asset_pools = ASSETS.load(deps.storage)?;

    if let Some( pool ) = asset_pools.clone().into_iter().find(|mut asset_pool| asset_pool.credit_asset.info.equal(&repayment.info)){

        let position_owner = deps.api.addr_validate(&user_info.clone().position_owner)?;

        //This forces withdrawals to be done by the position_owner
        //so no need to check if the withdrawal is done by the position owner
        let user_deposits: Vec<Deposit> = pool.clone().deposits
            .into_iter()
            .filter(|deposit| deposit.user == info.sender)
            .collect::<Vec<Deposit>>();

        let total_user_deposits: Decimal = user_deposits
            .iter()
            .map(|user_deposit| user_deposit.amount)
            .collect::<Vec<Decimal>>()
            .into_iter()
            .sum();

        
        //Cant withdraw more than the total deposit amount
        if total_user_deposits < Decimal::from_ratio(repayment.amount , Uint128::new(1u128)){
            return Err(ContractError::InvalidWithdrawal {  })
        } else {

            //Go thru each deposit and withdraw request from state
            let ( _withdrawable, new_pool) = withdrawal_from_state(
                deps.storage,
                deps.querier,
                env.clone(),
                config.clone(),
                position_owner, 
                Decimal::from_ratio(repayment.amount, Uint128::new(1u128)), 
                pool,
                true,
            )?;
            
            
            let mut temp_pools: Vec<AssetPool> = asset_pools.clone()
                .into_iter()
                .filter(|pool| !pool.credit_asset.info.equal(&repayment.info))
                .collect::<Vec<AssetPool>>();
            temp_pools.push(new_pool.clone());

            //Update pool
            ASSETS.save(deps.storage, &temp_pools)?;

            /////This is where the function is different from withdraw()

            //Add Positions RepayMsg
            let repay_msg = CDP_ExecuteMsg::Repay {
                basket_id: user_info.clone().basket_id,
                position_id: user_info.clone().position_id,
                position_owner: Some( user_info.clone().position_owner ),
            };

            let coin: Coin = asset_to_coin( repayment.clone() )?;

            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.positions_contract.to_string(),
                msg: to_binary(&repay_msg)?,
                funds: vec![coin], 
            });

            msgs.push( msg );
        }

    } else {
        return Err(ContractError::InvalidAsset {  })
    }

    Ok( Response::new()
        .add_attributes(attrs)
        .add_messages(msgs)
    )

}

//Sends available claims to info.sender
//If claim_as is passed, the claims will be sent as said asset
pub fn claim(
    deps: DepsMut,
    info: MessageInfo,
    claim_as_native: Option<String>,
    claim_as_cw20: Option<String>,
    deposit_to: Option<PositionUserInfo>,
) -> Result<Response, ContractError>{
    let config: Config = CONFIG.load( deps.storage )?;

    let mut messages: Vec<CosmosMsg>;

    if claim_as_native.is_some() && claim_as_cw20.is_some(){
        return Err( ContractError::CustomError { val: "Can't claim as multiple assets, if not all claimable assets".to_string() } )
    }

    messages = user_claims_msgs( deps.storage, deps.api, config.clone(), info.clone(), deposit_to.clone(), config.clone().dex_router, claim_as_native.clone(), claim_as_cw20.clone() )?;
    
    let deposit_attribute = if let Some( position ) = deposit_to {
        format!("{:?}", position)
    } else {
        String::from("None")
    };

    let res = Response::new()
        .add_attribute("method", "claim")
        .add_attribute("user", info.sender)
        .add_attribute("claim_as_native", claim_as_native.unwrap_or_default())
        .add_attribute("claim_as_cw20", claim_as_cw20.unwrap_or_default())
        .add_attribute("deposit_to", deposit_attribute);
    
   
    Ok( res
        .add_messages(messages)
    )
}

fn user_claims_msgs(
    storage: &mut dyn Storage,
    api: &dyn Api,
    config: Config,
    info: MessageInfo,
    deposit_to: Option<PositionUserInfo>,
    dex_router: Option<Addr>,
    claim_as_native: Option<String>,
    claim_as_cw20: Option<String>,
) -> Result<Vec<CosmosMsg>, ContractError>{

    let mut user = match USERS.load(storage, info.clone().sender){
        Ok( user ) => user ,
        Err(_) => { return Err( ContractError::CustomError { val: "Info.sender is not a user".to_string() } ) } 
    };
    
    let mut messages: Vec<CosmosMsg> = vec![];

    //If we are claiming the available assets without swaps
    if claim_as_cw20.is_none() && claim_as_native.is_none(){
        //If we are depositing to a Position then we don't use a regular withdrawal msg
        if deposit_to.is_some(){
            let msgs = deposit_to_position(storage, user.claimable_assets, deposit_to.unwrap(), info.clone().sender)?;
            messages.extend(msgs);
        }else{
             
            //List of coins to send
            let mut native_claims = vec![];

            //Aggregate native token sends
            for asset in user.clone().claimable_assets{
                match asset.clone().info {
                    AssetInfo::Token { address: _ } => {
                        messages.push( withdrawal_msg(asset, info.clone().sender)? );
                    },
                    AssetInfo::NativeToken { denom: _ } => {
                        native_claims.push( asset_to_coin( asset )? );
                    },
                }  
            }

            if native_claims != vec![] {
                let msg = CosmosMsg::Bank(BankMsg::Send {
                    to_address: info.clone().sender.to_string(),
                    amount: native_claims,
                });
                messages.push( msg );
            }
        }
    }else if dex_router.is_some(){//Router usage

        for asset in user.claimable_assets.clone(){
            match asset.clone().info{
                AssetInfo::Token { address } => {

                    //Swap to Cw20 before sending or depositing
                    if claim_as_cw20.is_some(){
            
                        let valid_claim_addr = api.addr_validate( &claim_as_cw20.clone().unwrap() )?;
                
                        if deposit_to.is_some(){ //Swap and deposit to a Position

                            let user_info = deposit_to.clone().unwrap();
            
                            //Create deposit msg as a Hook since the Router is sending to a Cw20 contract
                            let deposit_msg = CDP_Cw20HookMsg::Deposit { 
                                    basket_id: user_info.basket_id, 
                                    position_owner: user_info.position_owner, 
                                    position_id: user_info.position_id, 
                            };
            
                            //Create Cw20 Router SwapMsgs to the position contract (owner) w/ DepositMsgs as the hook        
                            let swap_hook = RouterCw20HookMsg::Swap { 
                                to: AssetInfo::Token { address: valid_claim_addr }, 
                                max_spread: Some( config.clone().max_spread.unwrap_or_else(|| Decimal::percent(10) ) ), 
                                recipient: Some( config.clone().owner.to_string() ), 
                                hook_msg: Some( to_binary( &deposit_msg )? ), 
                                split: None,
                            };
            
                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: address.to_string(),
                                msg: to_binary(
                                        &Cw20ExecuteMsg::Send { 
                                            contract: config.clone().dex_router.unwrap().to_string(), 
                                            amount: asset.amount, 
                                            msg: to_binary( &swap_hook )?, 
                                        }
                                        )?,
                                funds: vec![],
                            });
                            messages.push( message );
            
                        }else{ //Send straight to User
            
                            //Create Cw20 Router SwapMsgs        
                            let swap_hook = RouterCw20HookMsg::Swap { 
                                    to: AssetInfo::Token { address: valid_claim_addr }, 
                                    max_spread: Some( config.clone().max_spread.unwrap_or_else(|| Decimal::percent(10) ) ), 
                                    recipient: Some( info.clone().sender.to_string() ), 
                                    hook_msg: None, 
                                    split: None,
                                };
            
                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: address.to_string(),
                                msg: to_binary(
                                        &Cw20ExecuteMsg::Send { 
                                            contract: config.clone().dex_router.unwrap().to_string(), 
                                            amount: asset.amount, 
                                            msg: to_binary( &swap_hook )?, 
                                        }
                                        )?,
                                funds: vec![],
                            });
            
                            messages.push( message );
                        }
                    }//Swap to native before sending or depositing
                    else if claim_as_native.is_some(){
            
                        if deposit_to.is_some(){ //Swap and deposit to a Position

                            let user_info = deposit_to.clone().unwrap();
            
                            //Create deposit msg 
                            let deposit_msg = CDP_ExecuteMsg::Deposit { 
                                    position_owner: user_info.position_owner, 
                                    basket_id: user_info.basket_id, 
                                    position_id: user_info.position_id,
                            };
            
                            //Create Cw20 Router SwapMsgs to the position contract (owner) w/ DepositMsgs as the hook        
                            let swap_hook = RouterCw20HookMsg::Swap { 
                                to: AssetInfo::NativeToken { denom: claim_as_native.clone().unwrap() }, 
                                max_spread: Some( config.clone().max_spread.unwrap_or_else(|| Decimal::percent(10) ) ), 
                                recipient: Some( config.clone().owner.to_string() ), 
                                hook_msg: Some( to_binary( &deposit_msg )? ), 
                                split: None,
                            };
            
                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: address.to_string(),
                                msg: to_binary(
                                        &Cw20ExecuteMsg::Send { 
                                            contract: config.clone().dex_router.unwrap().to_string(), 
                                            amount: asset.amount, 
                                            msg: to_binary( &swap_hook )?, 
                                        }
                                        )?,
                                funds: vec![],
                            });
                            messages.push( message );
            
                        }else{ //Send straight to User
            
                            //Create Cw20 Router SwapMsgs        
                            let swap_hook = RouterCw20HookMsg::Swap { 
                                    to: AssetInfo::NativeToken { denom: claim_as_native.clone().unwrap() }, 
                                    max_spread: Some( config.clone().max_spread.unwrap_or_else(|| Decimal::percent(10) ) ), 
                                    recipient: Some( info.clone().sender.to_string() ), 
                                    hook_msg: None, 
                                    split: None,
                                };
            
                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: address.to_string(),
                                msg: to_binary(
                                        &Cw20ExecuteMsg::Send { 
                                            contract: config.clone().dex_router.unwrap().to_string(), 
                                            amount: asset.amount, 
                                            msg: to_binary( &swap_hook )?, 
                                        }
                                        )?,
                                funds: vec![],
                            });
            
                            messages.push( message );
                        }

                    }   
                },
                /////Starting token is native so msgs go straight to the router contract
                AssetInfo::NativeToken { denom } => {

                    //If the asset is MBRN, mint the incentives and skip everything else
                    if denom == config.clone().mbrn_denom {

                        let message = CosmosMsg::Wasm(WasmMsg::Execute {
                            contract_addr: config.clone().osmosis_proxy.to_string(),
                            msg: to_binary(
                                    &OsmoExecuteMsg::MintTokens { 
                                        denom, 
                                        amount: asset.amount, 
                                        mint_to_address: info.clone().sender.to_string() })?,
                            funds: vec![],
                        });
                        messages.push( message );
                        

                    } else {

                        //Swap to Cw20 before sending or depositing
                        if claim_as_cw20.is_some(){
                
                            let valid_claim_addr = api.addr_validate( claim_as_cw20.clone().unwrap().as_ref() )?;
                    
                            if deposit_to.is_some(){ //Swap and deposit to a Position

                                let user_info = deposit_to.clone().unwrap();
                
                                //Create deposit msg as a Hook since the Router is sending to a Cw20 contract
                                let deposit_msg = CDP_Cw20HookMsg::Deposit { 
                                        basket_id: user_info.basket_id, 
                                        position_owner: user_info.position_owner, 
                                        position_id: user_info.position_id, 
                                };
                
                                //Create Cw20 Router SwapMsgs to the position contract (owner) w/ DepositMsgs as the hook        
                                let swap_hook = RouterExecuteMsg::SwapFromNative { 
                                    to: AssetInfo::Token { address: valid_claim_addr }, 
                                    max_spread: Some( config.clone().max_spread.unwrap_or_else(|| Decimal::percent(10) ) ), 
                                    recipient: Some( config.clone().owner.to_string() ), 
                                    hook_msg: Some( to_binary( &deposit_msg )? ), 
                                    split: None,
                                };
                
                                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                    contract_addr: config.clone().dex_router.unwrap().to_string(),
                                    msg: to_binary(&swap_hook )?,
                                    funds: vec![ asset_to_coin( asset )? ],
                                });
                                messages.push( message );
                
                            }else{ //Send straight to User
                
                                //Create Cw20 Router SwapMsgs        
                                let swap_hook = RouterExecuteMsg::SwapFromNative { 
                                        to: AssetInfo::Token { address: valid_claim_addr }, 
                                        max_spread: Some( config.clone().max_spread.unwrap_or_else(|| Decimal::percent(10) ) ), 
                                        recipient: Some( info.clone().sender.to_string() ), 
                                        hook_msg: None, 
                                        split: None,
                                    };
                
                                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                    contract_addr: config.clone().dex_router.unwrap().to_string(),
                                    msg: to_binary(&swap_hook )?,
                                    funds: vec![ asset_to_coin( asset )? ],
                                });
                
                                messages.push( message );
                            }
                        }//Swap to native before sending or depositing
                        else if claim_as_native.is_some(){
                
                            if deposit_to.is_some(){ //Swap and deposit to a Position

                                let user_info = deposit_to.clone().unwrap();
                
                                //Create deposit msg 
                                let deposit_msg = CDP_ExecuteMsg::Deposit {  
                                        position_owner: user_info.position_owner, 
                                        basket_id: user_info.basket_id, 
                                        position_id: user_info.position_id,
                                };
                
                                //Create Cw20 Router SwapMsgs to the position contract (owner) w/ DepositMsgs as the hook        
                                let swap_hook = RouterExecuteMsg::SwapFromNative { 
                                    to: AssetInfo::NativeToken { denom: claim_as_native.clone().unwrap() }, 
                                    max_spread: Some( config.clone().max_spread.unwrap_or_else(|| Decimal::percent(10) ) ), 
                                    recipient: Some( config.clone().owner.to_string() ), 
                                    hook_msg: Some( to_binary( &deposit_msg )? ), 
                                    split: None,
                                };
                
                                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                    contract_addr: config.clone().dex_router.unwrap().to_string(),
                                    msg: to_binary(&swap_hook )?,
                                    funds: vec![ asset_to_coin( asset )? ],
                                });
                                messages.push( message );
                
                            }else{ //Send straight to User
                
                                //Create Cw20 Router SwapMsgs        
                                let swap_hook = RouterExecuteMsg::SwapFromNative { 
                                        to: AssetInfo::NativeToken { denom: claim_as_native.clone().unwrap() }, 
                                        max_spread: Some( config.clone().max_spread.unwrap_or_else(|| Decimal::percent(10) ) ), 
                                        recipient: Some( info.clone().sender.to_string() ), 
                                        hook_msg: None, 
                                        split: None,
                                    };
                
                                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                    contract_addr: config.clone().dex_router.unwrap().to_string(),
                                    msg: to_binary(&swap_hook )?,
                                    funds: vec![ asset_to_coin( asset )? ],
                                });
                
                                messages.push( message );
                            }

                        }   

                    }
                    

                }
            }

        }

    }

    //Remove User's claims
    USERS.update(storage, info.clone().sender, |user| -> Result<User, ContractError> {
        match user{
            Some( mut user ) => {
                user.claimable_assets = vec![];
                Ok( user )
            },
            None => { return Err( ContractError::CustomError { val: "Info.sender is not a user".to_string() } )}
        }
    })?; 
    

    Ok( messages )
}

pub fn add_asset_pool(
    deps: DepsMut,
    info: MessageInfo,
    credit_asset: Asset,
    liq_premium: Decimal,
) -> Result<Response, ContractError>{

    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner{
        return Err(ContractError::Unauthorized{ })
    }

    let mut asset_pools = ASSETS.load(deps.storage)?;

    let new_pool = AssetPool {
        credit_asset: credit_asset.clone(),
        liq_premium,
        deposits: vec![],
    };

    asset_pools.push(new_pool);

    ASSETS.save(deps.storage, &asset_pools)?;

    let res = Response::new()
    .add_attribute("method", "add_asset_pool")
    .add_attribute("asset", credit_asset.to_string())
    .add_attribute("premium", liq_premium.to_string());

    Ok( res )
}

pub fn deposit_to_position(
    deps: &mut dyn Storage,
    assets: Vec<Asset>,
    deposit_to: PositionUserInfo,
    user: Addr,
) -> Result<Vec<CosmosMsg>, ContractError>{

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut coins: Vec<Coin> = vec![];
    let mut native_assets: Vec<Asset> = vec![];

    let config = CONFIG.load(deps)?;
    
    for asset in assets.clone().into_iter(){
        match asset.clone().info{
            AssetInfo::NativeToken { denom } => {
                native_assets.push( asset.clone() );
                coins.push( asset_to_coin( asset.clone() )? );
            },
            AssetInfo::Token { address } => {

                let deposit_msg = CDP_Cw20HookMsg::Deposit {
                    position_owner: Some(user.to_string()),
                    basket_id: deposit_to.clone().basket_id,
                    position_id: deposit_to.clone().position_id,
                };
            
            //CW20 Send                         
            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: address.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::Send {
                    amount: asset.amount,
                    contract: config.clone().owner.to_string(), //Assumption that this is the Positions contract
                    msg: to_binary(&deposit_msg)?,
                })?,
                funds: vec![],
            });
            messages.push(msg);
            }
        };
    }

    // let asset_info: Vec<AssetInfo> = assets
    //     .into_iter()
    //     .map(|asset| {
    //         asset.info
    //     }).collect::<Vec<AssetInfo>>();

    //Adds Native token deposit msg to messages
    let deposit_msg = CDP_ExecuteMsg::Deposit { 
        position_owner: Some(user.to_string()),
        basket_id: deposit_to.clone().basket_id,
        position_id: deposit_to.clone().position_id,
    };

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.positions_contract.to_string(), 
        msg: to_binary(&deposit_msg)?,
        funds: coins,
    });


    messages.push(msg);



    Ok( messages )
}

pub fn get_distribution_ratios(
    deposits: Vec<Deposit>
) -> StdResult<(Vec<Decimal>, Vec<Deposit>)>{
    
    let mut user_deposits: Vec<Deposit> = vec![];
    let mut total_amount: Decimal = Decimal::percent(0);
    let mut new_deposits: Vec<Deposit> = vec![];

    //For each Deposit, create a condensed Deposit for its user.
    //Add to an existing one if found.
    
    for deposit in deposits.clone().into_iter(){

        match user_deposits.clone().into_iter().find(|user_deposit| user_deposit.user == deposit.user){
            Some( mut user_deposit) => {

                user_deposit.amount += deposit.amount;

                //Recreating edited user deposits due to lifetime issues
                new_deposits = user_deposits.into_iter()
                .filter(|deposit| deposit.user != user_deposit.user)
                .collect::<Vec<Deposit>>();
                new_deposits.push(user_deposit);

                total_amount += deposit.amount;
            },
            None => {
                new_deposits.push( Deposit { ..deposit });

                total_amount += deposit.amount;
            },
        }

        user_deposits = new_deposits.clone();
    }


    //getting each user's % of total amount
    let mut user_ratios: Vec<Decimal> = vec![];
    for deposit in user_deposits.iter(){
        user_ratios.push(decimal_division(deposit.amount, total_amount));
    }

    Ok( ( user_ratios, user_deposits ) )
}

//Confirms that sent assets are at least valued greater than the credit repayment value
// pub fn confirm_asset_values(
//     deps: DepsMut,
//     info: MessageInfo,
//     liq_msg: LiqModuleMsg,
// ) -> Result<(), ContractError>{

//     let liq_value: Decimal;

//     for asset in liq_msg.clone().liquidated_cAssets{

//         match asset.asset.info {
//             AssetInfo::NativeToken { denom } => {
//                 assert_sent_native_token_balance(&asset.asset, &info)?;
                
//                 //TODO: Query price by denom using the ORACLES item
//                 let asset_price: Decimal;

//                 let asset_value: Decimal = decimal_multiplication(asset_price, asset.asset.amount);
//                 liq_value += asset_value;
//             },

//             AssetInfo::Token { address } => {

//                  //TODO: Query price by denom using the ORACLES item
//                  let asset_price: Decimal;

//                  let asset_value: Decimal = decimal_multiplication(asset_price, asset.asset.amount);
//                  liq_value += asset_value;
//             }
//         }
//     }

//     let credit_value = decimal_multiplication(liq_msg.credit_price, liq_msg.credit_asset.amount);

//     if liq_value > credit_value {
//         return Ok(())
//     }else{
//         return Err(ContractError::CustomError { val: "Liquidated assets aren't worth more than the requested repayment amount".to_string() })
//     }
// }


pub fn withdrawal_msg(
    asset: Asset,
    recipient: Addr,
)-> Result<CosmosMsg, ContractError>{
    //let credit_contract: Addr = basket.credit_contract;

    match asset.clone().info{
        AssetInfo::Token { address } => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: address.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: recipient.to_string(),
                    amount: asset.amount,
                })?,
                funds: vec![],
            });
            Ok(message)
        },
        AssetInfo::NativeToken { denom: _ } => {

            let coin: Coin = asset_to_coin(asset)?;
            let message = CosmosMsg::Bank(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![coin],
            });
            Ok(message)
        },
    }
    
}


pub fn asset_to_coin(
    asset: Asset
)-> Result<Coin, ContractError>{

    match asset.info{
        //
        AssetInfo::Token { address: _ } => 
            return Err(ContractError::InvalidParameters {  })
        ,
        AssetInfo::NativeToken { denom } => {
            Ok(
                Coin {
                    denom: denom,
                    amount: asset.amount,
                }
            )
        },
    }
    
}


pub fn validate_liq_assets(
    deps:  &dyn Storage,
    liq_assets: Vec<LiqAsset>,
    info: MessageInfo
) -> Result<(), ContractError>{

    //Validate sent assets against accepted assets
    let asset_pools = ASSETS.load(deps)?;

    for asset in liq_assets{

        //Check if the asset has a pool
        match asset_pools.iter().find(|x| x.credit_asset.info.equal(&asset.info)) {
            Some( _a) => { },
            None => { return Err(ContractError::InvalidAsset {  }) },
        }
    }

    Ok( () )
}

//Note: This fails if an asset total is sent in two separate Asset objects. Both will be invalidated.
pub fn validate_assets(
    deps:  &dyn Storage,
    assets: Vec<AssetInfo>,
    info: MessageInfo,
    in_pool: bool,
) -> Result< Vec<Asset>, ContractError>{

    let mut valid_assets: Vec<Asset> = vec![];

    if in_pool{

        //Validate sent assets against accepted assets
        let asset_pools = ASSETS.load(deps)?;

        for asset in assets{
            //If the asset has a pool, validate its balance
            match asset_pools.iter().find(|x| x.credit_asset.info.equal(&asset)) {
                Some( _a) => {
                    match asset {
                        AssetInfo::NativeToken { denom: _ } => {
                            
                            match assert_sent_native_token_balance(asset, &info){
                                Ok( valid_asset ) => {
                                    valid_assets.push( valid_asset );
                                },
                                Err(_) => {},
                                }
                           
                            
                        },
                        AssetInfo::Token { address: _ } => {
                             //Functions assume Cw20 asset amounts are taken from Messageinfo

                         }
                    }
                },
                None => { },
            };
        }
    }else{
        for asset in assets{
            match asset {
                AssetInfo::NativeToken { denom: _ } => {
                    
                    match assert_sent_native_token_balance(asset, &info){
                        Ok( valid_asset ) => {
                            valid_assets.push( valid_asset );
                        },
                        Err(_) => {},
                    }
                    

                },
                AssetInfo::Token { address: _ } => {
                    //Functions assume Cw20 asset amounts are taken from Messageinfo
                }
        
            }
        }
    }
    

    Ok( valid_assets )
}

//Refactored Terraswap function
pub fn assert_sent_native_token_balance(
    asset_info: AssetInfo,
    message_info: &MessageInfo)-> StdResult<Asset> {
        
    let mut asset: Asset;

    if let AssetInfo::NativeToken { denom} = &asset_info {
        match message_info.funds.iter().find(|x| x.denom == *denom) {
            Some(coin) => {
                if coin.amount > Uint128::zero(){
                    asset = Asset{ info: asset_info, amount: coin.amount};
                }else{
                    return Err(StdError::generic_err("You gave me nothing to deposit"))
                }                
            },
            None => {
                {
                    return Err(StdError::generic_err("Incorrect denomination, sent asset denom and asset.info.denom differ"))
                }
            }
        }
    } else {
        return Err(StdError::generic_err("Asset type not native, check Msg schema and use AssetInfo::Token{ address: Addr }"))
    }
    
    Ok( asset )
}

//Validate Recipient
pub fn validate_position_owner(
    deps: &dyn Api, 
    info: MessageInfo, 
    recipient: Option<String>) -> StdResult<Addr>{
    
    let valid_recipient: Addr = if let Some(recipient) = recipient {
        deps.addr_validate(&recipient)?
    }else {
        info.sender.clone()
    };
    Ok(valid_recipient)
}




#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {  } => to_binary( &CONFIG.load( deps.storage )? ),
        QueryMsg::CheckLiquidatible { asset } => to_binary(&query_liquidatible(deps, asset)?),
        QueryMsg::AssetDeposits { user, asset_info } => to_binary(&query_deposits(deps, user, asset_info)?),
        QueryMsg::UserClaims{ user } => to_binary(&query_user_claims( deps, user )?),
        QueryMsg::AssetPool { asset_info } => to_binary( &query_pool(deps, asset_info )?),
    }
}


pub fn query_pool(
    deps: Deps,
    asset_info: AssetInfo,
) -> StdResult<PoolResponse>{

   match ASSETS.load(deps.storage)?
        .into_iter()
        .find(|pool| pool.credit_asset.info.equal(&asset_info)){

            Some( pool ) => {
                return Ok(PoolResponse {
                        credit_asset: pool.clone().credit_asset,
                        liq_premium: pool.clone().liq_premium,
                        deposits: pool.clone().deposits,
                        })
            },
            None => { return Err(StdError::GenericErr { msg: "Asset Pool nonexistent".to_string() })}
        }
        
}

pub fn query_liquidatible ( 
    deps: Deps,
    asset: LiqAsset,
) -> StdResult<LiquidatibleResponse> {

    match ASSETS.load(deps.storage)?.iter()
        .find(|pool| pool.credit_asset.info.equal(&asset.info)){
            Some( pool ) => {

                let asset_amount_uint128 = asset.amount * Uint128::new(1u128);
                
                let liquidatible_amount = pool.credit_asset.amount;
            
                if liquidatible_amount > asset_amount_uint128 {
                    return Ok(LiquidatibleResponse{ leftover: Decimal::percent(0)} )
                }else{
                    let leftover = asset_amount_uint128 - pool.credit_asset.amount;
                    return Ok(LiquidatibleResponse{
                         leftover: Decimal::from_ratio(leftover, Uint128::new(1u128))
                        })
                }
            },
            None => { return Err(StdError::GenericErr { msg: "Asset doesnt exist as an AssetPool".to_string() })}
        }
}

pub fn query_deposits (
    deps: Deps,
    user: String,
    asset_info: AssetInfo,
) -> StdResult<DepositResponse>{

    let valid_user = deps.api.addr_validate(&user)?;

    match ASSETS.load(deps.storage)?.into_iter()
        .find(|pool| pool.credit_asset.info.equal(&asset_info)){

            Some( pool ) => {

                let deposits: Vec<Deposit> = pool.deposits
                        .into_iter()
                        .filter(|deposit| deposit.user == valid_user)
                        .collect::<Vec<Deposit>>();

                if deposits.len() == 0 { return Err(StdError::GenericErr { msg: "User has no open positions in this asset pool or the pool doesn't exist".to_string() }) }

                return Ok( DepositResponse {
                    asset: asset_info,
                    deposits,
                } )      
            },
            None => { return Err(StdError::GenericErr { msg: "User has no open positions in this asset pool or the pool doesn't exist".to_string() })}
        } 
      
}

pub fn query_user_claims (
    deps: Deps,
    user: String,
) -> StdResult<ClaimsResponse>{

    let valid_user = deps.api.addr_validate(&user)?;

    match USERS.load(deps.storage, valid_user){
            Ok( user ) => {
                return Ok( ClaimsResponse {
                   claims: user.claimable_assets,
                } )      
            },
            Err(_) => { return Err(StdError::GenericErr { msg: "User has no claimable assets".to_string() })}
        } 
      
}

