use std::env;
use std::error::Error;
use std::ops::Index;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, StdError, Storage, Addr, Api, Uint128, CosmosMsg, BankMsg, WasmMsg, Coin, Decimal, BankQuery, BalanceResponse, QueryRequest, WasmQuery, QuerierWrapper, attr};
use cw2::set_contract_version;
use membrane::positions::{ExecuteMsg as CDP_ExecuteMsg, Cw20HookMsg as CDP_Cw20HookMsg};
use membrane::stability_pool::{ExecuteMsg, InstantiateMsg, QueryMsg, LiquidatibleResponse, DepositResponse, ClaimsResponse, PoolResponse, PositionUserInfo};
use membrane::types::{ Asset, AssetInfo, LiqAsset, AssetPool, Deposit, cAsset, UserRatio, User };
use cw20::{Cw20ExecuteMsg, Cw20QueryMsg};

use crate::error::ContractError;
use crate::math::{decimal_division, decimal_subtraction, decimal_multiplication};
use crate::state::{ ASSETS, CONFIG, Config, USERS };

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:stability-pool";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//FIFO Stability Pool

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {


    let config: Config;
    if msg.owner.is_some(){
        config = Config {
            owner:deps.api.addr_validate(&msg.owner.unwrap())?, 
            oracle_contract: None,
            liq_fee: Decimal::percent(0),
        };
    }else{
        config = Config {
            owner: info.sender.clone(),  
            oracle_contract: None,
            liq_fee: Decimal::percent(0),
        };
    }

    

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    let mut res = Response::new();
    let mut attrs = vec![];

    attrs.push(("method", "instantiate"));

    let c = &config.owner.to_string();
    attrs.push(("owner", c));

    
    if msg.asset_pool.is_some() {

        let mut pool = msg.asset_pool.unwrap();

        pool.deposits = vec![];

        ASSETS.save(deps.storage, &vec![pool])?;
    }
    
    Ok( res.add_attributes(attrs) )
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {ExecuteMsg::Deposit{user,assets}=> deposit( deps, info, user, assets ),
    ExecuteMsg::Withdraw{ assets}=> withdraw( deps, info, assets ),
    ExecuteMsg::Liquidate { credit_asset } => liquidate( deps, info, credit_asset ),
    ExecuteMsg::ClaimAs { claim_as, deposit_to } => claim( deps, info, claim_as, deposit_to ),
    ExecuteMsg::AddPool { asset_pool } => add_asset_pool( deps, info, asset_pool.credit_asset, asset_pool.liq_premium ),
    ExecuteMsg::Distribute { distribution_assets, credit_asset, credit_price } => distribute_funds( deps, info, env, distribution_assets, credit_asset, credit_price ), 
}
}//Functions assume Cw20 asset amounts are taken from Messageinfo

