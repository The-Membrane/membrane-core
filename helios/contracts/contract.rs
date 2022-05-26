#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, StdError, Addr, Uint128, QueryRequest, WasmQuery, Decimal, CosmosMsg, WasmMsg, BankMsg, Coin, from_binary};
use cw2::set_contract_version;
use cw20::{Cw20HookMsg, Cw20ReceiveMsg};
use cw_multi_test::Contract;

use crate::math::{decimal_multiplication, decimal_division};
use crate::error::ContractError;
use crate::msg::{CountResponse, ExecuteMsg, InstantiateMsg, QueryMsg, AssetInfo, Cw20HookMsg, BasketQueryMsg, Asset};
use crate::state::{Config, CONFIG, Position, POSITIONS, cAsset, Basket, BASKETS};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cdp";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
const PRICE_EXPIRE_TIME: u64 = 60;

//TODO: //Add function to update existing cAssets and Baskets and Config

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    
  
        let config = Config {
        owner: info.sender.clone(),
        current_basket_id: Uint128::from(0u128),
    }; 

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Receive(msg) => receive_cw20(deps, env, info, msg),
        ExecuteMsg::Deposit{ assets, position_owner, position_id, basket_id} => {
            for asset in assets{
                assert_sent_native_token_balance(&asset, &info);
            }
            let cAssets: Vec<cAsset> = assert_basket_assets(deps, basket_id, assets)?;
            deposit(deps, info, position_owner, position_id, basket_id, cAssets)
        },
        ExecuteMsg::Withdraw{ position_id, basket_id, assets } => {
            for asset in assets{
                assert_sent_native_token_balance(&asset, &info);
            }
            let cAssets: Vec<cAsset> = assert_basket_assets(deps, basket_id, assets)?;
            withdraw(deps, info, position_id, basket_id, cAssets)
        },
        
        ExecuteMsg::IncreaseDebt { basket_id, position_id, amount } => increase_debt(deps, info, basket_id, position_id, amount),
        ExecuteMsg::Repay { basket_id, position_id, position_owner, credit_asset } => repay(deps, info, basket_id, position_id, position_owner, credit_asset),
        
     

    }
}


//From a receive cw20 hook. Comes from the contract address so easy to validate sent funds. 
//Check if sent funds are equal to amount in msg so we don't have to recheck here
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
        //This only allows 1 cw20 token at a time when opening a position, whereas you can add multiple native assets
        Ok(Cw20HookMsg::Deposit { position_owner, basket_id, position_id}) => {      
            let valid_position_owner: Addr = if let Some(position_owner) = position_owner {
                deps.api.addr_validate(&position_owner)?
            }else {
                cw20_msg.sender.clone()
            };

            let assets: Vec<Asset> = Vec::new();
            assets.push(passed_asset);
            let cAssets: Vec<cAsset> = assert_basket_assets(deps, basket_id, assets)?;

            deposit(deps, info, position_owner, position_id, basket_id, cAssets) 
        },

        Ok(Cw20HookMsg::Repay { basket_id, position_id, position_owner, credit_asset }) => {

            repay(deps, info, basket_id, position_id, position_owner, credit_asset)
        }
        Err(_) => Err(ContractError::Cw20MsgError {}),
    }



}

