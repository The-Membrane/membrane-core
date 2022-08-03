

use std::str::FromStr;

use cosmwasm_bignumber::Uint256;
use cosmwasm_std::{MessageInfo, attr, Response, DepsMut, Uint128, CosmosMsg, Decimal, Storage, Api, Coin, to_binary, QueryRequest, WasmQuery, QuerierWrapper, StdResult, StdError, Addr, WasmMsg, BankMsg, SubMsg, coin, Env};
use cosmwasm_storage::{Bucket, ReadonlyBucket};
use cw20::Cw20ExecuteMsg;
use osmo_bindings::{ SpotPriceResponse, PoolStateResponse };

use membrane::{types::{Asset, Basket, Position, cAsset, AssetInfo, SellWallDistribution, RepayPropagation, LiqAsset, UserInfo, PriceInfo}, positions::CallbackMsg};
use membrane::positions::ExecuteMsg;
use membrane::apollo_router::{ExecuteMsg as RouterExecuteMsg, Cw20HookMsg as RouterHookMsg};
use membrane::liq_queue::{ExecuteMsg as LQ_ExecuteMsg, QueryMsg as LQ_QueryMsg, LiquidatibleResponse as LQ_LiquidatibleResponse };
use membrane::stability_pool::{Cw20HookMsg as SP_Cw20HookMsg, QueryMsg as SP_QueryMsg, LiquidatibleResponse as SP_LiquidatibleResponse, PoolResponse, ExecuteMsg as SP_ExecuteMsg};
use membrane::osmosis_proxy::{ ExecuteMsg as OsmoExecuteMsg, QueryMsg as OsmoQueryMsg };

use crate::{ContractError, state::{REPAY, CONFIG, BASKETS, POSITIONS, Config}, math::{decimal_multiplication, decimal_division, decimal_subtraction}, query::{query_stability_pool_fee, query_stability_pool_liquidatible}};

pub const LIQ_QUEUE_REPLY_ID: u64 = 1u64;
pub const STABILITY_POOL_REPLY_ID: u64 = 2u64;
pub const SELL_WALL_REPLY_ID: u64 = 3u64;
pub const CREATE_DENOM_REPLY_ID: u64 = 4u64;
pub const BAD_DEBT_REPLY_ID: u64 = 999999u64;

static PREFIX_PRICE: &[u8] = b"price";

//Deposit collateral to existing position. New or same collateral.
//Anyone can deposit, to any position. There will be barriers for withdrawals.
pub fn deposit(
    deps: DepsMut,
    info: MessageInfo,
    position_owner: Option<String>,
    position_id: Option<Uint128>,
    basket_id: Uint128,
    cAssets: Vec<cAsset>,
) -> Result<Response, ContractError>{

    let mut new_position_id: Uint128 = Uint128::new(0u128);

    let valid_owner_addr = validate_position_owner(deps.api, info, position_owner)?;

    let basket: Basket = match BASKETS.load(deps.storage, basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };

    //This has to error bc users can't withdraw without a price set. Don't want to trap users.
    if basket.credit_price.is_none(){ return Err(ContractError::NoRepaymentPrice {  })}


    let mut new_position: Position;
       
    match POSITIONS.load(deps.storage, (basket_id.to_string(), valid_owner_addr.clone())){
        
        //If Ok, adds collateral to the position_id or a new position is created            
        Ok( positions) => {

            //If the user wants to create a new/separate position, no position id is passed         
            if position_id.is_some(){

                let pos_id = position_id.unwrap();
                let position = positions.clone().into_iter().find(|x| x.position_id == pos_id);

                if position.is_some() {

                    //Go thru each deposited asset to add quantity to position
                    for deposited_cAsset in cAssets.clone(){
                        let deposited_asset = deposited_cAsset.clone().asset;

                        //HAve to reload positions each loop or else the state won't be edited for multiple deposits
                        //We can unwrap and ? safety bc of the layered conditionals
                        let position_s =  POSITIONS.load(deps.storage, (basket_id.to_string(), valid_owner_addr.clone()))?;
                        let existing_position = position_s.clone().into_iter().find(|x| x.position_id == pos_id).unwrap();

                        //Search for cAsset in the position then match
                        let temp_cAsset: Option<cAsset> = existing_position.clone().collateral_assets.into_iter().find(|x| x.asset.info.equal(&deposited_asset.clone().info));

                        match temp_cAsset {
                            //If Some, add amount to cAsset in the position
                            Some(cAsset) => {
                                let new_cAsset = cAsset{
                                    asset: Asset {
                                        amount: cAsset.clone().asset.amount + deposited_asset.clone().amount,
                                        info: cAsset.clone().asset.info,
                                    },
                                    debt_total: cAsset.clone().debt_total,
                                    max_borrow_LTV: cAsset.clone().max_borrow_LTV,
                                    max_LTV: cAsset.clone().max_LTV,
                                };

                                let mut temp_list: Vec<cAsset> = existing_position.clone().collateral_assets.into_iter().filter(|x| !x.asset.info.equal(&deposited_asset.clone().info)).collect::<Vec<cAsset>>();
                                temp_list.push(new_cAsset);

                                let temp_pos = Position {
                                    position_id: existing_position.clone().position_id,
                                    collateral_assets: temp_list,
                                    avg_borrow_LTV: existing_position.clone().avg_borrow_LTV, //We don't recalc bc it changes w/ price, leave it for solvency chcks
                                    avg_max_LTV: existing_position.clone().avg_max_LTV,
                                    credit_amount: existing_position.clone().credit_amount,
                                    basket_id: existing_position.clone().basket_id,
                                };


                                POSITIONS.update(deps.storage, (basket_id.to_string(), valid_owner_addr.clone()), |positions| -> Result<Vec<Position>, ContractError> 
                                {
                                    let unwrapped_pos = positions.unwrap();

                                    let mut update = unwrapped_pos.clone().into_iter().filter(|x| x.position_id != pos_id).collect::<Vec<Position>>();
                                    update.push(temp_pos);

                                    Ok( update )

                                })?;
                                

                            },
                            
                            // //if None, add cAsset to Position if in Basket options
                            None => {

                                let new_cAsset = deposited_cAsset.clone();

                                POSITIONS.update(deps.storage, (basket_id.to_string(), valid_owner_addr.clone()), |positions| -> Result<Vec<Position>, ContractError> 
                                {
                                    let temp_pos = positions.unwrap();
                                                                      
                                    let position = temp_pos.clone().into_iter().find(|x| x.position_id == pos_id);
                                    let mut p = position.clone().unwrap();
                                    p.collateral_assets.push(
                                        cAsset{
                                            asset: deposited_asset, 
                                            debt_total: Uint128::zero(),
                                            max_borrow_LTV:  new_cAsset.clone().max_borrow_LTV,
                                            max_LTV:  new_cAsset.clone().max_LTV,                                            
                                        }
                                    );

                                    let mut update = temp_pos.clone().into_iter().filter(|x| x.position_id != pos_id).collect::<Vec<Position>>();
                                    update.push( p );
                                    
                                    Ok( update )
                                        
                                })?;

                                
                            }
                        }

                    }
                    
                
                }else{
                    //If position_ID is passed but no position is found. In case its a mistake, don't want to add a new position.
                    return Err(ContractError::NonExistentPosition {  }) 
                }

            }else{
                //If user doesn't pass an ID, we create a new position
                new_position = create_position(deps.storage, cAssets.clone(), basket_id)?;
                
                //For response
                new_position_id = new_position.clone().position_id;
                
                //Need to add new position to the old set of positions if a new one was created.
                POSITIONS.update(deps.storage, (basket_id.to_string(), valid_owner_addr.clone()), |positions| -> Result<Vec<Position>, ContractError> 
                {
                    //We can .unwrap() here bc the initial .load() matched Ok()
                    let mut old_positions = positions.unwrap();

                    old_positions.push( new_position );

                    Ok( old_positions )

                })?;

            }
    

        
        },
        // If Err() meaning no positions loaded, new Vec<Position> is created 
        Err(_) => {

            new_position = create_position(deps.storage, cAssets.clone(), basket_id)?;
                
            //For response
            new_position_id = new_position.clone().position_id;
            
            //Add new Vec of Positions to state under the user
            POSITIONS.save(deps.storage, (basket_id.to_string(), valid_owner_addr.clone()), &vec![ new_position ] )?;
        }
    };

    //Response build
    let response = Response::new();
    let mut attrs = vec![];

    attrs.push(("method", "deposit"));

    let b = &basket_id.to_string();
    attrs.push(("basket_id", b));

    let v = &valid_owner_addr.to_string();
    attrs.push(("position_owner", v));

    let p = &position_id.unwrap_or_else(|| new_position_id).to_string();
    attrs.push(("position_id", p));

    let assets: Vec<String> = cAssets.iter().map(|x| x.asset.clone().to_string()).collect();
    
    for i in 0..assets.clone().len(){
        attrs.push(("assets", &assets[i]));    
    }

    Ok( response.add_attributes(attrs) )

}