pub fn deposit(
    deps: DepsMut,
    info: MessageInfo,
    position_owner: Option<String>,
    mut assets: Vec<Asset>,
) -> Result<Response, ContractError>{

    let valid_owner_addr = validate_position_owner(deps.api, info.clone(), position_owner)?;
    
    //Outputs asset objects w/ correct amounts
    assets = validate_assets(deps.storage, assets.clone(), info.clone(), true, true)?;

    //Adding to Asset_Pool totals and deposit's list
    for asset in assets.clone(){
        let asset_pools = ASSETS.load(deps.storage)?;

        let deposit = Deposit{
            user: valid_owner_addr.clone(),
            amount: Decimal::new(asset.amount * Uint128::new(1000000000000000000u128)),
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


pub fn withdraw(
    deps: DepsMut,
    info: MessageInfo,
    mut assets: Vec<Asset>,
) ->Result<Response, ContractError>{


    //Outputs asset objects w/ correct amounts
    //Because this outputs only the correctly/accurately submitted assets, InvalidAsset Error should never hit
    assets = validate_assets(deps.storage, assets.clone(), info.clone(), true, false)?;
    

    let mut message: CosmosMsg;
    let mut msgs = vec![];
       

    //Each Asset
    for asset in assets.clone(){
        //We have to reload after every asset so we are using up to date data
        //Otherwise multiple withdrawal msgs will pass, being validated by unedited state data
        let asset_pools = ASSETS.load(deps.storage)?;

        //If the Asset has a pool, act
        match asset_pools.clone().into_iter().find(|mut x| x.credit_asset.info.equal(&asset.info)){
            
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
                    let mut new_pool = withdrawal_from_state(
                        info.clone().sender, 
                        Decimal::from_ratio(asset.amount, Uint128::new(1u128)), 
                        pool)?;

                    
                    //Subtract from total pool
                    new_pool.credit_asset.amount -= asset.amount;
                    
                    let mut temp_pools: Vec<AssetPool> = asset_pools.clone()
                        .into_iter()
                        .filter(|pool| !pool.credit_asset.info.equal(&asset.info))
                        .collect::<Vec<AssetPool>>();
                    temp_pools.push(new_pool);

                    //Update pool
                    ASSETS.save(deps.storage, &temp_pools)?;

                    //This is here in case there are multiple withdrawal messages created.
                    message = withdrawal_msg(asset, info.sender.clone())?;
                    msgs.push(message);

                }

                                                             
            },
            None => return Err(ContractError::InvalidAsset {  })
        }
        
        
    }

         
    //Response builder
    let response = Response::new();
    let mut attrs = vec![];

    attrs.push(("method", "withdraw"));

    let i = &info.sender.to_string();
    attrs.push(("position_owner", i));

    
    let assets_as_string: Vec<String> = assets.iter().map(|x| x.to_string()).collect();
    
    for i in 0..assets.clone().len(){
        attrs.push(("withdrawn_assets", &assets_as_string[i]));    
    }
    
    Ok( response.add_attributes(attrs).add_messages(msgs) )
}

fn withdrawal_from_state(
    user: Addr,
    mut withdrawal_amount: Decimal,
    mut pool: AssetPool,
) -> Result<AssetPool, ContractError>{


    let new_deposits: Vec<Deposit> = pool.deposits
        .into_iter()
        .map( |mut deposit_item| {
            
            //Only edit user deposits
            if deposit_item.user == user{
                //subtract from each deposit until there is none left to withdraw
                if withdrawal_amount != Decimal::percent(0) && deposit_item.amount > withdrawal_amount{

                    deposit_item.amount -= withdrawal_amount;

                }else if withdrawal_amount != Decimal::percent(0) && deposit_item.amount <= withdrawal_amount{

                    //If it's less than amount, 0 the deposit and substract it from the withdrawal amount
                    withdrawal_amount -= deposit_item.amount;
                    deposit_item.amount = Decimal::percent(0);
                    
                }
            }
            deposit_item

            })
        .collect::<Vec<Deposit>>()
        .into_iter()
        .filter( |deposit| deposit.amount != Decimal::percent(0))
        .collect::<Vec<Deposit>>();

    pool.deposits = new_deposits;


    Ok( pool )
}

 /*
    - send repayments for an external contract
    - External contract sends back a distribute msg
    */
pub fn liquidate(
    deps: DepsMut,
    info: MessageInfo,
    credit_asset: LiqAsset,
) -> Result<Response, ContractError>{

    let config = CONFIG.load(deps.storage)?;

    if info.sender.clone() != config.owner {
        return Err(ContractError::Unauthorized {})
    }
    
    let asset_pools = ASSETS.load(deps.storage)?;
    let asset_pool = match asset_pools
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

    if liq_amount > Decimal::new(asset_pool.credit_asset.amount * Uint128::new(1000000000000000000u128)){
        //If greater then repay what's possible 
        repay_asset.amount = asset_pool.credit_asset.amount;
        leftover = liq_amount - Decimal::new(asset_pool.credit_asset.amount * Uint128::new(1000000000000000000u128));

    }else{ //Pay what's being asked
        repay_asset.amount = liq_amount *  Uint128::new(1u128); // * 1
    }
    
    //Repay for the user
    let repay_msg = CDP_ExecuteMsg::LiqRepay { 
        credit_asset: repay_asset.clone(),
    };

    let coin: Coin = asset_to_coin(repay_asset)?;

    let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.owner.to_string(),
            msg: to_binary(&repay_msg)?,
            funds: vec![coin], 
    });

    //Edit and save asset pools in state
    //Actually won't do this until the distribute function 
    // asset_pool.credit_asset.amount -= repay_asset.amount;
    // let temp_pools: Vec<AssetPool> = asset_pools
    //     .into_iter()
    //     .filter(|x| !(x.credit_asset.info.equal(&credit_asset.info)))
    //     .collect::<Vec<AssetPool>>();
    // temp_pools.push(asset_pool);

    // ASSETS.save(deps.storage, &temp_pools)?;
    
    //Build the response
    //1) Repay message
    //2) Add position user info to response
    //3) Add potential leftover funds

    let mut res: Response = Response::new();
    
    Ok( res.add_message(message)
            .add_attributes(vec![
                attr( "method", "liquidate" ),
                attr("leftover_repayment", leftover.to_string())
            ]) )

   
}