//create_position = check collateral types, create position object
pub fn create_position(
    deps: DepsMut,
    info: MessageInfo,
    assets: Vec<Asset>, //Assets being added into the position
    basket_id: Uint128,
)
     -> Result<Vec<Position>, ContractError> {

    let config: Config = CONFIG.load(deps.storage)?;
    let basket: Basket = BASKETS.load(deps.storage, basket_id.to_string())?;

    //Assert assets are in the basket
    let collateral_assets: Vec<cAsset> = assert_basket_assets(deps, basket_id, assets)?;

    //Create Position instance
    let new_position: Vec<Position>;

    let avg_LTV: Uint128 = get_avg_LTV(collateral_assets)?;

    new_position = vec![Position {
        position_id: basket.current_position_id,
        collateral_assets,
        avg_LTV: Uint128::from(avg_LTV), //avg_LTV determined by value of collateral
        credit_amount: Uint128::from(0u128),
        basket_id,
    }];

    /*//Add position to existing user positions
    POSITIONS.update(deps.storage, (basket_id.to_string(), valid_position_owner),|mut positions| -> Result<_, ContractError> {

        match positions{
            
            Some(positions) => {
                
                positions.push( new_position );
                Ok(positions)
            },

            None => {
                Ok(vec![ new_position ])
            }
        }
    })?;*/


    //increment config id
    BASKETS.update(deps.storage, basket_id.to_string(),|mut basket| -> Result<_, ContractError> {
        match basket{
            Some( basket ) => {
                basket.current_position_id += Uint128::from(1u128);
                Ok(basket)
            },
            None => Ok( basket.unwrap() ),
        }
        
    })?;


    return Ok( new_position )
}
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
    let new_position_id: Uint128;

    let valid_position_owner = validate_position_owner(deps, info, position_owner)?;

    //Finds the list of positions the positio_owner has in the selected basket
    POSITIONS.update(deps.storage, (basket_id.to_string(), valid_position_owner), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError>{
       
        match positions{
        
        //If Some, adds collateral to the position_id or a new position is created            
        Some(positions) => {

            //If the user wants to create a new/separate position, no position id is passed         
            if position_id.is_some(){

                let position_id = position_id.unwrap();
                let existing_position = positions.iter().find(|x| x.position_id == position_id);

                if existing_position.is_some() {

                    let existing_position = existing_position.unwrap();

                    //Go thru each deposited asset to add quantity to position
                    for deposited_cAsset in cAssets{
                        let deposited_asset = deposited_cAsset.asset;

                        //Add amount if collateral asset exists in the position
                        let found = false;
                        for cAsset in existing_position.collateral_assets{
                            if cAsset.asset.info == deposited_asset.info{
                                cAsset.asset.amount += deposited_asset.amount;
                                found = true;
                            }
                        }

                        //if not, add cAsset to Position
                        let assets: Vec<Asset> = Vec::new();
                        if !found{
                            assets.push(deposited_asset);
                            let new_cAsset = assert_basket_assets(deps, basket_id, assets)?;
                            existing_position.collateral_assets.push( cAsset{
                                asset: deposited_asset, 
                                ..new_cAsset[0]
                            });
                        }
                    }
                    
                    
                    //Recalc avg LTV. Likely unnecessary bc it changes anytime the price does. More necessary in a solvency check.
                    let avg_LTV: Uint128 = get_avg_LTV(existing_position.collateral_assets)?;
                    
                    let filtered_positions: Vec<Position> = positions.into_iter()
                    .filter(|x| x.position_id != position_id)
                    .collect::<Vec<Position>>();

                    //Add altered position to initial Vec
                    //Bc some_position is a reference we need to manually add each parameter
                    filtered_positions.push(
                        Position {
                            avg_LTV,
                            credit_amount: existing_position.credit_amount,
                            collateral_assets: existing_position.collateral_assets,
                            position_id: existing_position.position_id,
                            basket_id,
                        }
                    );
                Ok( filtered_positions )
            }else{
                //If position_ID is passed but no position is found. In case its a mistake, don't want to add a new position.
                return Err(ContractError::NonExistentPosition {  }) 
            }

            }else{
                //If user doesn't pass an ID, we create a new position
                let assets: Vec<Asset> = Vec::new();
                for cAsset in cAssets{
                    assets.push(cAsset.asset);
                }

                let updated_positions: Vec<Position> = create_position(deps, info, assets, basket_id)?;
                
                //For response
                new_position_id = updated_positions[0].position_id.clone();
                
                //Need to add new positions to the old set of positions if a new one was created.
                for position in positions{
                    updated_positions.push(position);
                }

                Ok( updated_positions )
            }
    

        
    },
    // If None, new position is created 
    None => {
        let assets: Vec<Asset> = Vec::new();
        for cAsset in cAssets{
                assets.push(cAsset.asset);
            }

        let updated_positions: Vec<Position> = create_position(deps, info, assets, basket_id)?;

        //For the response
        new_position_id = updated_positions[0].position_id.clone();

        Ok( updated_positions )
    }
       }       

    })?;

    //Response build
    let response = Response::new()
    .add_attribute("method", "deposit")
    .add_attribute("basket_id", basket_id.to_string())
    .add_attribute("position_owner", valid_position_owner.to_string());

    if position_id.is_some(){
        response.add_attribute("position_id", position_id.unwrap().to_string());
    }else{
        response.add_attribute("position_id", new_position_id.to_string());
    }
    
    for cAsset in cAssets{
        response.add_attribute("amount", cAsset.asset.to_string());
    }

    Ok(response)

}