pub fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    basket_id: Uint128,
    cAssets: Vec<cAsset>,
) ->Result<Response, ContractError>{

    let config: Config = CONFIG.load(deps.storage)?;

    let basket: Basket = match BASKETS.load(deps.storage, basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };
    
    
    let mut message: CosmosMsg;
    let mut msgs = vec![];
    let response = Response::new();
        

    //Each cAsset
    //We reload at every loop to account for edited state data. Otherwise users could siphon funds they don't own.
    for cAsset in cAssets.clone(){
        
        let withdraw_asset = cAsset.asset;

         //This forces withdrawals to be done by the info.sender 
        //so no need to check if the withdrawal is done by the position owner
        let positions: Vec<Position> = match POSITIONS.load(deps.storage, (basket_id.to_string(), info.sender.clone())){
            Err(_) => {  return Err(ContractError::NoUserPositions {  }) },
            Ok( positions ) => { positions },
        };

        //Search position by user and then filter by id
        let target_position = match positions.into_iter().find(|x| x.position_id == position_id) {
            Some(position) => position,
            None => return Err(ContractError::NonExistentPosition {  })
        };
        
        
        //If the cAsset is found in the position, attempt withdrawal 
        match target_position.clone().collateral_assets.into_iter().find(|x| x.asset.info.equal(&withdraw_asset.info)){
            //Some cAsset
            Some( position_collateral ) => {
                
                //Cant withdraw more than the positions amount
                if withdraw_asset.amount > position_collateral.asset.amount{
                    return Err(ContractError::InvalidWithdrawal {  })
                }else{
                    //Update cAsset data to account for the withdrawal
                    let leftover_amount = position_collateral.asset.amount - withdraw_asset.amount;
                                        

                    let mut updated_cAsset_list: Vec<cAsset> = target_position.clone().collateral_assets
                            .into_iter()
                            .filter(|x| !( x.asset.info.equal(&withdraw_asset.info) ))
                            .collect::<Vec<cAsset>>();


                    //Delete asset from the position if the amount is being fully withdrawn. In this case just don't push it
                    if leftover_amount != Uint128::new(0u128){
                        
                        let new_asset = Asset {
                            info: position_collateral.asset.info,
                            amount: leftover_amount,
                        };
    
                        let new_cAsset: cAsset = cAsset{
                            asset: new_asset,
                            ..position_collateral
                        };

                        updated_cAsset_list.push(new_cAsset);
                    }
                    
                                    
                    
                    //If resulting LTV makes the position insolvent, error. If not construct withdrawal_msg
                    if basket.credit_price.is_some(){
                        //This is taking max_borrow_LTV so users can't max borrow and then withdraw to get a higher initial LTV
                        if insolvency_check(deps.storage, env.clone(), deps.querier, updated_cAsset_list.clone(), target_position.clone().credit_amount, basket.credit_price.unwrap(), true, config.clone())?.0{ 
                            return Err(ContractError::PositionInsolvent {  })
                        }else{
                            
                            POSITIONS.update(deps.storage, (basket_id.to_string(), info.sender.clone()), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError>{

                                match positions {
                                    
                                    //Find the position we are withdrawing from to update
                                    Some(position_list) =>  
                                        match position_list.clone().into_iter().find(|x| x.position_id == position_id) {
                                        Some(position) => {

                                            let mut updated_positions: Vec<Position> = position_list
                                            .into_iter()
                                            .filter(|x| x.position_id != position_id)
                                            .collect::<Vec<Position>>();

                                            //Leave finding LTVs for solvency checks bc it uses deps. Can't be used inside of an update function
                                            // let new_avg_LTV = get_avg_LTV(deps.querier, updated_cAsset_list)?;

                                            updated_positions.push(
                                                Position{
                                                    avg_borrow_LTV: Decimal::percent(0),
                                                    avg_max_LTV: Decimal::percent(0),
                                                    collateral_assets: updated_cAsset_list.clone(),
                                                    ..position
                                            });
                                            Ok( updated_positions )
                                        },
                                        None => return Err(ContractError::NonExistentPosition {  })
                                    },
                                
                                    None => return Err(ContractError::NoUserPositions {  }),
                                }
                            })?;
                        }
                    }else{
                        return Err(ContractError::NoRepaymentPrice {  })
                    }
                    
                    //This is here in case there are multiple withdrawal messages created.
                    message = withdrawal_msg(withdraw_asset, info.sender.clone())?;
                    msgs.push(message);
                }
                
            },
            None => return Err(ContractError::InvalidCollateral {  })
        };
        
    }

    let mut attrs = vec![];
    attrs.push(("method", "withdraw"));
    
    //These placeholders are for lifetime warnings
    let b = &basket_id.to_string();
    attrs.push(("basket_id", b));

    let p = &position_id.to_string();
    attrs.push(("position_id", p));

    let temp: Vec<String> = cAssets.into_iter().map( |cAsset|
        cAsset.asset.to_string()
    ).collect::<Vec<String>>();

    for i in 0..temp.clone().len(){
        attrs.push(("assets", &temp[i]));    
    }

    
    Ok( response.add_attributes(attrs).add_messages(msgs) )
}

pub fn repay(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    api: &dyn Api,
    env: Env,
    info: MessageInfo,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: Option<String>,
    credit_asset: Asset,
) ->Result<Response, ContractError>{
    let config: Config = CONFIG.load( storage )?;
    
    let basket: Basket = match BASKETS.load(storage, basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };
        
    if basket.credit_price.is_none(){
        return Err(ContractError::NoRepaymentPrice {  })
    }

    let valid_owner_addr = validate_position_owner(api, info.clone(), position_owner)?;
    let target_position = get_target_position(storage, basket_id, valid_owner_addr.clone(), position_id)?;

    let response = Response::new();
    
    let mut total_loan: Decimal = Decimal::percent(0);
    let mut updated_list: Vec<Position> = vec![];


    //Assert that the correct credit_asset was sent
    //Only one of these match arms will be used once the credit_contract type is decided on
    match credit_asset.clone().info {
        AssetInfo::Token { address: submitted_address } => {
            if let AssetInfo::Token { address } = basket.credit_asset.info{

                if submitted_address != address || info.sender.clone() != address {
                    return Err(ContractError::InvalidCollateral {  })
                }
            };
            
        },
        AssetInfo::NativeToken { denom: submitted_denom } => { 
           
            if let AssetInfo::NativeToken { denom } = basket.credit_asset.info{

                if submitted_denom != denom {
                    return Err(ContractError::InvalidCollateral {  })
                }

            };            
            
        }
    }    
    POSITIONS.update(storage, (basket_id.to_string(), valid_owner_addr.clone()), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError>{

        match positions {

            Some(position_list) => {

               updated_list = match position_list.clone().into_iter().find(|x| x.position_id == position_id.clone()) {

                    Some( mut position) => {
                        
                        //Can the amount be repaid?
                        if position.credit_amount >= Decimal::from_ratio(credit_asset.amount, Uint128::new(1u128)) {
                            //Repay amount
                            position.credit_amount -= Decimal::from_ratio(credit_asset.amount, Uint128::new(1u128));
                            
                            //Position's resulting debt can't be below minimum without being fully repaid
                            if position.credit_amount < config.debt_minimum && !position.credit_amount.is_zero(){
                                return Err( ContractError::BelowMinimumDebt{})
                            }

                            total_loan = position.clone().credit_amount;
                        }else{
                            return Err(ContractError::ExcessRepayment {  })
                        }

                        //Create replacement Vec<Position> to update w/
                        let mut update: Vec<Position> = position_list.clone().into_iter().filter(|x| x.position_id != position_id.clone()).collect::<Vec<Position>>();
                        update.push( 
                            Position {
                                credit_amount: total_loan.clone(),
                                ..position
                            }
                         );
                       
                        update


                    },
                    None => return Err(ContractError::NonExistentPosition {  })

                };
                
                //Now update w/ the updated_list
                //The compiler is saying this value is never read so check in tests
                Ok( updated_list )
            },
                        
            None => return Err(ContractError::NoUserPositions {  }),

            }
    
    })?;

     //Subtract paid debt from debt-per-asset tallies
     update_basket_debt( storage, env, querier, config, basket_id, target_position.collateral_assets, credit_asset.amount, false )?;
    
    Ok( response.add_attributes(vec![
        attr("method", "repay".to_string() ),
        attr("basket_id", basket_id.to_string() ),
        attr("position_id", position_id.to_string() ),
        attr("loan_amount", total_loan.to_string() )]) )
}