//Calculate which and how much each user gets distributed from the liquidation
pub fn distribute_funds(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    distribution_assets: Vec<cAsset>,
    credit_asset: AssetInfo,
    credit_price: Decimal,
) -> Result<Response, ContractError>{

    let config = CONFIG.load(deps.storage)?;
    //Can only be called by its owner 
    if info.sender != config.owner {
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
    let mut assets: Vec<Asset> = distribution_assets.clone()
        .into_iter()
        .map(|cAsset| cAsset.asset )
        .collect::<Vec<Asset>>();
    assets = validate_assets(deps.storage, assets, info, false, true)?;
    
    if assets.len() != distribution_assets.len() { return Err(ContractError::InvalidAssetObject{ }) }
    

    //Check difference between AssetPool state and contract funds to confirm how much to distribute
    //Query contract for current asset totals
    let repaid_amount: Uint128;
    //Match is bc there is an uncertainty around credit asset type
    match credit_asset.clone(){
        AssetInfo::Token { address } => {

            let contract_coin: BalanceResponse = deps.querier.query(
                &QueryRequest::Wasm(WasmQuery::Smart { 
                    contract_addr: address.to_string(), 
                    msg:  to_binary(&Cw20QueryMsg::Balance { 
                        address: env.contract.address.to_string(),  
                    })?
                }
                   ))?;
                repaid_amount = match asset_pool.credit_asset.amount.checked_sub( contract_coin.amount.amount ){
                    Ok( amount ) => amount,
                    Err(_) => return Err(ContractError::MismanagedState { }),
                };
        },
        AssetInfo::NativeToken { denom } => {
            
            let contract_coin: BalanceResponse = deps.querier.query(
                &QueryRequest::Bank (
                    BankQuery::Balance { 
                        address: env.contract.address.to_string(), 
                        denom, 
                    }))?;

            
            repaid_amount = match asset_pool.credit_asset.amount.checked_sub( contract_coin.amount.amount ){
                Ok( amount ) => amount,
                Err(_) => return Err(ContractError::MismanagedState { }),
            };
            
        },
    }
    
    
    //Ensure funds are at least equal to the repaid funds so SP users can't lose money
    let (cAsset_values, _cAsset_prices) = get_asset_values(deps.querier, distribution_assets.clone())?;
    let total_asset_value: Decimal = cAsset_values.iter().sum();

    
    let repaid_value = decimal_multiplication(Decimal::from_ratio(repaid_amount, Uint128::new(1u128)), credit_price);
    
    if repaid_value > total_asset_value{
        return Err(ContractError::InsufficientFunds{ })
    }
    //^This would only happen if assets fall too quickly that liquidations aren't called fast enough. Using the collateral ratio as buffer time.
    
    //Edit and save state now that the data has been taken 
    asset_pool.credit_asset.amount -= repaid_amount;
    let mut temp_pools: Vec<AssetPool> = asset_pools.clone()
        .into_iter()
        .filter(| pool | !(pool.credit_asset.info.equal(&credit_asset)))
        .collect::<Vec<AssetPool>>();
    temp_pools.push(asset_pool.clone());

    ASSETS.save(deps.storage, &temp_pools)?;

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
                    
                    //Subtract what's left to add
                    let remaining_repayment = repaid_amount_decimal - current_repay_total;

                    deposit.amount -= remaining_repayment;
                    current_repay_total += remaining_repayment;

                    //Add Deposit w/ amount = to remaining_repayment
                    //Splits original Deposit amount between both Vecs
                    distribution_list.push( Deposit{
                        amount: remaining_repayment,
                        ..deposit
                    } );

                }else{//Else, keep adding 
                    
                    current_repay_total += deposit.amount;

                    distribution_list.push( deposit );
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
        

    let edited_deposits: Vec<Deposit> = asset_pool.clone().deposits
        .into_iter()
        .filter(|deposit| !deposit.equal(&distribution_list))
        .collect::<Vec<Deposit>>();
        
    asset_pool.deposits = edited_deposits;
    
    let mut new_pools: Vec<AssetPool> = ASSETS.load(deps.storage)?
        .into_iter()
        .filter(|pool| pool.credit_asset.info.equal(&credit_asset))
        .collect::<Vec<AssetPool>>();
    new_pools.push(asset_pool);

    //Save pools w/ edited deposits to state
    ASSETS.save(deps.storage, &new_pools)?;

    //create function to find user ratios and distribute collateral based on them
    //1 collateral at a time for gas and UX optimizations (ie if a user wants to sell they won't have to sell on 4 different pairs)
    //Also bc native tokens come in batches, CW20s come separately 
    let ratios = get_distribution_ratios(distribution_list.clone())?;

    let mut distribution_ratios: Vec<UserRatio> = distribution_list
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
    
    let mut cAsset_ratios = get_cAsset_ratios(deps.querier, distribution_assets.clone())?;
    let messages: Vec<CosmosMsg> = vec![];

        
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
                                .find( | asset | asset.info.equal(&distribution_assets[index].asset.info) ){

                                    Some( mut asset) => {
                                        //Add claim amount to the asset object
                                        asset.amount += distribution_assets[index].asset.amount;
                                        
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
                                            info: distribution_assets[index].clone().asset.info,
                                            amount: distribution_assets[index].clone().asset.amount
                                        });
                                    }
                            }

                            Ok( some_user )
                        },
                        None => {
                            //Create object for user
                            Ok( User{
                                claimable_assets: vec![ Asset{
                                    info: distribution_assets[index].clone().asset.info,
                                    amount: distribution_assets[index].clone().asset.amount
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
                let send_amount = decimal_multiplication(send_ratio, Decimal::from_ratio(distribution_assets[index].asset.amount, Uint128::new(1u128))) * Uint128::new(1u128);
                
                //Add to existing user claims
                USERS.update(deps.storage, user_ratio.clone().user, |user| -> Result<User, ContractError> {

                    match user{
                        Some ( mut user ) => {
                            //Find Asset in user state
                            match user.clone().claimable_assets.into_iter()
                                .find( | asset | asset.info.equal(&distribution_assets[index].asset.info) ){

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
                                            info: distribution_assets[index].clone().asset.info,
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
                                    info: distribution_assets[index].clone().asset.info,
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
                                .find( | asset | asset.info.equal(&distribution_assets[index].asset.info) ){

                                    Some( mut asset) => {
                                        asset.amount += distribution_assets[index].asset.amount;

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
                                            info: distribution_assets[index].clone().asset.info,
                                            amount: distribution_assets[index].clone().asset.amount
                                        });
                                    }
                            }

                            Ok( user )
                        },
                        None => {
                            //Create object for user
                            Ok( User{
                                claimable_assets: vec![ Asset{
                                    info: distribution_assets[index].clone().asset.info,
                                    amount: distribution_assets[index].clone().asset.amount
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
    let assets: Vec<Asset> = distribution_assets
        .into_iter()
        .map(|cAsset| cAsset.asset)
        .collect::<Vec<Asset>>();

    let res = Response::new()
    .add_attribute("method", "distribute")
    .add_attribute("credit_asset", credit_asset.to_string());

    let mut attrs = vec![];
    let assets_as_string: Vec<String> = assets.iter().map(|x| x.to_string()).collect();
    for i in 0..assets.clone().len(){
        attrs.push(("distribution_assets", &assets_as_string[i]));    
    }

    Ok( res.add_attributes(attrs) )

}

//Sends available claims to info.sender
//If asset is passed, the claims will be sent as said asset
pub fn claim(
    deps: DepsMut,
    info: MessageInfo,
    claim_as: Option<String>,
    deposit_to: Option<PositionUserInfo>,
) -> Result<Response, ContractError>{

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut user: Option<User> = None;

    match claim_as{
        Some( address ) => { /*Route thru Osmosis*/
            let valid_addr = deps.api.addr_validate( &address )?;
        
            //TODO: Create router for Osmosis
            //Add msg creation from match arm below
        },
        None => { //Send all claimable assets
            
            user = match USERS.load(deps.storage, info.clone().sender){
                Ok( user ) => Some( user ),
                Err(_) => { None }
            };

            //If we are depositing to a Position then we don't use a regular withdrawal msg
            if deposit_to.is_some(){
                let msgs: Vec<CosmosMsg> = deposit_to_position( deps.storage, user.clone().unwrap().claimable_assets, deposit_to.unwrap(), info.clone().sender)?;
                messages.extend(msgs);
            }else{
            for asset in user.clone().unwrap().claimable_assets{
                messages.push( withdrawal_msg(asset, info.clone().sender)? );
            }}
        }
    }

    let res = Response::new()
        .add_attribute("user", info.sender);
    
   
    Ok( res
        .add_messages(messages)
    )
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
                    position_id: Some(deposit_to.clone().position_id),
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

    let asset_info: Vec<AssetInfo> = assets
        .into_iter()
        .map(|asset| {
            asset.info
        }).collect::<Vec<AssetInfo>>();

    //Adds Native token deposit msg to messages
    let deposit_msg = CDP_ExecuteMsg::Deposit { 
        assets: asset_info, 
        position_owner: Some(user.to_string()),
        basket_id: deposit_to.clone().basket_id,
        position_id: Some(deposit_to.clone().position_id),
    };

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.owner.to_string(), //Assumption that this is the Positions contract
        msg: to_binary(&deposit_msg)?,
        funds: coins,
    });


    messages.push(msg);



    Ok( messages )
}

pub fn get_cAsset_ratios(
    deps: QuerierWrapper,
    collateral_assets: Vec<cAsset>
) -> StdResult<Vec<Decimal>>{
    let (cAsset_values, cAsset_prices) = get_asset_values(deps, collateral_assets.clone())?;

    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values{
        cAsset_ratios.push(cAsset/total_value) ;
    }

    //TODO: Oracle related Testing purposes. Delete.
    if cAsset_ratios.len() == 0 { cAsset_ratios.push(Decimal::percent(50));
        cAsset_ratios.push(Decimal::percent(50));}

    //Error correction for ratios so we end up w/ least amount undistributed funds
    let ratio_total: Option<Decimal> = Some(cAsset_ratios.iter().sum());


    if ratio_total.unwrap() != Decimal::percent(100){
        let mut new_ratios: Vec<Decimal> = vec![];
        
        match ratio_total{
            Some( total ) if total > Decimal::percent(100) => {

                    let margin_of_error = total - Decimal::percent(100);

                    let num_users = Decimal::new(Uint128::from( cAsset_ratios.len() as u128 ));

                    let error_correction = decimal_division( margin_of_error, num_users );

                    new_ratios = cAsset_ratios.into_iter()
                        .map(|ratio| 
                            decimal_subtraction( ratio, error_correction )
                        ).collect::<Vec<Decimal>>();
                    
            },
            Some( total ) if total < Decimal::percent(100) => {

                let margin_of_error = Decimal::percent(100) - total;

                let num_users = Decimal::new(Uint128::from( cAsset_ratios.len() as u128 ));

                let error_correction = decimal_division( margin_of_error, num_users );

                new_ratios = cAsset_ratios.into_iter()
                        .map(|ratio| 
                            ratio + error_correction
                        ).collect::<Vec<Decimal>>();
                
            },
            None => { return Err(StdError::GenericErr { msg: "Input amounts were null".to_string() }) },
            Some(_) => { /*Unreachable due to if statement*/ },
        }

        return Ok( new_ratios )
    }

    Ok( cAsset_ratios )
}

pub fn get_avg_LTV(
    deps: QuerierWrapper, 
    collateral_assets: Vec<cAsset>,
)-> StdResult<(Decimal, Decimal, Decimal, Vec<Decimal>)>{

    let (cAsset_values, cAsset_prices) = get_asset_values(deps, collateral_assets.clone())?;

    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values{
        cAsset_ratios.push(cAsset/total_value) ;
    }

    //converting % of value to avg_LTV by multiplying collateral LTV by % of total value
    let mut avg_max_LTV: Decimal = Decimal::new(Uint128::from(0u128));
    let mut avg_borrow_LTV: Decimal = Decimal::new(Uint128::from(0u128));

    if cAsset_ratios.len() == 0{
        //TODO: Change back to no values. This is for testing without oracles
       //return Ok((Decimal::percent(0), Decimal::percent(0), Decimal::percent(0)))
       return Ok((Decimal::percent(50), Decimal::percent(50), Decimal::percent(100000000), vec![]))
    }
    
    for (i, _cAsset) in collateral_assets.clone().iter().enumerate(){
        avg_borrow_LTV += decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_borrow_LTV);
    }

    for (i, _cAsset) in collateral_assets.clone().iter().enumerate(){
        avg_max_LTV += decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_LTV);
    }
    

    Ok((avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices))
}

pub fn get_asset_values(deps: QuerierWrapper, assets: Vec<cAsset>) -> StdResult<(Vec<Decimal>, Vec<Decimal>)>
{

   /* let timeframe: Option<u64> = if check_expire {
        Some(PRICE_EXPIRE_TIME)
    } else {
        None
    };*/

    //Getting proportions for position collateral to calculate avg LTV
    //Using the index in the for loop to parse through the assets Vec and collateral_assets Vec
    //, as they are now aligned due to the collateral check w/ the Config's data
    let cAsset_values: Vec<Decimal> = Vec::new();
    let cAsset_prices: Vec<Decimal> = Vec::new();

    for (i,n) in assets.iter().enumerate() {

    //    //TODO: Query collateral prices from the oracle
    //    let collateral_price = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
    //         contract_addr: assets[i].oracle.to_string(),
    //         msg: to_binary(&OracleQueryMsg::Price {
    //             asset_token: assets[i].asset.info.to_string(),
    //             None,
    //         })?,
    //     }))?;

        //cAsset_prices.push(collateral_price.rate);
        // let collateral_value = decimal_multiplication(Decimal::new(assets[i].asset.amount), collateral_price.rate);
        // cAsset_values.push(collateral_value); 

    }
    Ok((cAsset_values, cAsset_prices))
}




pub fn get_distribution_ratios(
    deposits: Vec<Deposit>
) -> StdResult<Vec<Decimal>>{
    
    let mut user_deposits: Vec<Deposit> = vec![];
    let mut total_amount: Decimal = Decimal::percent(0);
    let mut new_deposits: Vec<Deposit> = vec![];

    //For each Deposit, create a condensed Deposit for its user.
    //Add to an existing one if found.
    for deposit in deposits.into_iter(){

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
    for deposit in user_deposits{
        user_ratios.push(decimal_division(deposit.amount,total_amount));
    }

    //Error correction for distribution so we end up w/ least amount undistributed funds/ dust
    let ratio_total: Option<Decimal> = Some(user_ratios.iter().sum());
    
    
    if ratio_total.unwrap() != Decimal::percent(100){
        let mut new_ratios: Vec<Decimal> = vec![];
        
        match ratio_total{
            Some( total ) if total > Decimal::percent(100) => {

                    let margin_of_error = total - Decimal::percent(100);

                    let num_users = Decimal::new(Uint128::from( user_ratios.len() as u128 ));

                    let error_correction = decimal_division( margin_of_error, num_users );

                    new_ratios = user_ratios.into_iter()
                        .map(|ratio| 
                            decimal_subtraction( ratio, error_correction )
                        ).collect::<Vec<Decimal>>();
                    
            },
            Some( total ) if total < Decimal::percent(100) => {

                let margin_of_error = Decimal::percent(100) - total;

                let num_users = Decimal::new(Uint128::from( user_ratios.len() as u128 ));

                let error_correction = decimal_division( margin_of_error, num_users );

                new_ratios = user_ratios.into_iter()
                .map(|ratio| 
                    ratio + error_correction
                ).collect::<Vec<Decimal>>();
            },
            None => { return Err(StdError::GenericErr { msg: "Input amounts were null".to_string() }) },
            Some(_) => { /*Unreachable due to if statement*/ },
        }

        return Ok( new_ratios )
    }

    Ok( user_ratios )
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

   // let new_assets: Vec<LiqAsset>;

    for asset in liq_assets{

        //If the asset has a pool, validate its balance
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
    assets: Vec<Asset>,
    info: MessageInfo,
    in_pool: bool,
    sent_assertion: bool,
) -> Result< Vec<Asset>, ContractError>{

    let mut valid_assets: Vec<Asset> = vec![];

    if in_pool{

        //Validate sent assets against accepted assets
        let asset_pools = ASSETS.load(deps)?;

        for asset in assets{
            //If the asset has a pool, validate its balance
            match asset_pools.iter().find(|x| x.credit_asset.info.equal(&asset.info)) {
                Some( _a) => {
                    match asset.info {
                        AssetInfo::NativeToken { denom: _ } => {

                            if sent_assertion{
                                match assert_sent_native_token_balance(&asset, &info){
                                    Ok(_) => {
                                        valid_assets.push(asset);
                                    },
                                    Err(_) => {},
                                }
                            }else{
                                valid_assets.push(asset);
                            }
                            
                        },
                        AssetInfo::Token { address: _ } => {
                             //Functions assume Cw20 asset amounts are taken from Messageinfo
                            valid_assets.push(asset)
                         }
                    }
                },
                None => { },
            }
        }
    }else{
        for asset in assets{
            match asset.info {
                AssetInfo::NativeToken { denom: _ } => {
                    
                    match assert_sent_native_token_balance(&asset, &info){
                        Ok(_) => {
                            valid_assets.push(asset);
                        },
                        Err(_) => {},
                    }
                    

                },
                AssetInfo::Token { address: _ } => {
                    //Functions assume Cw20 asset amounts are taken from Messageinfo
                   valid_assets.push(asset)}
            }
        
        }
    }
    

    Ok( valid_assets )
}

//Refactored Terraswap function
pub fn assert_sent_native_token_balance(
    asset: &Asset,
    message_info: &MessageInfo
)-> StdResult<()> {
    
    if let AssetInfo::NativeToken { denom} = &asset.info {
        match message_info.funds.iter().find(|x| x.denom == *denom) {
            Some(coin) => {
                if asset.amount == coin.amount {
                    Ok(())
                } else {
                    Err(StdError::generic_err("Sent coin.amount is different from asset.amount"))
                }
            },
            None => {
                {
                    Err(StdError::generic_err("Incorrect denomination, sent asset denom and asset.info.denom differ"))
                }
            }
        }
    } else {
        Err(StdError::generic_err("Asset type not native, check Msg schema and use AssetInfo::Token{ address: Addr }"))
    }
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


#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies_with_balance, mock_env, mock_info};
    use cosmwasm_std::{coins, from_binary, attr, SubMsg};

    #[test]
    fn deposit() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
        };

        let mut coin = coins(11, "credit");
        coin.append(&mut coins(11, "2ndcredit"));
        //Instantiating contract
        let info = mock_info("sender88", &coin);
        let res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "instantiate"),
            attr("owner", "sender88"),
            ]
        );

        //Depositing 2 invalid asset
        let assets: Vec<Asset> = vec![
            Asset { 
                info: AssetInfo::NativeToken { denom: "notcredit".to_string() }, 
                amount: Uint128::new(10u128) },
            Asset { 
                info: AssetInfo::NativeToken { denom: "notnotcredit".to_string() }, 
                amount: Uint128::new(10u128) }
        ];

        let deposit_msg = ExecuteMsg::Deposit { 
            assets,
            user: None,
        };

        //Fail due to Invalid Asset
        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        //Query position data to make sure it was NOT saved to state 
        let res = query(deps.as_ref(),
        mock_env(),
        QueryMsg::AssetDeposits {
            user: "sender88".to_string(),
            asset_info: AssetInfo::NativeToken { denom: "notcredit".to_string() }
        });
        let error = "User has no open positions in this asset pool or the pool doesn't exist".to_string();
        
        match res {
            Err(StdError::GenericErr { msg: error }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Deposit should've failed due to an invalid asset"),
        } 

        //Add Pool for a 2nd deposit asset
        let add_msg = ExecuteMsg::AddPool { 
            asset_pool: AssetPool{
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                    amount: Uint128::zero() },
                liq_premium: Decimal::zero(),
                deposits: vec![],
            }
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), add_msg).unwrap();

        //Successful attempt
        let assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(11u128),},
            Asset { 
                info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                amount: Uint128::new(11u128) }
            
        ];

        let deposit_msg = ExecuteMsg::Deposit { 
            assets,
            user: None,
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "deposit"),
            attr("position_owner","sender88"),
            attr("deposited_assets", "11 credit"),
            attr("deposited_assets", "11 2ndcredit"),
            ]
        );

        //Query position data to make sure it was saved to state correctly
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::AssetDeposits {
                user: "sender88".to_string(),
                asset_info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }
            })
            .unwrap();
        
        let resp: DepositResponse = from_binary(&res).unwrap();

        assert_eq!(resp.asset.to_string(), "2ndcredit".to_string());
        assert_eq!(resp.deposits[0].to_string(), "sender88 11".to_string());

        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::AssetPool {
                asset_info: AssetInfo::NativeToken { denom: "credit".to_string() }
            })
            .unwrap();
        
        let resp: PoolResponse = from_binary(&res).unwrap();

        assert_eq!(resp.credit_asset.to_string(), "11 credit".to_string());
        assert_eq!(resp.liq_premium.to_string(), "0".to_string());
        assert_eq!(resp.deposits[0].to_string(), "sender88 11".to_string());


    }


    #[test]
    fn withdrawal() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
        };

        let mut coin = coins(11, "credit");
        coin.append(&mut coins(11, "2ndcredit"));
        //Instantiating contract
        let info = mock_info("sender88", &coin);
        let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        //Depositing 2 asseta
        //Add Pool for a 2nd deposit asset
        let add_msg = ExecuteMsg::AddPool { 
            asset_pool: AssetPool{
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                    amount: Uint128::zero() },
                liq_premium: Decimal::zero(),
                deposits: vec![],
            }
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), add_msg).unwrap();

        //Successful attempt
        let assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(11u128),},
            Asset { 
                info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                amount: Uint128::new(11u128) }
            
        ];

        let deposit_msg = ExecuteMsg::Deposit { 
            assets,
            user: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();
        

        //Invalid Asset
        let assets: Vec<Asset> = vec![
           Asset { 
                info: AssetInfo::NativeToken { denom: "notcredit".to_string() }, 
                amount: Uint128::new(0u128) }
        ];

        let withdraw_msg = ExecuteMsg::Withdraw { 
            assets,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), withdraw_msg);

         //Query position data to make sure nothing was withdrawn
         let res = query(deps.as_ref(),
         mock_env(),
         QueryMsg::AssetDeposits {
             user: "sender88".to_string(),
             asset_info: AssetInfo::NativeToken { denom: "credit".to_string() }
         }).unwrap();
     
         let resp: DepositResponse = from_binary(&res).unwrap();

        assert_eq!(resp.asset.to_string(), "credit".to_string());
        assert_eq!(resp.deposits[0].to_string(), "sender88 11".to_string());
        /////////////////////

        //Invalid Withdrawal "Amount too high"
        let assets: Vec<Asset> = vec![
           Asset { 
                info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                amount: Uint128::new(12u128) }
        ];

        let withdraw_msg = ExecuteMsg::Withdraw { 
            assets,
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), withdraw_msg);

        match res {
            Err(ContractError::InvalidWithdrawal {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Withdrawal amount too high, should've failed"),
        } 
        
        //Successful attempt
        let assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(5u128),},
            Asset { 
                info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                amount: Uint128::new(5u128) }
            
        ];

        let withdraw_msg = ExecuteMsg::Withdraw { 
            assets,
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), withdraw_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "withdraw"),
            attr("position_owner","sender88"),
            attr("withdrawn_assets", "5 credit"),
            attr("withdrawn_assets", "5 credit"),
            ]
        );

        //Query position data to make sure it was saved to state correctly
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::AssetDeposits {
                user: "sender88".to_string(),
                asset_info: AssetInfo::NativeToken { denom: "credit".to_string() }
            })
            .unwrap();
        
        let resp: DepositResponse = from_binary(&res).unwrap();

        assert_eq!(resp.asset.to_string(), "credit".to_string());
        assert_eq!(resp.deposits[0].to_string(), "sender88 1".to_string());

         //Successful attempt
         let assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(1u128),
            }
        ];

        let withdraw_msg = ExecuteMsg::Withdraw { 
            assets,
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), withdraw_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "withdraw"),
            attr("position_owner","sender88"),
            attr("withdrawn_assets", "1 credit"),
            ]
        );

        //Query position data to make sure it was deleted from state 
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::AssetDeposits {
                user: "sender88".to_string(),
                asset_info: AssetInfo::NativeToken { denom: "credit".to_string() }
            });
    
        
        let error = "User has no open positions in this asset pool or the pool doesn't exist".to_string();
        
        match res {
            Err(StdError::GenericErr { msg: error }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Deposit should've failed due to an invalid withdrawal amount"),
        }
        

    }

    #[test]
    fn liquidate(){

        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
        };

        let mut coin = coins(11, "credit");
        coin.append(&mut coins(11, "2ndcredit"));
        //Instantiating contract
        let info = mock_info("sender88", &coin);
        let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        //Depositing 2 asset
        //Add Pool for a 2nd deposit asset
        let add_msg = ExecuteMsg::AddPool { 
            asset_pool: AssetPool{
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                    amount: Uint128::zero() },
                liq_premium: Decimal::zero(),
                deposits: vec![],
            }
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), add_msg).unwrap();

        //Successful attempt
        let assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(11u128),},
            Asset { 
                info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                amount: Uint128::new(11u128) }
            
        ];

        let deposit_msg = ExecuteMsg::Deposit { 
            assets,
            user: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        //Unauthorized Sender
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Decimal::zero(),
            },  
        };

        let unauthorized_info = mock_info("notsender", &coins(0, "credit"));

        let res = execute(deps.as_mut(), mock_env(), unauthorized_info.clone(), liq_msg);

        match res {
            Err(ContractError::Unauthorized {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Liquidation should have failed bc of an unauthorized sender"),
        } 


        //Invalid Credit Asset
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "notcredit".to_string() },
                amount: Decimal::zero(),
            }, 
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), liq_msg);

        match res {
            Err(ContractError::InvalidAsset {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Liquidation should have failed bc of an invalid credit asset"),
        } 

        //Successful Attempt
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Decimal::from_ratio(1u128, 1u128),
            }, 
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), liq_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "liquidate"),
            attr("leftover_repayment", "0 credit"),
        ]);

        let config = CONFIG.load(&deps.storage).unwrap();

        assert_eq!(
            res.messages,
            vec![SubMsg::new(
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.owner.to_string(),
                    funds: vec![Coin { 
                        denom: "credit".to_string(), 
                        amount: Uint128::new(1u128) 
                    }],
                    msg: to_binary(&CDP_ExecuteMsg::LiqRepay {
                        credit_asset:Asset{
                            info: AssetInfo::NativeToken{
                                denom:"credit".to_string()
                            },
                            amount:Uint128::from(1u128),
                        }, 
                    })
                    .unwrap(),
                })
            )]
        );

    }

    #[test]
    fn distribute(){
        env::set_var("RUST_BACKTRACE", "1");

        let mut deps = mock_dependencies_with_balance(&coins(2, "credit"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
        };

        //Instantiating contract
        let info = mock_info("sender88", &coins(5, "credit"));
        let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        //Unauthorized Sender
        let distribute_msg = ExecuteMsg::Distribute { 
            distribution_assets: vec![],
            credit_asset: AssetInfo::NativeToken { denom: "credit".to_string() }, 
            credit_price: Decimal::zero(),
        };

        let unauthorized_info = mock_info("notsender", &coins(0, "credit"));

        let res = execute(deps.as_mut(), mock_env(), unauthorized_info.clone(), distribute_msg);

        match res {
            Err(ContractError::Unauthorized {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Distribution should have failed bc of an unauthorized sender"),
        } 

        //Invalid Credit Asset
        let distribute_msg = ExecuteMsg::Distribute { 
            distribution_assets: vec![cAsset{
                asset:  Asset { 
                    info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                    amount: Uint128::new(100u128) },
                oracle: "funnybone".to_string(),
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(90),
            }],
            credit_asset: AssetInfo::NativeToken { denom: "notcredit".to_string() }, 
            credit_price: Decimal::zero(),
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg);

        match res {
            Err(ContractError::InvalidAsset {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Distribution should've failed bc of an invalid credit asset"),
        } 

        //Deposit for first user
        let assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(5u128),
            }
        ];

        let deposit_msg = ExecuteMsg::Deposit { 
            assets: assets.clone(),
            user: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        //Deposit for second user
        let deposit_msg = ExecuteMsg::Deposit { 
            assets,
            user: Some("2nduser".to_string()),
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

         //Insufficient Funds
         let distribute_msg = ExecuteMsg::Distribute { 
            distribution_assets: vec![],
            credit_asset: AssetInfo::NativeToken { denom: "credit".to_string() }, 
            credit_price: Decimal::from_ratio(Uint128::new(1u128), Uint128::new(1u128)),
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg);

        match res {
            Err(ContractError::InsufficientFunds {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Distribution should've failed bc of insufficient dsitribution asseets"),
        } 

        //Succesfful attempt
        //I know how to simulate this. Need a successful deposit, liquidate call and a distribute call
        

        //Liquidation
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Decimal::from_ratio(8u128, 1u128),
            }, 
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), liq_msg).unwrap();

        //Distribute
        let distribute_msg = ExecuteMsg::Distribute { 
            distribution_assets: vec![cAsset{
                asset:  Asset { 
                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                    amount: Uint128::new(100u128) },
                oracle: "funnybone".to_string(),
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(90),
            },
            cAsset{
                asset:  Asset { 
                    info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, 
                    amount: Uint128::new(100u128) },
                oracle: "funnybone".to_string(),
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(90),
            }],
            credit_asset: AssetInfo::NativeToken { denom: "credit".to_string() }, 
            credit_price: Decimal::from_ratio(Uint128::new(0u128), Uint128::new(1u128)), //This is 0 so we don't trigger Insufficient funds
        };
        
        let mut coin = coins(100, "debit");
        coin.append(&mut coins(100, "2nddebit"));

        let info = mock_info("sender88", &coin);

        let res = execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "distribute"),
            attr("credit_asset", "credit"),
            attr("distribution_assets", "100 debit"),
            attr("distribution_assets", "100 2nddebit"),
        ]);

        //Query and assert User claimables
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::UserClaims {
                user: "sender88".to_string(),
            }).unwrap();

         
        let resp: ClaimsResponse = from_binary(&res).unwrap();
        
        assert_eq!(resp.claims[0].to_string(), "100 debit".to_string());
        assert_eq!(resp.claims[1].to_string(), "25 2nddebit".to_string());

        //Query and assert User claimables
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::UserClaims {
                user: "2nduser".to_string(),
            }).unwrap();

         
        let resp: ClaimsResponse = from_binary(&res).unwrap();
        
        assert_eq!(resp.claims[0].to_string(), "75 2nddebit".to_string());
        
        
    }

    
    #[test]
    fn add_asset_pool(){

        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
        };

        //Instantiating contract
        let info = mock_info("sender88", &coins(11, "credit"));
        let res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();


         //Unauthorized Sender
         let add_msg = ExecuteMsg::AddPool { 
            asset_pool: AssetPool{
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                    amount: Uint128::zero() },
                liq_premium: Decimal::zero(),
                deposits: vec![],
            }
        };

        let unauthorized_info = mock_info("notsender", &coins(0, "credit"));

        let res = execute(deps.as_mut(), mock_env(), unauthorized_info.clone(), add_msg.clone());

        match res {
            Err(ContractError::Unauthorized {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Message should have failed bc of an unauthorized sender"),
        } 

         //Successful Attempt
        let res = execute(deps.as_mut(), mock_env(), info.clone(), add_msg.clone()).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "add_asset_pool"),
            attr("asset","0 2ndcredit"),
            attr("premium", "0"),
            ]);
        }

        //TODO: Add AssetPoolQuery

}