pub fn withdraw(
    deps: DepsMut,
    info: MessageInfo,
    position_id: Uint128,
    basket_id: Uint128,
    cAssets: Vec<cAsset>,
) ->Result<Response, ContractError>{
    //This forces withdrawals to be done by the info.sender 
    //so no need to check if the withdrawal is done by the position owner
    let positions: Vec<Position> = POSITIONS.load(deps.storage, (basket_id.to_string(), info.sender.clone()))?;
    let basket: Basket = BASKETS.load(deps.storage, basket_id.to_string())?;
    
    //Search position by user and then filter by id
    let target_position = match positions.into_iter().find(|x| x.position_id == position_id) {
        Some(position) => position,
        None => return Err(ContractError::NonExistentPosition {  })
    };

    let message: CosmosMsg;
    let response = Response::new();

    //Each cAsset
    for cAsset in cAssets{
        let withdraw_asset = cAsset.asset;

        //If the cAsset is found in the position, attempt withdrawal 
        match target_position.collateral_assets.into_iter().find(|x| x.asset.info == withdraw_asset.info){
            //Some cAsset
            Some( position_collateral ) => {
                //Cant withdraw more than the positions amount
                if withdraw_asset.amount > position_collateral.asset.amount{
                    return Err(ContractError::InvalidWithdrawal {  })
                }else{
                    //Update cAsset data to account for the withdrawal
                    let leftover_amount = position_collateral.asset.amount - withdraw_asset.amount;
                    let new_asset: Asset = Asset {
                        info: position_collateral.asset.info,
                        amount: leftover_amount,
                    };

                    let new_cAsset: cAsset = cAsset{
                        asset: new_asset,
                        ..position_collateral
                    };

                    let updated_cAsset_list: Vec<cAsset> = target_position.collateral_assets
                    .into_iter()
                    .filter(|x| x.asset.info != withdraw_asset.info)
                    .collect::<Vec<cAsset>>();

                    updated_cAsset_list.push(new_cAsset);
                    
                    //Get current max_LTV and asset_values for the position
                    let new_avg_LTV: Uint128 = get_avg_LTV(updated_cAsset_list)?;
                    let asset_values: Decimal = get_asset_values(updated_cAsset_list)?.iter().sum();
                    
                    //If resulting LTV makes the position insolvent, error. If not construct wtihdrawal_msg
                    if basket.repayment_price.is_some(){

                        if decimal_division(Decimal::new(target_position.credit_amount * basket.repayment_price.unwrap()), asset_values) > new_avg_LTV{
                            return Err(ContractError::PositionInsolvent {  })
                        }else{

                            message = withdrawal_msg(withdraw_asset, info.sender.clone())?;

                            POSITIONS.update(deps.storage, (basket_id.to_string(), info.sender.clone()), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError>{

                                match positions {
                                    
                                    //Find the position we are withdrawing from to update
                                    Some(position_list) =>  
                                        match position_list.into_iter().find(|x| x.position_id == position_id) {
                                        Some(position) => {

                                            let updated_positions: Vec<Position> = position_list
                                            .into_iter()
                                            .filter(|x| x.position_id != position_id)
                                            .collect::<Vec<Position>>();

                                            updated_positions.push(
                                                Position{
                                                    avg_LTV: new_avg_LTV,
                                                    collateral_assets: updated_cAsset_list,
                                                    ..position
                                            });
                                            Ok( updated_positions )
                                        },
                                        None => return Err(ContractError::NonExistentPosition {  })
                                    },
                                
                                    None => return Err(ContractError::NonExistentPosition {  }),
                                }
                            })?;
                        }
                    }else{
                        return Err(ContractError::NoRepaymentPrice {  })
                    }

                }
            },
            None => return Err(ContractError::InvalidCollateral {  })
        };
        //This is here in case there are multiple withdrawal messages created.
        response.add_message(message);
    }
    
    response
    .add_attribute("method", "withdraw")
    .add_attribute("basket_id", basket_id.to_string())
    .add_attribute("position_id", position_id.to_string());

    for cAsset in cAssets{
        response.add_attribute("amount", cAsset.asset.to_string());
    }
    Ok(response)
}