//This is what the stability pool contract will call to repay for a liquidation and get its collateral distribution
//1) Repay
//2) Send position collateral + fee
pub fn liq_repay(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    credit_asset: Asset,
) ->Result<Response, ContractError>
{
    let config = CONFIG.load(deps.storage)?;
    let repay_propagation = REPAY.load(deps.storage)?;
    
    //Can only be called by the SP contract
    if config.clone().stability_pool.is_none() || info.sender != config.clone().stability_pool.unwrap(){
        return Err( ContractError::Unauthorized {  })
    }

    //These 3 checks shouldn't be possible since we are pulling the ids from state. 
    //Would have to be an issue w/ the repay_progation initialization
    let basket: Basket = match BASKETS.load(deps.storage, repay_propagation.clone().basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };

    let positions: Vec<Position> = match POSITIONS.load(deps.storage, (repay_propagation.clone().basket_id.to_string(), repay_propagation.clone().position_owner)){
        Err(_) => {  return Err(ContractError::NoUserPositions {  }) },
        Ok( positions ) => { positions },
    };

    let target_position = match positions.into_iter().find(|x| x.position_id == repay_propagation.clone().position_id) {
        Some(position) => position,
        None => return Err(ContractError::NonExistentPosition {  }) 
    };

    //Fetch position info to repay for 
    let repay_propagation = REPAY.load(deps.storage)?;

   //Position repayment
    let res = match repay(deps.storage, deps.querier, deps.api, env.clone(), info.clone(), repay_propagation.clone().basket_id, repay_propagation.clone().position_id, Some(repay_propagation.clone().position_owner.to_string()), credit_asset.clone() ){
        Ok( res ) => { res },
        Err( e ) => { return Err( e )  }
    };

    
    let cAsset_ratios = get_cAsset_ratios(deps.storage, env.clone(), deps.querier, target_position.clone().collateral_assets, config.clone())?;
    let (avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices) = get_avg_LTV(deps.storage, env.clone(), deps.querier, target_position.clone().collateral_assets, config.clone())?;

    let repay_value = decimal_multiplication(Decimal::from_ratio(credit_asset.amount, Uint128::new(1u128)), basket.credit_price.unwrap());

    let mut messages = vec![];
    let mut coins: Vec<Coin> = vec![];
    let mut native_repayment = Uint128::zero();
    
    
    //Stability Pool receives pro rata assets

    //Add distribute messages to the message builder, so the contract knows what to do with the received funds 
    let mut distribution_assets = vec![];

    //Query SP liq fee
    let resp: PoolResponse = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().stability_pool.unwrap().to_string(),
        msg: to_binary(&SP_QueryMsg::AssetPool {
            asset_info: basket.clone().credit_asset.info,
        })?,
    }))?;
    let sp_liq_fee = resp.liq_premium;

    //Calculate distribution of assets to send from the repaid position    
    for (num, cAsset) in target_position.clone().collateral_assets.into_iter().enumerate(){
        //Builds msgs to the sender (liq contract)

        let collateral_value = decimal_multiplication(repay_value, cAsset_ratios[num]);
        let collateral_amount = decimal_division(collateral_value, cAsset_prices[num]);
        let collateral_w_fee = (decimal_multiplication(collateral_amount, sp_liq_fee) + collateral_amount) * Uint128::new(1u128);

        let repay_amount_per_asset = credit_asset.amount * cAsset_ratios[num];
        
        //Remove collateral from user's position claims
        update_position_claims(
            deps.storage, 
            repay_propagation.clone().basket_id,
            repay_propagation.clone().position_id, 
            repay_propagation.clone().position_owner, 
            cAsset.clone().asset.info, 
            collateral_w_fee)?;

        //SP Distribution needs list of cAsset's and is pulling the amount from the Asset object                
        match cAsset.clone().asset.info {

            AssetInfo::Token { address } => {

                //DistributionMsg builder
                //Only adding the 1 cAsset for the CW20Msg
                let distribution_msg = SP_Cw20HookMsg::Distribute { 
                        distribution_assets: vec![ Asset {
                                amount: collateral_w_fee,
                                ..cAsset.clone().asset
                            }],
                        distribution_asset_ratios: vec![],
                        credit_asset: credit_asset.clone().info, 
                        distribute_for: repay_amount_per_asset,
                    };
                
                //CW20 Send                         
                let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.to_string(),
                    msg: to_binary(&Cw20ExecuteMsg::Send {
                        amount: collateral_w_fee,
                        contract: info.clone().sender.to_string(),
                        msg: to_binary(&distribution_msg)?,
                    })?,
                    funds: vec![],
                });
                messages.push(msg);
            
            }
            AssetInfo::NativeToken { denom: _ } => {

                //Adding each native token to the list of distribution assets
                let asset = Asset{ 
                    amount: collateral_w_fee ,
                    ..cAsset.clone().asset
                };
                //Add to the distribution_for field for native sends
                native_repayment += repay_amount_per_asset;
                
                distribution_assets.push( asset.clone() );
                coins.push(asset_to_coin(asset)?);
                
            },
        }
    }
    
    //Adds Native token distribution msg to messages
    let distribution_msg = SP_ExecuteMsg::Distribute { 
        distribution_assets, 
        distribution_asset_ratios: cAsset_ratios, //The distributions are based off cAsset_ratios so they shouldn't change
        credit_asset: credit_asset.info,
        distribute_for: native_repayment,
    };
    //Build the Execute msg w/ the full list of native tokens
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.clone().stability_pool.unwrap().to_string(),
        msg: to_binary(&distribution_msg)?,
        funds: coins,
    });
    
    messages.push(msg);   

    Ok( res.add_messages(messages) )
}

pub fn increase_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    basket_id: Uint128,
    position_id: Uint128,
    amount: Uint128,
) ->Result<Response, ContractError>{

    let config: Config = CONFIG.load(deps.storage)?;

    let basket: Basket= match BASKETS.load(deps.storage, basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };
    let positions: Vec<Position> = match POSITIONS.load(deps.storage, (basket_id.to_string(), info.sender.clone())){
        Err(_) => {  return Err(ContractError::NoUserPositions {  }) },
        Ok( positions ) => { positions },
    };

    //Filter position by id
    let target_position = match positions.into_iter().find(|x| x.position_id == position_id) {
        Some(position) => position,
        None => return Err(ContractError::NonExistentPosition {  }) 
    };
    let decimal_amount: Decimal = Decimal::from_ratio(amount, Uint128::new(1u128));
    let total_credit = target_position.credit_amount + decimal_amount;

    //Test for minimum debt requirements
    if decimal_multiplication( total_credit, basket.credit_price.unwrap() ) < config.debt_minimum{
        return Err( ContractError::BelowMinimumDebt { })
    }
    
    let message: CosmosMsg;

    //Can't take credit before there is a preset repayment price
    if basket.credit_price.is_some(){
        
        //If resulting LTV makes the position insolvent, error. If not construct mint msg
        //credit_value / asset_value > avg_LTV
                
        if insolvency_check( deps.storage, env.clone(), deps.querier, target_position.clone().collateral_assets, total_credit, basket.credit_price.unwrap(), true, config.clone())?.0 { 
            return Err(ContractError::PositionInsolvent {  })
        }else{
            
            
            message = credit_mint_msg(config.clone(), basket.credit_asset, info.sender.clone())?;
            
            //Add credit amount to the position
            POSITIONS.update(deps.storage, (basket_id.to_string(), info.sender.clone()), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError>{

                match positions {
                    
                    //Find the open positions from the info.sender() in this basket
                    Some(position_list) => 

                        //Find the position we are updating
                        match position_list.clone().into_iter().find(|x| x.position_id == position_id.clone()) {

                            Some(position) => {

                                let mut updated_positions: Vec<Position> = position_list
                                .into_iter()
                                .filter(|x| x.position_id != position_id)
                                .collect::<Vec<Position>>();
                                
                                updated_positions.push(
                                    Position{
                                        credit_amount: total_credit,
                                        ..position
                                });
                                Ok( updated_positions )
                            },
                            None => return Err(ContractError::NonExistentPosition {  }) 
                    },

                    None => return Err(ContractError::NoUserPositions {  })
            }})?;

            //Add new debt to debt-per-asset tallies
            update_basket_debt( deps.storage, env, deps.querier, config, basket_id, target_position.collateral_assets, amount, true )?;
            }
            
        }else{
            return Err(ContractError::NoRepaymentPrice {  })
        }
        

    let response = Response::new()
    .add_message(message)
    .add_attribute("method", "increase_debt")
    .add_attribute("basket_id", basket_id.to_string())
    .add_attribute("position_id", position_id.to_string())
    .add_attribute("total_loan", total_credit.to_string());     

    Ok(response)
            
}


//Confirms insolvency and calculates repayment amount
//Then sends liquidation messages to the modules if they have funds
//If not, sell wall
pub fn liquidate(
    storage: &mut dyn Storage,
    api: &dyn Api,
    querier: QuerierWrapper,
    env: Env,
    info: MessageInfo,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError>{
    
    //TODO: Add batch liquidations so we don't need to do minimum fee bonuses for small accounts

    let config: Config = CONFIG.load(storage)?;

    let basket: Basket= match BASKETS.load(storage, basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };
    let valid_position_owner = validate_position_owner(api, info.clone(), Some(position_owner.clone()))?;

    let target_position = get_target_position( storage, basket_id, valid_position_owner.clone(), position_id )?;

    //Check position health comparative to max_LTV
    let (insolvent, current_LTV, _available_fee) = insolvency_check( storage, env.clone(), querier, target_position.clone().collateral_assets, target_position.clone().credit_amount, basket.credit_price.unwrap(), false, config.clone())?;
    //TODO: Delete
    let insolvent = true;
    let current_LTV = Decimal::percent(90);

    if !insolvent{  return Err(ContractError::PositionSolvent { }) } 
    
    
    //Send liquidation amounts and info to the modules
    //1) We need to calculate how much needs to be liquidated (down to max_borrow_LTV): 
    
    let (avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices) = get_avg_LTV( storage, env.clone(), querier, target_position.clone().collateral_assets, config.clone())?;
    
    
    // max_borrow_LTV/ current_LTV, * current_loan_value, current_loan_value - __ = value of loan amount  
    let loan_value = decimal_multiplication(basket.credit_price.unwrap(), target_position.clone().credit_amount);
    
    //repay value = the % of the loan insolvent. Insolvent is anything between current and max borrow LTV.
    //IE, repay what to get the position down to borrow LTV
    let mut repay_value = loan_value - decimal_multiplication(decimal_division(avg_borrow_LTV, current_LTV), loan_value);

    ///Assert repay_value is above the minimum, if not repay at least the minimum
    /// Repay the full loan if the resulting is going to be less than the minimum.
    if repay_value < config.debt_minimum{
        //If setting the repay value to the minimum leaves at least the minimum in the position...
        //..then partially liquidate
        if loan_value - config.debt_minimum >= config.debt_minimum{
            repay_value = config.debt_minimum;
        }else{ //Else liquidate it all
            repay_value = loan_value;
        }
    }

    let credit_repay_amount = match decimal_division(repay_value, basket.clone().credit_price.unwrap()){
        
        //Repay amount has to be above 0, or there is nothing to liquidate and there was a mistake prior
        x if x <= Decimal::new(Uint128::zero()) => {
            return Err(ContractError::PositionSolvent {  })
        },
        //No need to repay more than the debt
        x if x > target_position.clone().credit_amount => {
            return Err(ContractError::FaultyCalc { })
        }
        x => { x }
    };
    
    
     
    // Don't send any funds here, only send user_ids and repayment amounts.
    // We want to act on the reply status but since that means our state won't revert, assets we send won't come back.
     
    let mut res = Response::new();
    let mut submessages = vec![];
    let mut fee_messages: Vec<CosmosMsg> = vec![];
    
    let cAsset_ratios = get_cAsset_ratios( storage, env.clone(), querier, target_position.clone().collateral_assets, config.clone())?;

    //Dynamic fee that goes to the caller (info.sender): current_LTV - max_LTV
    let caller_fee = decimal_subtraction(current_LTV, avg_max_LTV);

    let total_fees = caller_fee + config.clone().liq_fee;
    
    //Track total leftover repayment after the liq_queue
    let mut liq_queue_leftover_credit_repayment: Decimal = credit_repay_amount;


    let mut total_credit_repaid: Uint256 = Uint256::zero();
    let mut leftover_position_value = total_value;
    let mut leftover_repayment = Decimal::zero();
    let mut sell_wall_repayment_amount = Decimal::zero();

    //1) Calcs repay amount per asset
    //2) Calcs collateral amount to be liquidated per asset (Fees not included yet)
    //2 will happen again in the reply. This instance is to pay the function caller
    for (num, cAsset) in  target_position.clone().collateral_assets.iter().enumerate(){

        let mut caller_coins: Vec<Coin> = vec![];
        let mut protocol_coins: Vec<Coin> = vec![];
        
        let repay_amount_per_asset = decimal_multiplication(credit_repay_amount, cAsset_ratios[num]);
        
        let collateral_price = cAsset_prices[num];
        let collateral_value = decimal_multiplication(repay_value, cAsset_ratios[num]);
        let mut collateral_amount = decimal_division(collateral_value, collateral_price);
       
        
        //Subtract caller fee from Position's claims
        let caller_fee_in_collateral_amount = decimal_multiplication(collateral_amount, caller_fee) * Uint128::new(1u128);
        update_position_claims(storage, basket_id, position_id, valid_position_owner.clone(),  cAsset.clone().asset.info, caller_fee_in_collateral_amount)?;

        //Subtract Protocol fee from Position's claims
        let protocol_fee_in_collateral_amount = decimal_multiplication(collateral_amount, config.clone().liq_fee) * Uint128::new(1u128);
        update_position_claims(storage, basket_id, position_id, valid_position_owner.clone(),  cAsset.clone().asset.info, protocol_fee_in_collateral_amount)?;

        //panic!("{}, {}", caller_fee_in_collateral_amount, protocol_fee_in_collateral_amount );

        //Subtract fees from leftover_position value
        //fee_value = total_fee_collateral_amount * collateral_price
        let fee_value = decimal_multiplication( Decimal::from_ratio( caller_fee_in_collateral_amount + protocol_fee_in_collateral_amount,Uint128::new(1u128)), collateral_price );
        leftover_position_value = decimal_subtraction( leftover_position_value, fee_value );

        //Create msgs to caller as well as to liq_queue if Some()
        match cAsset.clone().asset.info {
            AssetInfo::Token { address } => {
                
                //Send caller Fee
                let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.to_string(),
                    msg: to_binary(&Cw20ExecuteMsg::Transfer {
                        amount: caller_fee_in_collateral_amount,
                        recipient: info.clone().sender.to_string(),
                    })?,
                    funds: vec![],
                });
                fee_messages.push( msg ); 
                
                
                //Send Protocol Fee
                let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.to_string(),
                    msg: to_binary(&Cw20ExecuteMsg::Transfer {
                        amount: protocol_fee_in_collateral_amount,
                        recipient: config.clone().fee_collector.unwrap().to_string(),
                    })?,
                    funds: vec![],
                });
                fee_messages.push( msg ); 
            }
                       
            
            AssetInfo::NativeToken { denom: _ } => {

                let asset = Asset{ 
                    amount: caller_fee_in_collateral_amount,
                    ..cAsset.clone().asset
                };
    
                caller_coins.push(asset_to_coin(asset)?);


                let asset = Asset{ 
                    amount: protocol_fee_in_collateral_amount,
                    ..cAsset.clone().asset
                };
    
                protocol_coins.push(asset_to_coin(asset)?);
                
                
            },
        }
        //Create Msg to send all native token liq fees for fn caller
        let msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: info.clone().sender.to_string(),
            amount: caller_coins,
        });
        fee_messages.push( msg );

        //Create Msg to send all native token liq fees for protocol
        let msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: config.clone().fee_collector.unwrap().to_string(),
            amount: protocol_coins,
        });
        fee_messages.push( msg );

                
        //Set collateral_amount to the amount minus the fees
        //collateral_amount = decimal_subtraction(  collateral_amount, Decimal::from_ratio( (caller_fee_in_collateral_amount + protocol_fee_in_collateral_amount), Uint128::new(1u128) ) );

        
         /////////////LiqQueue calls//////
        if basket.clone().liq_queue.is_some(){

            let res: LQ_LiquidatibleResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: basket.clone().liq_queue.unwrap().to_string(),
                msg: to_binary(
                    &LQ_QueryMsg::CheckLiquidatible {
                        bid_for: cAsset.clone().asset.info,
                        collateral_price,
                        collateral_amount: Uint256::from( (collateral_amount * Uint128::new(1u128)).u128() ),
                        credit_info: basket.clone().credit_asset.info,
                        credit_price: basket.clone().credit_price.unwrap(),
                })?,
            }))?;

            //Calculate how much collateral we are sending to the liq_queue to liquidate
            let leftover: Uint128 = Uint128::from_str( &res.leftover_collateral )?;
            let queue_asset_amount_paid: Uint128 = (collateral_amount * Uint128::new(1u128)) - leftover;
            
            //Keep track of remaining position value
            //value_paid_to_queue = queue_asset_amount_paid * collateral_price
            let value_paid_to_queue: Decimal = decimal_multiplication( Decimal::from_ratio( queue_asset_amount_paid, Uint128::new(1u128)), collateral_price );
            leftover_position_value = decimal_subtraction( leftover_position_value, value_paid_to_queue );


            //Calculate how much the queue repaid in credit
            let queue_credit_repaid = Uint128::from_str( &res.total_credit_repaid )?;
            liq_queue_leftover_credit_repayment = decimal_subtraction(liq_queue_leftover_credit_repayment, Decimal::from_ratio(queue_credit_repaid, Uint128::new(1u128)));
            
            
            //Call Liq Queue::Liquidate for the asset 
            let liq_msg = 
                LQ_ExecuteMsg::Liquidate {
                    credit_price: basket.credit_price.unwrap(),
                    collateral_price,
                    collateral_amount: Uint256::from( queue_asset_amount_paid.u128() ),
                    bid_for: cAsset.clone().asset.info,
                    bid_with: basket.clone().credit_asset.info,
                    basket_id,
                    position_id,
                    position_owner: position_owner.clone(),
                };
            

            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: basket.clone().liq_queue.unwrap().to_string(),
                msg: to_binary(&liq_msg)?,
                funds: vec![],
            });
            
            //Convert to submsg
            let sub_msg: SubMsg = SubMsg::reply_always(msg, LIQ_QUEUE_REPLY_ID);

            submessages.push( sub_msg );


        }
    }



    //If this is some that means the module is in use.
    //Build SubMsgs to send to the Stability Pool
    if config.clone().stability_pool.is_some(){ 

        let sp_liq_fee = query_stability_pool_fee( querier, config.clone(), basket.clone() )?;

        //If LTV is 90% and the fees are 10%, the position would pay everything to pay the liquidators. 
        //So above that, the liquidators are losing the premium guarantee.
        // !( leftover_position_value >= repay_value * fees)
        
        if !( leftover_position_value >= decimal_multiplication( repay_value, (Decimal::one() + sp_liq_fee + total_fees ) )){
            
            sell_wall_repayment_amount = liq_queue_leftover_credit_repayment;

            //Go straight to sell wall
            let ( sell_wall_msgs, collateral_distributions ) = sell_wall( 
                storage, 
                target_position.clone().collateral_assets, 
                cAsset_ratios.clone(), 
                sell_wall_repayment_amount, 
                basket.clone().credit_asset.info,
                basket_id,
                position_id,
                position_owner.clone(),
                )?;
    
            submessages.extend( sell_wall_msgs.
                into_iter()
                .map(|msg| {
                    //If this succeeds, we update the positions collateral claims
                    //If this fails, do nothing. Try again isn't a useful alternative.
                    SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
                }).collect::<Vec<SubMsg>>() );

            //Leftover's starts as the total LQ is supposed to pay, and is subtracted by every successful LQ reply
            let liq_queue_leftovers = decimal_subtraction(credit_repay_amount, liq_queue_leftover_credit_repayment);

             // Set repay values for reply msg
             let repay_propagation = RepayPropagation {
                liq_queue_leftovers, 
                stability_pool: Decimal::zero(),
                sell_wall_distributions: vec![ SellWallDistribution {distributions: collateral_distributions} ],
                basket_id,
                position_id,
                position_owner: valid_position_owner.clone(),
                positions_contract: env.clone().contract.address,
            };

            REPAY.save(storage, &repay_propagation)?;
            
        }else{
            
            //Check for stability pool funds before any liquidation attempts
            //If no funds, go directly to the sell wall
            let leftover_repayment = 
                        query_stability_pool_liquidatible(
                            querier, 
                            config.clone(), 
                            liq_queue_leftover_credit_repayment,
                             basket.clone().credit_asset.info
                        )?;

            let mut collateral_distributions = vec![];
            if leftover_repayment > Decimal::zero(){

                sell_wall_repayment_amount = leftover_repayment;

               //Sell wall remaining
               let ( sell_wall_msgs, distributions ) = sell_wall( 
                storage, 
                target_position.clone().collateral_assets, 
                cAsset_ratios.clone(), 
                sell_wall_repayment_amount, 
                basket.clone().credit_asset.info ,
                basket_id,
                position_id,
                position_owner.clone(),
                )?;
                collateral_distributions = distributions;
    
            submessages.extend( sell_wall_msgs.
                into_iter()
                .map(|msg| {
                    //If this succeeds, we update the positions collateral claims
                    //If this fails, do nothing. Try again isn't a useful alternative.
                    SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
                }).collect::<Vec<SubMsg>>() );

            }

            //Set Stability Pool repay_amount 
            let sp_repay_amount = liq_queue_leftover_credit_repayment - leftover_repayment;
            
            //Leftover's starts as the total LQ is supposed to pay, and is subtracted by every successful LQ reply
            let liq_queue_leftovers = decimal_subtraction(credit_repay_amount, liq_queue_leftover_credit_repayment);
            
            // Set repay values for reply msg
            let repay_propagation = RepayPropagation {
                liq_queue_leftovers, 
                stability_pool: sp_repay_amount,
                sell_wall_distributions: vec![ SellWallDistribution {distributions: collateral_distributions} ],
                basket_id,
                position_id,
                position_owner: valid_position_owner.clone(),
                positions_contract: env.clone().contract.address,
            };

            REPAY.save(storage, &repay_propagation)?;

            ///////////////////

            
            //Stability Pool message builder
            let liq_msg = SP_ExecuteMsg::Liquidate {
                credit_asset: LiqAsset{
                    amount: sp_repay_amount,
                    info: basket.clone().credit_asset.info,
                },
            };

            
            let msg: CosmosMsg =  CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.clone().stability_pool.unwrap().to_string(),
                msg: to_binary(&liq_msg)?,
                funds: vec![],
            });

            let sub_msg: SubMsg = SubMsg::reply_always(msg, STABILITY_POOL_REPLY_ID);

            submessages.push( sub_msg );
        
            //Because these are reply always, we can NOT make state changes that we wouldn't allow no matter the tx result, as our altereed state will NOT revert.
            //Errors also won't revert the whole transaction
            //( https://github.com/CosmWasm/cosmwasm/blob/main/SEMANTICS.md#submessages )
            
        
        //Collateral distributions get handled in the reply

        //Set and subtract the value of what was paid to the Stability Pool
        //(sp_repay_amount * credit_price) * (1+sp_liq_fee)
        let paid_to_sp = decimal_multiplication( decimal_multiplication( sp_repay_amount, basket.credit_price.unwrap() ), (Decimal::one() + sp_liq_fee));
        leftover_position_value = decimal_subtraction( leftover_position_value, paid_to_sp );
        
        }
    }

    //Add the Bad debt callback message as the last SubMsg
    let msg = CosmosMsg::Wasm(
            WasmMsg::Execute {
                 contract_addr: env.contract.address.to_string(), 
                 msg: to_binary(&ExecuteMsg::Callback(
                        CallbackMsg::BadDebtCheck{
                            basket_id,
                            position_id,
                            position_owner: valid_position_owner.clone(),
                        }
                 ))?, 
                 funds: vec![] 
                }
    );
    //Not replying for this, the logic needed will be handled in the callback
    //Replying on Error is just so an Error doesn't cancel transaction
    let call_back = SubMsg::reply_on_error( msg, BAD_DEBT_REPLY_ID );


    //If the SP hasn't repaid everything the liq_queue hasn't AND the value of the position is <= the value that needs to be repaid...
    //..sell wall everything from the start, don't go through either module. 
    //If we don't we are guaranteeing increased bad debt by selling collateral for a discount.
    if !( leftover_repayment ).is_zero() && leftover_position_value <= repay_value{


        //Sell wall credit_repay_amount
        //The other submessages were for the LQ and SP so we reassign the submessage variable
        let ( sell_wall_msgs, collateral_distributions ) = sell_wall( 
            storage, 
            target_position.clone().collateral_assets, 
            cAsset_ratios.clone(), 
            credit_repay_amount, 
            basket.clone().credit_asset.info,
            basket_id,
            position_id,
            position_owner.clone(),
        )?;

        let submessages = sell_wall_msgs.
            into_iter()
            .map(|msg| {
                //If this succeeds, we update the positions collateral claims
                //If this fails, do nothing. Try again isn't a useful alternative.
                SubMsg::reply_on_success(msg, SELL_WALL_REPLY_ID)
            }).collect::<Vec<SubMsg>>();

             // Set repay values for reply msg
        let repay_propagation = RepayPropagation {
            liq_queue_leftovers: Decimal::zero(), 
            stability_pool: Decimal::zero(),
            sell_wall_distributions: vec![ SellWallDistribution {distributions: collateral_distributions} ],
            basket_id,
            position_id,
            position_owner: valid_position_owner.clone(),
            positions_contract: env.clone().contract.address,
        };

        REPAY.save(storage, &repay_propagation)?;

        Ok (       
            res.add_messages( fee_messages )
            .add_submessages(submessages)
            .add_submessage( call_back )
        )

    }else{

        Ok( res
            .add_messages( fee_messages )
            .add_submessages( submessages )
            .add_submessage( call_back )
        )

    }

}