pub fn repay(
    deps: DepsMut,
    info: MessageInfo,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: Option<String>,
    credit_asset: Asset,
) ->Result<Response, ContractError>{
    let basket: Basket = BASKETS.load(deps.storage, basket_id.to_string())?;

    let valid_position_owner = validate_position_owner(deps, info, position_owner)?;

    //Assert that the correct credit_asset was sent
    match credit_asset.info {
        AssetInfo::Token { address: submitted_address } => {
            let AssetInfo::Token { address } = basket.credit_asset.info;

            if submitted_address != address || info.sender.clone() != address {
                return Err(ContractError::InvalidCollateral {  })
            }
        },
        AssetInfo::NativeToken { denom: submitted_denom } => { 
            //The to_string() here is just so it compiles, only one of these match arms will be used once the credit_contract type is decided on
            let AssetInfo::NativeToken { denom } = basket.credit_asset.info;
            if submitted_denom != denom {
                return Err(ContractError::InvalidCollateral {  })
            }
            //Assert sent tokens are the same as the Asset parameter
            assert_sent_native_token_balance( &credit_asset, &info )?;
        }
    }
    
    
    
    POSITIONS.update(deps.storage, (basket_id.to_string(), valid_position_owner.clone()), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError>{

        match positions {

            Some(position_list) => {
                //into_iter() gives ownership so it should change in place, tests will confirm
                match position_list.into_iter().find(|x| x.position_id == position_id) {

                    Some(position) => {
                        //Can the amount be repaid?
                        if position.credit_amount >= credit_asset.amount {
                            //Repay amount
                            position.credit_amount -= credit_asset.amount;
                        }else{
                            return Err(ContractError::ExcessRepayment {  })
                        }
                    },
                    None => return Err(ContractError::NonExistentPosition {  })

                }
                
                Ok(position_list)
            },
                        
            None => return Err(ContractError::NonExistentPosition {  }),

            }
    
})?;

    let response = Response::new()
    .add_attribute("method", "repay")
    .add_attribute("basket_id", basket_id.to_string())
    .add_attribute("position_id", position_id.to_string())
    .add_attribute("amount", credit_asset.to_string());
    
    Ok(response)
}

pub fn increase_debt(
    deps: DepsMut,
    info: MessageInfo,
    basket_id: Uint128,
    position_id: Uint128,
    amount: Uint128,
) ->Result<Response, ContractError>{

    let basket: Basket = BASKETS.load(deps.storage, basket_id.to_string())?;
    let positions: Vec<Position> = POSITIONS.load(deps.storage, (basket_id.to_string(), info.sender.clone()))?;

    //Search position by user and then filter by id
    let target_position = match positions.into_iter().find(|x| x.position_id == position_id) {
        Some(position) => position,
        None => return Err(ContractError::NonExistentPosition {  }) 
    };
    
    let total_credit = target_position.credit_amount + amount;
            
    let avg_LTV: Uint128 = get_avg_LTV(target_position.collateral_assets)?;
    let asset_values: Decimal = get_asset_values(target_position.collateral_assets)?.iter().sum();
    
    let message: CosmosMsg;

    //If resulting LTV makes the position insolvent, error. If not construct mint msg
    if basket.repayment_price.is_some(){
        
        //credit_value / asset_value > avg_LTV
        if decimal_division(Decimal::new(target_position.credit_amount * basket.repayment_price.unwrap()), asset_values) > avg_LTV{ 
            return Err(ContractError::PositionInsolvent {  })
        }else{
            
            message = credit_mint_msg(basket.credit_asset, info.sender.clone())?;
            
            //Add credit amount to the position
            POSITIONS.update(deps.storage, (basket_id.to_string(), info.sender.clone()), |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError>{

                match positions {
                    
                    //Find the open positions from the info.sender() in this basket
                    Some(position_list) => 

                        //Find the position we are updating
                        match position_list.into_iter().find(|x| x.position_id == position_id) {

                            Some(position) => {

                                let updated_positions: Vec<Position> = position_list
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

                    None => return Err(ContractError::NonExistentPosition {  })
            }})?;
            }
            
        }else{
            return Err(ContractError::NoRepaymentPrice {  })
        }
        

    let response = Response::new()
    .add_message(message)
    .add_attribute("method", "increase_debt")
    .add_attribute("basket_id", basket_id.to_string())
    .add_attribute("position_id", position_id.to_string())
    .add_attribute("amount", amount.to_string());     

    Ok(response)
            
}

pub fn credit_mint_msg(
    credit_asset: Asset,
    recipient: Addr,
)-> Result<CosmosMsg, ContractError>{

    match credit_asset.info{
        AssetInfo::Token { address } => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: address.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::Mint {
                    recipient: recipient.to_string(),
                    amount: credit_asset.amount,
                })?,
                funds: vec![],
            });
            Ok(message)
        },
        AssetInfo::NativeToken { denom } => {

            //TODO: How to mint native tokens
            Ok(message)
        },
    }
}

pub fn withdrawal_msg(
    asset: Asset,
    recipient: Addr,
)-> Result<CosmosMsg, ContractError>{
    //let credit_contract: Addr = basket.credit_contract;

    match asset.info{
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
        AssetInfo::NativeToken { denom } => {

            let coin: Coin = asset_to_coin(asset)?;
            let message = CosmosMsg::Bank(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![coin],
            });
            Ok(message)
        },
    }
    
    
    
}