pub fn create_basket(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    collateral_types: Vec<cAsset>,
    credit_asset: Asset,
    credit_price: Option<Decimal>,
    credit_interest: Option<Decimal>,
) -> Result<Response, ContractError>{
    let mut config: Config = CONFIG.load(deps.storage)?;

    let valid_owner: Addr = validate_position_owner(deps.api, info.clone(), owner)?;


    //Only contract owner can create new baskets. This can be governance.
    if info.sender != config.owner{
        return Err(ContractError::NotContractOwner {})
    }

    //Each cAsset has to initialize amount as 0
    let new_cAssets: Vec<cAsset> = collateral_types
        .into_iter()
        .map(|mut asset| {
            asset.asset.amount = Uint128::zero();

            asset
        })
        .collect::<Vec<cAsset>>();


    let new_basket: Basket = Basket {
        owner: valid_owner.clone(),
        basket_id: config.current_basket_id.clone(),
        current_position_id: Uint128::from(1u128),
        collateral_types: new_cAssets,
        collateral_debt_caps: vec![],
        credit_asset: credit_asset.clone(),
        credit_price,
        credit_interest,
        debt_pool_ids: vec![],
        debt_liquidity_multiplier_for_caps: Decimal::one(),
        liq_queue: None,
    };

    let mut subdenom: String;
    let sub_msg: SubMsg;

    if let AssetInfo::NativeToken { denom } = credit_asset.clone().info {
         //Create credit as native token using a tokenfactory proxy
        sub_msg = create_denom( config.clone(), String::from(denom.clone()), new_basket.basket_id.to_string() )?;

        subdenom = denom;
    }else{
        return Err( ContractError::CustomError { val: "Can't create a basket without creating a native token denom".to_string() } )
    }
   


    BASKETS.update(deps.storage, new_basket.basket_id.to_string(), |basket| -> Result<Basket, ContractError>{
        match basket{
            Some( _basket ) => {
                //This is a new basket so there shouldn't already be one made
                return Err(ContractError::ConfigIDError {  })
            },
            None =>{
                Ok(new_basket)
            }
        }
    })?;

    config.current_basket_id += Uint128::from(1u128);
    CONFIG.save(deps.storage, &config)?;

    //Response Building
    let response = Response::new();

    let price = match credit_price{
        Some(x) => { x.to_string()},
        None => { "None".to_string() },
    };
    
    let interest = match credit_interest{
        Some(x) => { x.to_string()},
        None => { "None".to_string() },
    };


    Ok(response.add_attributes(vec![
        attr("method", "create_basket"),
        attr("basket_id", config.current_basket_id.to_string()),
        attr("position_owner", valid_owner.to_string()),
        attr("credit_asset", credit_asset.to_string() ),
        attr("credit_subdenom", subdenom),
        attr("credit_price", price),
        attr("credit_interest", interest),
    ]).add_submessage(sub_msg))
}

pub fn edit_basket(//Can't edit basket id, current_position_id or credit_asset. Can only add cAssets. Can edit owner. Credit price can only be chaged thru the accrue function, but credit_interest is mutable here.
    deps: DepsMut,
    info: MessageInfo,
    basket_id: Uint128,
    added_cAsset: Option<cAsset>,
    owner: Option<String>,
    credit_interest: Option<Decimal>,
    liq_queue: Option<String>,
    pool_ids: Option<Vec<u64>>,
    liquidity_multiplier: Option<Decimal>,
)->Result<Response, ContractError>{

    let new_owner: Option<Addr>;

    if let Some(owner) = owner {
        new_owner = Some(deps.api.addr_validate(&owner)?);
    }else{ new_owner = None }      

    let mut new_queue: Option<Addr> = None;
    if liq_queue.is_some(){
        new_queue = Some(deps.api.addr_validate(&liq_queue.clone().unwrap())?);
    }

    BASKETS.update(deps.storage, basket_id.to_string(), |basket| -> Result<Basket, ContractError>   {

        match basket{
            Some( mut basket ) => {

                if info.sender != basket.owner{
                    return Err(ContractError::NotBasketOwner {  })
                }else{
                    if added_cAsset.is_some(){
                        basket.collateral_types.push(added_cAsset.clone().unwrap());
                    }
                    if new_owner.is_some(){
                        basket.owner = new_owner.clone().unwrap();
                    }
                    if credit_interest.is_some(){
                        basket.credit_interest = credit_interest.clone();
                    }
                    if liq_queue.is_some(){
                        basket.liq_queue = new_queue.clone();
                    }
                    if pool_ids.is_some(){
                        basket.debt_pool_ids = pool_ids.clone().unwrap();
                    }
                    if liquidity_multiplier.is_some(){
                        basket.debt_liquidity_multiplier_for_caps = liquidity_multiplier.clone().unwrap();
                    }
                }

                Ok( basket )
            },
            None => return Err(ContractError::NonExistentBasket { })
        }
    })?;

let res = Response::new();
let mut attrs = vec![];

if added_cAsset.is_some(){
    attrs.push(("asset", added_cAsset.unwrap().asset.info.to_string()));
}
if new_owner.is_some(){
    attrs.push(("owner", new_owner.unwrap().to_string()));
}
if credit_interest.is_some(){
    attrs.push(("credit_interest rate", credit_interest.unwrap().to_string()));
}
if liq_queue.is_some(){
    attrs.push(("liq_queue", liq_queue.unwrap()));
}

Ok(res.add_attributes(attrs))

}

pub fn edit_contract_owner(
    deps: DepsMut,
    info: MessageInfo,
    owner: String,
)-> Result<Response, ContractError>{
    if info.sender.to_string() == owner{

        let valid_owner: Addr = deps.api.addr_validate(&owner)?;
        let mut config: Config = CONFIG.load(deps.storage)?;
        
        config.owner = valid_owner;

        CONFIG.save(deps.storage, &config)?;
    }else{
        return Err(ContractError::NotContractOwner {  })
    }

    let response = Response::new()
    .add_attribute("method","edit_contract_owner")
    .add_attribute("new_owner", owner);

    Ok(response)
}

//create_position = check collateral types, create position object
pub fn create_position(
    deps: &mut dyn Storage,
    cAssets: Vec<cAsset>, //Assets being added into the position
    basket_id: Uint128,
) -> Result<Position, ContractError> {

    let basket: Basket = match BASKETS.load(deps, basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };

    //increment config id
    BASKETS.update(deps, basket_id.to_string(),|basket| -> Result<_, ContractError> {
        match basket{
            Some( mut basket ) => {
                basket.current_position_id += Uint128::from(1u128);
                Ok(basket)
            },
            None => return Err(ContractError::NonExistentBasket {  }), //Due to the first check this should never get hit
        }
        
    })?;

    //Create Position instance
    let new_position: Position;

    new_position = Position {
        position_id: basket.current_position_id,
        collateral_assets: cAssets,
        avg_borrow_LTV: Decimal::zero(),
        avg_max_LTV: Decimal::zero(),
        credit_amount: Decimal::zero(),
        basket_id,
    };   


    return Ok( new_position )
}


pub fn sell_wall_using_ids(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: Addr,
    repay_amount: Decimal,
)-> StdResult<( Vec<CosmosMsg>,Vec<(AssetInfo,Decimal)> )>{
    let config: Config = CONFIG.load(storage)?;
    
    let basket: Basket = BASKETS.load(storage, basket_id.to_string())?;

    let positions: Vec<Position> = POSITIONS.load(storage, (basket_id.to_string(), position_owner.clone()))?;

    let target_position = match positions.into_iter().find(|x| x.position_id == position_id){
        
        Some( position ) => position,
        None => return Err( StdError::NotFound { kind: "Position".to_string() } )
    };

    let cAsset_ratios  = get_cAsset_ratios( storage, env, querier, target_position.clone().collateral_assets, config )?;

    match sell_wall(
        storage, 
        target_position.clone().collateral_assets, 
        cAsset_ratios, 
        repay_amount, 
    basket.clone().credit_asset.info,
        basket_id, 
        position_id,
        position_owner.to_string(),
        ){

        Ok( res ) => Ok( res ),
        Err( err ) => { return Err( StdError::GenericErr { msg: err.to_string() } )}
    }

    
}

pub fn sell_wall(
    storage: &dyn Storage,
    collateral_assets: Vec<cAsset>,
    cAsset_ratios: Vec<Decimal>,
    repay_amount: Decimal,
    credit_info: AssetInfo,
    //For Repay msg
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: String,
)-> Result<( Vec<CosmosMsg>,Vec<(AssetInfo,Decimal)> ), ContractError>{
    
    let config: Config = CONFIG.load(storage)?;

    let mut messages = vec![];
    let mut collateral_distribution = vec![];
    
    for ( index, ratio ) in cAsset_ratios.into_iter().enumerate(){

        let collateral_repay_amount = decimal_multiplication(ratio, repay_amount);
        collateral_distribution.push( ( collateral_assets[index].clone().asset.info, collateral_repay_amount ) );

        //TODO:
        //MAtch credit info and create a different msg for each
        match collateral_assets[index].clone().asset.info{
            AssetInfo::NativeToken { denom } => {

                let router_msg = RouterExecuteMsg::SwapFromNative {
                    to: credit_info.clone(),
                    max_spread: None, //Max spread doesn't matter bc we want to sell the whole amount no matter what
                    recipient: None,
                    hook_msg: Some( 
                        to_binary(
                            &ExecuteMsg::Repay { 
                                basket_id, 
                                position_id, 
                                position_owner: 
                                Some( position_owner.clone() ) })? ),
                    split: None,
                };

                let payment = coin( (collateral_repay_amount*Uint128::new(1u128)).u128(), denom);
        
                let msg: CosmosMsg =  CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.clone().dex_router.unwrap().to_string(),
                    msg: to_binary( &router_msg )?,
                    funds: vec![payment],
                });

                messages.push( msg );
            },
            AssetInfo::Token { address } => {

                //////////////////////////
                let router_hook_msg = RouterHookMsg::Swap { 
                        to: credit_info.clone(),
                        max_spread: None, 
                        recipient: None, 
                        hook_msg: Some( 
                            to_binary(
                                &ExecuteMsg::Repay { 
                                    basket_id, 
                                    position_id, 
                                    position_owner: 
                                    Some( position_owner.clone() ) })? ), 
                        split: None, 
                };

                let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.to_string(),
                    msg: to_binary(&Cw20ExecuteMsg::Send {
                        amount: collateral_repay_amount * Uint128::new(1u128),
                        contract:  config.clone().dex_router.unwrap().to_string(),
                        msg: to_binary(&router_hook_msg)?,
                    })?,
                    funds: vec![],
                });

                messages.push( msg );
            },
        }

    }

    Ok( ( messages, collateral_distribution) ) 
}


pub fn credit_mint_msg(
    config: Config,
    credit_asset: Asset,
    recipient: Addr,
)-> StdResult<CosmosMsg>{


    match credit_asset.clone().info{
        
        AssetInfo::Token { address:_ } =>{
            return Err(StdError::GenericErr { msg: "Credit has to be a native token".to_string() })
        },
        AssetInfo::NativeToken { denom } => {

        if config.osmosis_proxy.is_some(){
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.osmosis_proxy.unwrap().to_string(),
                msg: to_binary(
                        &OsmoExecuteMsg::MintTokens { 
                            denom, 
                            amount: credit_asset.amount, 
                            mint_to_address: recipient.to_string() })?,
                funds: vec![],
            });
            Ok(message)
        }else{
            return Err(StdError::GenericErr { msg: "No proxy contract setup".to_string() })
        }
        },
    }
}

pub fn withdrawal_msg(
    asset: Asset,
    recipient: Addr,
)-> StdResult<CosmosMsg>{
    //let credit_contract: Addr = basket.credit_contract;

    match asset.clone().info{
        AssetInfo::NativeToken { denom: _ } => {
            
            let coin: Coin = asset_to_coin(asset)?;
            let message = CosmosMsg::Bank(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![coin],
            });
            Ok(message)
        },
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
    }
    
}