#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetCount {} => to_binary(&query_count(deps)?),
    }
}

fn query_count(deps: Deps) -> StdResult<CountResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(CountResponse { count: state.count })
}




pub fn asset_to_coin(
    asset: Asset
)-> Result<Coin, ContractError>{

    match asset.info{
        //
        AssetInfo::Token { address } => Ok(
            return Err(ContractError::InvalidParameters {  })
        ),
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
    collateral_assets: Vec<cAsset>
)-> StdResult<Uint128>{
        let cAsset_values: Vec<Decimal> = get_asset_values(collateral_assets)?;

        let total_value: Decimal = cAsset_values.iter().sum();

        //getting each cAsset's % of total value
        let cAsset_proportions: Vec<Decimal>;
        for cAsset in cAsset_values{
            cAsset_proportions.push(cAsset/total_value) ;
        }

        //converting % of value to avg_LTV by multiplying collateral LTV by % of total value
        let avg_LTV: Uint128 = Uint128::new(0);
        for (i, cAsset) in collateral_assets.iter().enumerate(){
            avg_LTV += cAsset_proportions[i] * collateral_assets[i].collateral_LTV;
        }
    Ok(avg_LTV)
}

pub fn assert_basket_assets(
    deps: DepsMut,
    basket_id: Uint128,
    assets: Vec<Asset>,

) -> Result<Vec<cAsset>, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    let basket: Basket = BASKETS.load(deps.storage, basket_id.to_string())?;


    //Checking if Assets for the position are available collateral assets in the basket
    let valid = false;
    let collateral_assets: Vec<cAsset> = Vec::new();
    
    //TODO: if multi-asset sends arent't possible, will need to change this
    for asset in assets {
       for cAsset in basket.collateral_types{
        match (asset.info, cAsset.asset.info){

            (AssetInfo::Token { address }, AssetInfo::Token { address: cAsset_address }) => {
                if address == cAsset_address {
                    valid = true;
                    collateral_assets.push(cAsset{
                        asset,
                        ..cAsset
                    });
                 }
            },
            (AssetInfo::NativeToken { denom }, AssetInfo::NativeToken { denom: cAsset_denom }) => {
                if denom == cAsset_denom {
                    valid = true;
                    collateral_assets.push(cAsset{
                        asset,
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
    Ok(collateral_assets)
}


//Validate Recipient
pub fn validate_position_owner(
    deps: DepsMut, 
    info: MessageInfo, 
    recipient: Option<String>) -> StdResult<Addr>{
    
    let valid_recipient: Addr = if let Some(recipient) = recipient {
        deps.api.addr_validate(&recipient)?
    }else {
        info.sender.clone()
    };
    Ok(valid_recipient)
}

//Refactored Terraswap function
pub fn assert_sent_native_token_balance(
    asset: &Asset,
    message_info: &MessageInfo)-> StdResult<()> {

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

//Get Asset values / query oracle
pub fn get_asset_values(assets: Vec<cAsset>) -> StdResult<Vec<Decimal>>
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

    for (i,n) in assets.iter().enumerate() {

        //TODO: Query collateral prices from the oracle
       /*let collateral_price = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: collateral_asset.oracle.to_string(),
            msg: to_binary(&OracleQueryMsg::Price {
                asset_token: cAsset.address,
                timeframe,
            })?,
        }))?;*/

        let collateral_value = decimal_multiplication( Decimal::new(assets[i].amount) * collateral_price.rate);
        cAsset_values.push(collateral_value); 

    }
    Ok(cAsset_values)
}






#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies_with_balance, mock_env, mock_info};
    use cosmwasm_std::{coins, from_binary};

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(1000, "earth"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(17, value.count);
    }

    #[test]
    fn increment() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let info = mock_info("anyone", &coins(2, "token"));
        let msg = ExecuteMsg::Increment {};
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // should increase counter by 1
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(18, value.count);
    }

    #[test]
    fn reset() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let unauth_info = mock_info("anyone", &coins(2, "token"));
        let msg = ExecuteMsg::Reset { count: 5 };
        let res = execute(deps.as_mut(), mock_env(), unauth_info, msg);
        match res {
            Err(ContractError::Unauthorized {}) => {}
            _ => panic!("Must return unauthorized error"),
        }

        // only the original creator can reset the counter
        let auth_info = mock_info("creator", &coins(2, "token"));
        let msg = ExecuteMsg::Reset { count: 5 };
        let _res = execute(deps.as_mut(), mock_env(), auth_info, msg).unwrap();

        // should now be 5
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(5, value.count);
    }
}