pub fn asset_to_coin(
    asset: Asset
)-> StdResult<Coin>{

    match asset.info{
        //
        AssetInfo::Token { address: _ } => 
            return Err(StdError::GenericErr { msg: "Only native assets can become Coin objects".to_string() })
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

pub fn assert_credit(credit: Option<Uint128>) -> StdResult<Uint128>{
    //Check if user wants to take credit out now
    let checked_amount = if credit.is_some() &&  !credit.unwrap().is_zero(){
        Uint128::from(credit.unwrap())
     }else{
        Uint128::from(0u128)
    };
    Ok(checked_amount)
}

pub fn get_avg_LTV(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper, 
    collateral_assets: Vec<cAsset>,
    config: Config,
)-> StdResult<(Decimal, Decimal, Decimal, Vec<Decimal>)>{

    let (cAsset_values, cAsset_prices) = get_asset_values(storage, env, querier, collateral_assets.clone(), config)?;

    //panic!("{}", cAsset_values.len());

    let total_value: Decimal = cAsset_values.iter().sum();
    
    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values{
        cAsset_ratios.push( decimal_division( cAsset, total_value) );
    }

    //converting % of value to avg_LTV by multiplying collateral LTV by % of total value
    let mut avg_max_LTV: Decimal = Decimal::zero();
    let mut avg_borrow_LTV: Decimal = Decimal::zero();

    if cAsset_ratios.len() == 0{
        //TODO: Change back to no values. This is for testing without oracles
       //return Ok((Decimal::percent(0), Decimal::percent(0), Decimal::percent(0)))
       return Ok((Decimal::percent(50), Decimal::percent(50), Decimal::percent(100_000_000), vec![Decimal::one()]))
    }

    //Skip unecessary calculations if length is 1
    if cAsset_ratios.len() == 1 { return Ok(( collateral_assets[0].max_borrow_LTV, collateral_assets[0].max_LTV, total_value, cAsset_prices ))}
    
    for (i, _cAsset) in collateral_assets.clone().iter().enumerate(){
        avg_borrow_LTV += decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_borrow_LTV);
    }

    for (i, _cAsset) in collateral_assets.clone().iter().enumerate(){
        avg_max_LTV += decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_LTV);
    }
    

    Ok((avg_borrow_LTV, avg_max_LTV, total_value, cAsset_prices))
}

pub fn get_cAsset_ratios(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    config: Config,
) -> StdResult<Vec<Decimal>>{
    let (cAsset_values, cAsset_prices) = get_asset_values(storage, env, querier, collateral_assets.clone(), config)?;

    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values{
        cAsset_ratios.push(cAsset/total_value) ;
    }

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


pub fn insolvency_check( //Returns true if insolvent, current_LTV and available fee to the caller if insolvent
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    collateral_assets: Vec<cAsset>,
    credit_amount: Decimal,
    credit_price: Decimal,
    max_borrow: bool, //Toggle for either over max_borrow or over max_LTV (liquidatable), ie taking the minimum collateral ratio into account.
    config: Config,
) -> StdResult<(bool, Decimal, Uint128)>{

    //No assets but still has debt
    if collateral_assets.len() == 0 && !credit_amount.is_zero(){
        return Ok( (true, Decimal::percent(100), Uint128::zero()) )
    }
    
    let avg_LTVs: (Decimal, Decimal, Decimal, Vec<Decimal>) = get_avg_LTV(storage, env, querier, collateral_assets, config)?;
    
    //TODO: Change, this is solely for testing. This would liquidate anyone anytime oracles failed.
    //Returns insolvent if oracle's failed
    if avg_LTVs == (Decimal::percent(0), Decimal::percent(50), Decimal::percent(100_000_000), vec![Decimal::one()]){
         return Ok((true, Decimal::percent(90), Uint128::zero())) 
        }

    let asset_values: Decimal = avg_LTVs.2; //pulls total_asset_value
    

    let mut check: bool;
    let current_LTV = decimal_division( decimal_multiplication(credit_amount, credit_price) , asset_values);

    match max_borrow{
        true => { //Checks max_borrow
            check = current_LTV > avg_LTVs.0;
        },
        false => { //Checks max_LTV
            check = current_LTV > avg_LTVs.1;
        },
    }

    let available_fee = if check{
        match max_borrow{
            true => { //Checks max_borrow
                (current_LTV - avg_LTVs.0) * Uint128::new(1)
            },
            false => { //Checks max_LTV
                (current_LTV - avg_LTVs.1) * Uint128::new(1)
            },
        }
    } else {
        Uint128::zero()
    };

    Ok( (check, current_LTV, available_fee) )
}

pub fn assert_basket_assets(
    storage: &mut dyn Storage,
    basket_id: Uint128,
    assets: Vec<Asset>,
    add_to_cAsset: bool,
) -> Result<Vec<cAsset>, ContractError> {
    //let config: Config = CONFIG.load(deps)?;

    let basket: Basket= match BASKETS.load(storage, basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };


    //Checking if Assets for the position are available collateral assets in the basket
    let mut valid = false;
    let mut collateral_assets: Vec<cAsset> = Vec::new();
    
    
    for asset in assets {
       for cAsset in basket.clone().collateral_types{
        match (asset.clone().info, cAsset.asset.info){

            (AssetInfo::Token { address }, AssetInfo::Token { address: cAsset_address }) => {
                if address == cAsset_address {
                    valid = true;
                    collateral_assets.push(cAsset{
                        asset: asset.clone(),
                        ..cAsset
                    });
                 }
            },
            (AssetInfo::NativeToken { denom }, AssetInfo::NativeToken { denom: cAsset_denom }) => {
                if denom == cAsset_denom {
                    valid = true;
                    collateral_assets.push(cAsset{
                        asset: asset.clone(),
                        ..cAsset
                    });
                 }
            },
            (_,_) => continue,
        }}
           
       //Error if invalid collateral, meaning it wasn't found in the list of cAssets
       if !valid {
           return Err(ContractError::InvalidCollateral {  })
        }
        valid = false;
    }

    //Add valid asset amounts to running basket total
    //This is done before deposit() so if that errors this will revert as well
    update_basket_tally( storage, basket_id, collateral_assets.clone(), add_to_cAsset)?;

    Ok(collateral_assets)
}

fn update_basket_tally(
    storage: &mut dyn Storage,
    basket_id: Uint128,
    collateral_assets: Vec<cAsset>,
    add_to_cAsset: bool,
)-> Result<(), ContractError>{

    BASKETS.update(storage, basket_id.to_string(), | basket | -> Result<Basket, ContractError> {
        match basket{

            Some( mut basket ) => {
                
                for cAsset in collateral_assets.iter(){

                    basket.collateral_types = basket.clone().collateral_types
                        .into_iter()
                        .map(| mut asset | {
                            //Add or subtract deposited amount to/from the correlated cAsset object
                            if asset.asset.info.equal(&cAsset.asset.info){
                                if add_to_cAsset {                                 
                                     
                                    asset.asset.amount += cAsset.asset.amount;
                                 }else{

                                    match asset.asset.amount.checked_sub( cAsset.asset.amount ){
                                        Ok( difference ) => {
                                            asset.asset.amount = difference;
                                        },
                                        Err(_) => {
                                            //Don't subtract bc it'll end up being an invalid withdrawal error anyway
                                            //Can't return an Error here without inferring the map return type
                                        }
                                    };
                                 } 
                                 
                             }                            
                            asset
                        }).collect::<Vec<cAsset>>();
                }

                Ok( basket )
            },
            //None should be unreachable 
            None => { return Err( ContractError::NonExistentBasket {  } )},
        }
    })?;

    Ok(())
}

//Validate Recipient
pub fn validate_position_owner(
    deps: &dyn Api, 
    info: MessageInfo, 
    recipient: Option<String>
) -> StdResult<Addr>{

    //let r: Option<String> = String::from(00000owner);
    
    let valid_recipient: Addr = if recipient.is_some(){
        deps.addr_validate(&recipient.unwrap())?
    }else {
        info.sender.clone()
    };

    Ok(valid_recipient)
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

pub fn store_price(
    storage: &mut dyn Storage,
    asset_token: &AssetInfo,
    price: &PriceInfo,
) -> StdResult<()> {
    let mut price_bucket: Bucket<PriceInfo> = Bucket::new(storage, PREFIX_PRICE);
    price_bucket.save( &to_binary(asset_token)?, price)
}

pub fn read_price(
    storage: &dyn Storage,
    asset_token: &AssetInfo,
) -> StdResult<PriceInfo> {
    let price_bucket: ReadonlyBucket<PriceInfo> = ReadonlyBucket::new(storage, PREFIX_PRICE);
    price_bucket.load(&to_binary(asset_token)?)
}

//Get Asset values / query oracle
pub fn get_asset_values(
    storage: &mut dyn Storage, 
    env: Env, 
    querier: QuerierWrapper, 
    assets: Vec<cAsset>, 
    config: Config
) -> StdResult<(Vec<Decimal>, Vec<Decimal>)>
{

   //Getting proportions for position collateral to calculate avg LTV
    //Using the index in the for loop to parse through the assets Vec and collateral_assets Vec
    //, as they are now aligned due to the collateral check w/ the Config's data
    let mut cAsset_values: Vec<Decimal> = vec![];
    let mut cAsset_prices: Vec<Decimal> = vec![];

    if config.clone().osmosis_proxy.is_some(){

        for (i, casset) in assets.iter().enumerate() {

        let price_info: PriceInfo = match read_price( storage, &casset.asset.info ){
            Ok( info ) => { info },
            Err(_) => { 
                //Set time to fail in the next check. We don't want the error to stop from querying though
                PriceInfo {
                    price: Decimal::zero(),
                    last_time_updated: env.block.time.plus_seconds( config.oracle_time_limit + 1u64 ).seconds(),
                } 
            },
        }; 
        let mut valid_price: bool = false;
        let mut price: Decimal;

        //If last_time_updated hasn't hit the limit set by the config...
        //..don't query and use the saved price.
        //Else try to query new price.
        let time_elapsed: Option<u64> = env.block.time.seconds().checked_sub(price_info.last_time_updated);

        //If its None then the subtraction was negative meaning the initial read_price() errored
        if time_elapsed.is_some() && time_elapsed.unwrap() <= config.oracle_time_limit{
            price = price_info.price;
            valid_price = true;
        }else{

            //TODO: REPLACE WITH TWAP WHEN RELEASED PLEEEEASSSE, DONT LET THIS BE OUR DOWNFALL
            price = match querier.query::<SpotPriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                msg: to_binary(&OsmoQueryMsg::SpotPrice {
                    asset: casset.asset.info.to_string(),
                })?,
            })){
                Ok( res ) => { res.price },
                Err( err ) => { 
                    
                    if valid_price{
                        price_info.price
                    }else{
                        return Err( err )
                    }
                }
            };

            store_price(
                storage, 
                &casset.asset.info, 
                &PriceInfo {
                    price,
                    last_time_updated: env.block.time.seconds(),    
                }
            )?;
        }
        
        cAsset_prices.push(price);
        let collateral_value = decimal_multiplication(Decimal::from_ratio(casset.asset.amount, Uint128::new(1u128)), price);
        cAsset_values.push(collateral_value); 
        }
    }
    

    
    Ok((cAsset_values, cAsset_prices))
}




pub fn update_position_claims(
    storage: &mut dyn Storage,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: Addr,
    liquidated_asset: AssetInfo,
    liquidated_amount: Uint128,
)-> StdResult<()>{


    POSITIONS.update(storage, (basket_id.to_string(), position_owner), |old_positions| -> StdResult<Vec<Position>>{
        match old_positions{
            Some( old_positions ) => {

                let new_positions = old_positions
                    .into_iter()
                    .map(|mut position|{
                        //Find position
                        if position.position_id == position_id{
                            //Find asset in position
                            position.collateral_assets = position.collateral_assets
                                .into_iter()
                                .map(|mut c_asset|{
                                    //Subtract amount liquidated from claims
                                    if c_asset.asset.info.equal(&liquidated_asset){
                                        c_asset.asset.amount -= liquidated_amount;
                                    }

                                    c_asset
                                }
                                ).collect::<Vec<cAsset>>();
                        }
                        position  
                    }     
                    ).collect::<Vec<Position>>();

                Ok( new_positions )
            },
            None => { return Err(StdError::GenericErr { msg: "Invalid position owner".to_string() }) }
        }
    })?;
    
    //Subtract liquidated amount from total asset tally
    let collateral_assets = vec![
            cAsset { 
                asset: Asset { info: liquidated_asset, amount: liquidated_amount }, 
                debt_total: Uint128::zero(),
                max_borrow_LTV: Decimal::zero(), 
                max_LTV: Decimal::zero()
            }
    ];
    match update_basket_tally(storage, basket_id, collateral_assets, false){
        Ok( res ) => {},
        Err( err ) => return Err( StdError::GenericErr { msg: err.to_string() } )
    };

    Ok(())
}

 fn get_cAsset_caps(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    //These are Basket specific fields
    credit_info: AssetInfo,
    collateral_assets: Vec<cAsset>, 
    liquidity_multiplier: Decimal,
    pool_ids: Vec<u64>,
 )-> Result<Vec<Uint128>, ContractError>{

    let config: Config = CONFIG.load( storage )?;

    //Get the Basket's asset ratios
    let cAsset_ratios = get_cAsset_ratios(storage, env, querier, collateral_assets, config.clone())?;

    //Get the debt cap 
    let debt_cap = get_asset_liquidity( querier, config, pool_ids, credit_info )? * liquidity_multiplier;

    let mut asset_caps = vec![];

    for cAsset in cAsset_ratios{

        asset_caps.push( cAsset * debt_cap );
    }                       

    //Save these to the basket when returned. For queries.
    Ok( asset_caps )
 }

 pub fn get_asset_liquidity(
    querier: QuerierWrapper,
    config: Config,
    pool_ids: Vec<u64>,
    asset_info: AssetInfo,
 )-> StdResult<Uint128>{

    //Assumption that credit is a native token
    let mut denom = String::from("");
    if let AssetInfo::NativeToken { denom: denomination } = asset_info{
        denom = denomination
    };

    let mut total_pooled = Uint128::zero();

    if config.clone().osmosis_proxy.is_some(){

        for id in pool_ids{

            let res: PoolStateResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                msg: to_binary(&OsmoQueryMsg::PoolState {   
                    id,
                })?,
            }))?;


            let pooled_amount = res.assets
                                .into_iter()
                                .filter(|coin| {
                                coin.denom == denom
                                }).collect::<Vec<Coin>>()
                                [0].amount;

            total_pooled += pooled_amount;

        }

        
    }else{
        return Err( StdError::GenericErr { msg: "No proxy contract setup".to_string() })
    }

   Ok( total_pooled )

 }

 fn update_basket_debt(
     storage: &mut dyn Storage,
     env: Env,
     querier: QuerierWrapper,
     config: Config,
     basket_id: Uint128,
     collateral_assets: Vec<cAsset>,
     credit_amount: Uint128,
     add_to_debt: bool,
 )-> Result<(), ContractError>{

    let basket: Basket = match BASKETS.load( storage, basket_id.to_string()) {
        Err(_) => { return Err(ContractError::NonExistentBasket {  })},
        Ok( basket ) => { basket },
    };

    //Save the debt distribution per asset to a Vec
    let cAsset_ratios = get_cAsset_ratios(storage, env.clone(), querier, collateral_assets.clone(), config)?;

    let mut asset_debt = vec![];

    for asset in cAsset_ratios{
        asset_debt.push( asset*credit_amount );
    }

    let mut over_cap = false;
    let mut assets_over_cap = vec!{};
    //Calculate debt per asset caps
    let cAsset_caps = get_cAsset_caps(
            storage, 
            querier, 
            env, 
            basket.credit_asset.info, 
            collateral_assets.clone(), 
            basket.debt_liquidity_multiplier_for_caps, 
            basket.debt_pool_ids,
        )?;
 
     BASKETS.update(storage, basket_id.to_string(), | basket | -> Result<Basket, ContractError> {
         match basket{
 
             Some( mut basket ) => {
                 
                 for ( index, cAsset ) in collateral_assets.iter().enumerate(){
 
                     basket.collateral_types = basket.clone().collateral_types
                         .into_iter()
                         .map(| mut asset | {
                             //Add or subtract deposited amount to/from the correlated cAsset object
                             if asset.asset.info.equal(&cAsset.asset.info){
                                if add_to_debt {               
                                    
                                    //Assert its not over the cap
                                    if ( asset.debt_total + asset_debt[index] ) < cAsset_caps[index]{
                                        asset.debt_total += asset_debt[index];
                                    }else{
                                        over_cap = true;
                                        assets_over_cap.push( asset.asset.info.to_string() );
                                    }

                                 }else{

                                    match  asset.debt_total.checked_sub( asset_debt[index] ){
                                        Ok( difference ) => {
                                            asset.debt_total = difference;
                                        },
                                        Err(_) => {
                                            //Don't subtract bc it'll end up being an invalid withdrawal error anyway
                                            //Can't return an Error here without inferring the map return type
                                        }
                                    };
                                 } 
                                 
                             }
                             
                             asset
                         }).collect::<Vec<cAsset>>();
                 }
 
                 Ok( basket )
             },
             //None should be unreachable 
             None => { return Err( ContractError::NonExistentBasket {  } )},
         }
     })?;

     //Error if over the asset cap
     if over_cap{
        let mut assets = String::from("");

        assets_over_cap.into_iter().map(|asset| {
                assets += &format!("{} ", asset);
        });

        return Err( ContractError::CustomError { val: format!("This increase of debt sets [ {} ] assets above the protocol debt cap", assets) } )
    }
 
     Ok(())
 }

 fn get_target_position(
    storage: &dyn Storage,
    basket_id: Uint128,
    valid_position_owner: Addr,
    position_id: Uint128,
 )-> Result<Position, ContractError>{

    let positions: Vec<Position> = match POSITIONS.load(storage, (basket_id.to_string(), valid_position_owner.clone())){
        Err(_) => {  return Err(ContractError::NoUserPositions {  }) },
        Ok( positions ) => { positions },
    };

    match positions.into_iter().find(|x| x.position_id == position_id) {
        Some(position) => Ok( position ),
        None => return Err(ContractError::NonExistentPosition {  }) 
    }

 }

 fn create_denom(
    config: Config,
    subdenom: String,
    basket_id: String,
 )-> StdResult<SubMsg>{
    
    if config.osmosis_proxy.is_some(){

        let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.osmosis_proxy.unwrap().to_string(),
            msg: to_binary(
                    &OsmoExecuteMsg::CreateDenom { 
                        subdenom,
                        basket_id,
                     })?,
            funds: vec![],
        });
        
        return Ok( SubMsg::reply_on_success(message, CREATE_DENOM_REPLY_ID));

    }
    return Err( StdError::GenericErr { msg: "No osmosis proxy added to the config yet".to_string() } )

 }


