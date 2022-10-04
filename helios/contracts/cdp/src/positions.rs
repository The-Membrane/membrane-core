use std::cmp::min;
use std::vec;

use cosmwasm_std::{
    attr, coin, to_binary, Addr, Api, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo,
    QuerierWrapper, QueryRequest, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg,
    WasmQuery,
};
use cosmwasm_storage::{Bucket, ReadonlyBucket};
use cw20::{Cw20ExecuteMsg};
use membrane::oracle::{AssetResponse, PriceResponse};
use osmo_bindings::PoolStateResponse;

use membrane::liq_queue::{
    ExecuteMsg as LQ_ExecuteMsg,
};
use membrane::liquidity_check::{ExecuteMsg as LiquidityExecuteMsg, QueryMsg as LiquidityQueryMsg};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction, Uint256};
use membrane::oracle::{ExecuteMsg as OracleExecuteMsg, QueryMsg as OracleQueryMsg};
use membrane::osmosis_proxy::{
    ExecuteMsg as OsmoExecuteMsg, QueryMsg as OsmoQueryMsg, TokenInfoResponse,
};
use membrane::positions::{ExecuteMsg};
use membrane::stability_pool::{
    Cw20HookMsg as SP_Cw20HookMsg, ExecuteMsg as SP_ExecuteMsg, PoolResponse,
    QueryMsg as SP_QueryMsg,
};
use membrane::types::{
    cAsset, Asset, AssetInfo, AssetOracleInfo, Basket, LiquidityInfo, Position,
    StoredPrice, SupplyCap, TWAPPoolInfo, UserInfo, 
};

use crate::contract::get_contract_balances;
use crate::liquidations::query_stability_pool_fee;
use crate::state::CREDIT_MULTI;
use crate::{
    state::{
        Config, WithdrawPropagation, BASKETS, CONFIG, POSITIONS, REPAY, WITHDRAW,
    },
    ContractError,
};

pub const CREATE_DENOM_REPLY_ID: u64 = 4u64;
pub const WITHDRAW_REPLY_ID: u64 = 5u64;
pub const BAD_DEBT_REPLY_ID: u64 = 999999u64;

pub const SECONDS_PER_YEAR: u64 = 31_536_000u64;

static PREFIX_PRICE: &[u8] = b"price";

//Deposit collateral to existing position. New or same collateral.
//Anyone can deposit, to any position. There will be barriers for withdrawals.
pub fn deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_owner: Option<String>,
    position_id: Option<Uint128>,
    basket_id: Uint128,
    cAssets: Vec<cAsset>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //For Response
    let mut new_position_id: Uint128 = Uint128::new(0u128);

    let valid_owner_addr = validate_position_owner(deps.api, info, position_owner)?;

    let mut basket: Basket = match BASKETS.load(deps.storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    let mut new_position: Position;
    let credit_amount: Uint128;

    //For Withdraw Prop
    let mut old_assets: Vec<cAsset>;
    let mut new_assets = vec![];

    match POSITIONS.load(
        deps.storage,
        (basket_id.to_string(), valid_owner_addr.clone()),
    ) {
        //If Ok, adds collateral to the position_id or a new position is created
        Ok(positions) => {
            //If the user wants to create a new/separate position, no position id is passed
            if position_id.is_some() {
                let pos_id = position_id.unwrap();
                let position = positions
                    .clone()
                    .into_iter()
                    .find(|x| x.position_id == pos_id);

                if position.is_some() {
                    //Set old Assets for debt cap update
                    old_assets = position.clone().unwrap().collateral_assets;
                    //Set credit_amount as well for updates
                    credit_amount = position.clone().unwrap().credit_amount;

                    //Go thru each deposited asset to add quantity to position
                    for deposited_cAsset in cAssets.clone() {
                        let deposited_asset = deposited_cAsset.clone().asset;

                        //Have to reload positions each loop or else the state won't be edited for multiple deposits
                        //We can unwrap and ? safety bc of the layered conditionals
                        let position_s = POSITIONS.load(
                            deps.storage,
                            (basket_id.to_string(), valid_owner_addr.clone()),
                        )?;
                        let existing_position = position_s
                            .clone()
                            .into_iter()
                            .find(|x| x.position_id == pos_id)
                            .unwrap();

                        //Search for cAsset in the position then match
                        let temp_cAsset: Option<cAsset> = existing_position
                            .clone()
                            .collateral_assets
                            .into_iter()
                            .find(|x| x.asset.info.equal(&deposited_asset.clone().info));

                        match temp_cAsset {
                            //If Some, add amount to cAsset in the position
                            Some(cAsset) => {
                                let new_cAsset = cAsset {
                                    asset: Asset {
                                        amount: cAsset.clone().asset.amount
                                            + deposited_asset.clone().amount,
                                        info: cAsset.clone().asset.info,
                                    },
                                    ..cAsset.clone()
                                };

                                let mut temp_list: Vec<cAsset> = existing_position
                                    .clone()
                                    .collateral_assets
                                    .into_iter()
                                    .filter(|x| !x.asset.info.equal(&deposited_asset.clone().info))
                                    .collect::<Vec<cAsset>>();
                                temp_list.push(new_cAsset);

                                let temp_pos = Position {
                                    collateral_assets: temp_list,
                                    ..existing_position.clone()
                                };

                                //Set new_assets for debt cap updates
                                new_assets = temp_pos.clone().collateral_assets;

                                POSITIONS.update(
                                    deps.storage,
                                    (basket_id.to_string(), valid_owner_addr.clone()),
                                    |positions| -> Result<Vec<Position>, ContractError> {
                                        let unwrapped_pos = positions.unwrap();

                                        let mut update = unwrapped_pos
                                            .clone()
                                            .into_iter()
                                            .filter(|x| x.position_id != pos_id)
                                            .collect::<Vec<Position>>();
                                        update.push(temp_pos);

                                        Ok(update)
                                    },
                                )?;
                            }

                            // //if None, add cAsset to Position if in Basket options
                            None => {
                                let new_cAsset = deposited_cAsset.clone();

                                POSITIONS.update(
                                    deps.storage,
                                    (basket_id.to_string(), valid_owner_addr.clone()),
                                    |positions| -> Result<Vec<Position>, ContractError> {
                                        let temp_pos = positions.unwrap();

                                        let position = temp_pos
                                            .clone()
                                            .into_iter()
                                            .find(|x| x.position_id == pos_id);
                                        let mut p = position.clone().unwrap();
                                        p.collateral_assets.push(cAsset {
                                            asset: deposited_asset.clone(),
                                            max_borrow_LTV: new_cAsset.clone().max_borrow_LTV,
                                            max_LTV: new_cAsset.clone().max_LTV,
                                            pool_info: new_cAsset.clone().pool_info,
                                        });

                                        //Set new_assets for debt cap updates
                                        new_assets = p.clone().collateral_assets;
                                        //Add empty asset to old_assets as a placeholder
                                        old_assets.push(cAsset {
                                            asset: Asset {
                                                amount: Uint128::zero(),
                                                ..deposited_asset
                                            },
                                            max_borrow_LTV: new_cAsset.clone().max_borrow_LTV,
                                            max_LTV: new_cAsset.clone().max_LTV,
                                            pool_info: new_cAsset.clone().pool_info,
                                        });

                                        //Add updated position to user positions
                                        let mut update = temp_pos
                                            .clone()
                                            .into_iter()
                                            .filter(|x| x.position_id != pos_id)
                                            .collect::<Vec<Position>>();
                                        update.push(p);

                                        Ok(update)
                                    },
                                )?;
                            }
                        }
                    }
                    //Accrue, mainly for repayment price
                    accrue(
                        deps.storage,
                        deps.querier,
                        env.clone(),
                        &mut position.clone().unwrap(),
                        &mut basket,
                    )?;
                    //Save Basket
                    BASKETS.save(deps.storage, basket_id.clone().to_string(), &basket)?;

                    if !credit_amount.is_zero() {
                        update_debt_per_asset_in_position(
                            deps.storage,
                            env.clone(),
                            deps.querier,
                            config,
                            basket_id,
                            old_assets,
                            new_assets,
                            Decimal::from_ratio(credit_amount, Uint128::new(1u128)),
                        )?;
                    }
                } else {
                    //If position_ID is passed but no position is found, Error. 
                    //In case its a mistake, don't want to add assets to a new position.
                    return Err(ContractError::NonExistentPosition {});
                }
            } else {
                //If user doesn't pass an ID, we create a new position
                new_position =
                    create_position(deps.storage, cAssets.clone(), &mut basket, env.clone())?;

                //Accrue, mainly for repayment price
                accrue(
                    deps.storage,
                    deps.querier,
                    env.clone(),
                    &mut new_position,
                    &mut basket,
                )?;
                //Save Basket. This only doesn't overwrite the save in update_debt_per_asset_in_position() bc they are certain to never happen at the same time
                BASKETS.save(deps.storage, basket_id.clone().to_string(), &basket)?;

                //For response
                new_position_id = new_position.clone().position_id;
                

                //Need to add new position to the old set of positions if a new one was created.
                POSITIONS.update(
                    deps.storage,
                    (basket_id.to_string(), valid_owner_addr.clone()),
                    |positions| -> Result<Vec<Position>, ContractError> {
                        //We can .unwrap() here bc the initial .load() matched Ok()
                        let mut old_positions = positions.unwrap();

                        old_positions.push(new_position);

                        Ok(old_positions)
                    },
                )?;
            }
        }
        // If Err() meaning no positions loaded, new Vec<Position> is created
        Err(_) => {
            new_position = create_position(deps.storage, cAssets.clone(), &mut basket, env.clone())?;

            //Accrue, mainly for repayment price
            accrue(
                deps.storage,
                deps.querier,
                env.clone(),
                &mut new_position,
                &mut basket,
            )?;
            //Save Basket. This only doesn't overwrite the save in update_debt_per_asset_in_position() bc they are certain to never happen at the same time
            BASKETS.save(deps.storage, basket_id.clone().to_string(), &basket)?;

            //For response
            new_position_id = new_position.clone().position_id;

            //Add new Vec of Positions to state under the user
            POSITIONS.save(
                deps.storage,
                (basket_id.to_string(), valid_owner_addr.clone()),
                &vec![new_position],
            )?;
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

    let assets: Vec<String> = cAssets
        .iter()
        .map(|x| x.asset.clone().to_string())
        .collect();

    for i in 0..assets.clone().len() {
        attrs.push(("assets", &assets[i]));
    }

    Ok(response.add_attributes(attrs))
}

pub fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    basket_id: Uint128,
    cAssets: Vec<cAsset>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    let mut basket: Basket = match BASKETS.load(deps.storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    let mut msgs = vec![];
    let response = Response::new();

    //For debt cap updates
    let old_assets =
        get_target_position(deps.storage, basket_id, info.sender.clone(), position_id)?
            .collateral_assets;
    let mut new_assets: Vec<cAsset> = vec![];
    let mut tally_update_list: Vec<cAsset> = vec![];
    let mut credit_amount = Uint128::zero();

    //Set withdrawal prop variables
    let mut prop_assets = vec![];
    let mut reply_order: Vec<usize> = vec![];
    let mut withdraw_assets: Vec<Asset> = vec![];

    //For Withdraw Msg
    let mut withdraw_coins: Vec<Coin> = vec![];

    //Check for expunged assets and assert they are being withdrawn
    check_for_expunged( old_assets.clone(), cAssets.clone(), basket.clone() )?;

    //Each cAsset
    //We reload at every loop to account for edited state data
    //Otherwise users could siphon funds they don't own w/ duplicate cAssets.
    //Could fix the problem at the duplicate assets but I like operating on the most up to date state.
    for cAsset in cAssets.clone() {
        let withdraw_asset = cAsset.asset;

        //This forces withdrawals to be done by the info.sender
        //so no need to check if the withdrawal is done by the position owner
        let mut target_position =
            get_target_position(deps.storage, basket_id, info.sender.clone(), position_id)?;

        //Accrue interest
        accrue(
            deps.storage,
            deps.querier,
            env.clone(),
            &mut target_position,
            &mut basket,
        )?;
        //Save updated position to lock-in credit_amount and last_accrued time
        POSITIONS.update(
            deps.storage,
            (basket_id.to_string(), info.clone().sender),
            |old_positions| -> StdResult<Vec<Position>> {
                match old_positions {
                    Some(old_positions) => {
                        let new_positions = old_positions
                            .into_iter()
                            .map(|stored_position| {
                                //Find position
                                if stored_position.position_id == position_id {
                                    //Swap to target_position 
                                    target_position.clone()
                                } else {
                                    //Save stored_positon
                                    stored_position
                                }
                            })
                            .collect::<Vec<Position>>();

                        Ok(new_positions)
                    },
                    None => {
                        return Err(StdError::GenericErr {
                            msg: "Invalid position owner".to_string(),
                        })
                    }
                }
            },
        )?;

        //Find cAsset in target_position
        match target_position
            .clone()
            .collateral_assets
            .into_iter()
            .find(|x| x.asset.info.equal(&withdraw_asset.info))
        {
            //If the cAsset is found in the position, attempt withdrawal
            Some(position_collateral) => {
                //Cant withdraw more than the positions amount
                if withdraw_asset.amount > position_collateral.asset.amount {
                    return Err(ContractError::InvalidWithdrawal {});
                } else {
                    //Now that its a valid withdrawal and debt has accrued, we can add to tally_update_list
                    //This will be used to keep track of Basket supply caps
                    tally_update_list.push(cAsset {
                        asset: withdraw_asset.clone(),
                        ..position_collateral.clone()
                    });

                    //Withdraw Prop: Push the initial asset
                    prop_assets.push(position_collateral.clone().asset);

                    //Update cAsset data to account for the withdrawal
                    let leftover_amount = position_collateral.asset.amount - withdraw_asset.amount;

                    let mut updated_cAsset_list: Vec<cAsset> = target_position
                        .clone()
                        .collateral_assets
                        .into_iter()
                        .filter(|x| !(x.asset.info.equal(&withdraw_asset.info)))
                        .collect::<Vec<cAsset>>();

                    //Delete asset from the position if the amount is being fully withdrawn. In this case just don't push it
                    if leftover_amount != Uint128::new(0u128) {
                        let new_asset = Asset {
                            amount: leftover_amount,
                            ..position_collateral.clone().asset
                        };

                        let new_cAsset: cAsset = cAsset {
                            asset: new_asset,
                            ..position_collateral.clone()
                        };

                        updated_cAsset_list.push(new_cAsset);
                    }

                    //If resulting LTV makes the position insolvent, error. If not construct withdrawal_msg
                    //This is taking max_borrow_LTV so users can't max borrow and then withdraw to get a higher initial LTV
                    if insolvency_check(
                        deps.storage,
                        env.clone(),
                        deps.querier,
                        basket.clone(),
                        updated_cAsset_list.clone(),
                        Decimal::from_ratio(
                            target_position.clone().credit_amount,
                            Uint128::new(1u128),
                        ),
                        basket.credit_price,
                        true,
                        config.clone(),
                    )?
                    .0
                    {
                        return Err(ContractError::PositionInsolvent {});
                    } else {
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

                                            //For debt cap updates
                                            new_assets = updated_cAsset_list.clone();
                                            credit_amount = position.clone().credit_amount;

                                            updated_positions.push(
                                                Position{
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
                    //Push withdraw asset to list for withdraw prop
                    withdraw_assets.push(withdraw_asset.clone());

                    //Create send msgs
                    match withdraw_asset.clone().info {
                        AssetInfo::Token { address: _ } => {
                            //Create separate withdraw msg
                            let message = withdrawal_msg(withdraw_asset, info.sender.clone())?;
                            msgs.push(SubMsg::reply_on_success(message, WITHDRAW_REPLY_ID));

                            //Signal 1 asset reply
                            reply_order.push(1u64 as usize);
                        }
                        AssetInfo::NativeToken { denom: _ } => {
                            //Push to withdraw_coins
                            withdraw_coins.push(asset_to_coin(withdraw_asset)?);
                        }
                    }
                }
            }
            None => return Err(ContractError::InvalidCollateral {}),
        };
    }

    //Push aggregated native coin withdrawal
    if withdraw_coins != vec![] {
        //Save # of native tokens sent for the withdrawal reply propagation 
        reply_order.push(withdraw_coins.len() as usize);

        let message = CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.clone().to_string(),
            amount: withdraw_coins,
        });
        msgs.push(SubMsg::reply_on_success(message, WITHDRAW_REPLY_ID));
    }

    //Update basket supply cap tallies after all withdrawals to improve UX by smoothing debt_cap restrictions
    update_basket_tally(
        deps.storage,
        deps.querier,
        env.clone(),
        &mut basket,
        tally_update_list,
        false,
    )?;

    //Save updated repayment price and asset tallies
    BASKETS.save(deps.storage, basket_id.to_string(), &basket)?;

    //Update debt distribution for position assets
    if !credit_amount.is_zero() {
        //Make sure lists are equal and add blank assets if not
        if old_assets.len() != new_assets.len() {
            for i in 0..old_assets.len() {
                let mut already_pushed = false;
                if i == new_assets.len() {
                    new_assets.push(cAsset {
                        asset: Asset {
                            info: old_assets[i].clone().asset.info,
                            amount: Uint128::zero(),
                        },
                        ..old_assets[i].clone()
                    });
                    already_pushed = true;
                }
                //If the index isn't equal, push a blank asset (0 amount) beforehand
                if !already_pushed {
                    if !old_assets[i].asset.info.equal(&new_assets[i].asset.info) {
                        let temp_vec = vec![cAsset {
                            asset: Asset {
                                info: old_assets[i].clone().asset.info,
                                amount: Uint128::zero(),
                            },
                            ..old_assets[i].clone()
                        }];

                        let mut left: Vec<cAsset> = vec![];
                        let mut right: Vec<cAsset> = vec![];
                        for (index, asset) in new_assets.into_iter().enumerate() {
                            if index < i {
                                left.push(asset)
                            } else {
                                right.push(asset)
                            }
                        }
                        left.extend(temp_vec);
                        left.extend(right);
                        new_assets = left;
                    }
                }
            }
        }
        //Update debt caps
        update_debt_per_asset_in_position(
            deps.storage,
            env.clone(),
            deps.querier,
            config,
            basket_id,
            old_assets,
            new_assets,
            Decimal::from_ratio(credit_amount, Uint128::new(1u128)),
        )?;
    }

    //Set Withdrawal_Prop
    let prop_assets_info: Vec<AssetInfo> = prop_assets
        .clone()
        .into_iter()
        .map(|asset| asset.info)
        .collect::<Vec<AssetInfo>>();

    let withdraw_amounts: Vec<Uint128> = withdraw_assets
        .clone()
        .into_iter()
        .map(|asset| asset.amount)
        .collect::<Vec<Uint128>>();

    let withdrawal_prop = WithdrawPropagation {
        positions_prev_collateral: prop_assets,
        withdraw_amounts,
        contracts_prev_collateral_amount: get_contract_balances(
            deps.querier,
            env,
            prop_assets_info,
        )?,
        position_info: UserInfo {
            basket_id,
            position_id,
            position_owner: info.clone().sender.to_string(),
        },
        reply_order,
    };
    WITHDRAW.save(deps.storage, &withdrawal_prop)?;

    let mut attrs = vec![];
    attrs.push(("method", "withdraw"));

    //These placeholders are for lifetime warnings
    let b = &basket_id.to_string();
    attrs.push(("basket_id", b));

    let p = &position_id.to_string();
    attrs.push(("position_id", p));

    let temp: Vec<String> = cAssets
        .into_iter()
        .map(|cAsset| cAsset.asset.to_string())
        .collect::<Vec<String>>();

    for i in 0..temp.clone().len() {
        attrs.push(("assets", &temp[i]));
    }

    Ok(response
        .add_attributes(attrs)
        .add_submessages(msgs))
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
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(storage)?;

    let mut basket: Basket = match BASKETS.load(storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    //Validate position owner 
    let valid_owner_addr = validate_position_owner(api, info.clone(), position_owner)?;
    
    //Get target_position
    let mut target_position =
        get_target_position(storage, basket_id, valid_owner_addr.clone(), position_id)?;

    //Accrue interest
    accrue(
        storage,
        querier,
        env.clone(),
        &mut target_position,
        &mut basket,
    )?;

    let response = Response::new();
    let mut burn_msg: Option<CosmosMsg> = None;

    let mut total_loan = Uint128::zero();
    let mut updated_list: Vec<Position> = vec![];

    //Assert that the correct credit_asset was sent
    assert_credit_asset(basket.clone(), credit_asset.clone(), info.clone().sender)?;

    //Attempt position repayment
    POSITIONS.update(
        storage,
        (basket_id.to_string(), valid_owner_addr.clone()),
        |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError> {
            match positions {
                Some(position_list) => {
                    //Find target position in the list of user's positions
                    updated_list = match position_list
                        .clone()
                        .into_iter()
                        .find(|x| x.position_id == position_id.clone())
                    {
                        Some(_position) => {
                            //Can the amount be repaid?
                            if target_position.credit_amount >= credit_asset.amount {
                                //Repay amount
                                target_position.credit_amount -= credit_asset.amount;

                                //Position's resulting debt can't be below minimum without being fully repaid
                                if target_position.credit_amount * basket.clone().credit_price
                                    < config.debt_minimum
                                    && !target_position.credit_amount.is_zero()
                                {
                                    //Router contract is allowed to.
                                    //We rather $1 of bad debt than $2000 and bad debt comes from swap slippage
                                    if let Some(router) = config.clone().dex_router {
                                        if info.sender != router {
                                            return Err(ContractError::BelowMinimumDebt {});
                                        }
                                    }
                                }

                                //Burn repayment
                                burn_msg = Some(credit_burn_msg(
                                    config.clone(),
                                    env.clone(),
                                    credit_asset.clone(),
                                )?);

                                total_loan = target_position.clone().credit_amount;
                            } else {
                                return Err(ContractError::ExcessRepayment {});
                                //We don't want to have to send back assets
                            }

                            //Create replacement Vec<Position> to update w/
                            let mut update: Vec<Position> = position_list
                                .clone()
                                .into_iter()
                                .filter(|x| x.position_id != position_id.clone())
                                .collect::<Vec<Position>>();

                            update.push(Position {
                                credit_amount: total_loan.clone(),
                                ..target_position.clone()
                            });

                            update
                        }
                        None => return Err(ContractError::NonExistentPosition {}),
                    };

                    //Now update w/ the updated_list
                    //The compiler is saying this value is never read so check in tests
                    //Works fine but will leave the warning
                    Ok(updated_list)
                }

                None => return Err(ContractError::NoUserPositions {}),
            }
        },
    )?;

    //Subtract paid debt from debt-per-asset tallies
    update_basket_debt(
        storage,
        env,
        querier,
        config,
        &mut basket,
        target_position.collateral_assets,
        credit_asset.amount,
        false,
        false,
    )?;

    //Save updated repayment price and debts
    BASKETS.save(storage, basket_id.to_string(), &basket)?;

    //This is a safe unwrap bc the code errors if it is uninitialized
    Ok(response.add_message(burn_msg.unwrap()).add_attributes(vec![
        attr("method", "repay".to_string()),
        attr("basket_id", basket_id.to_string()),
        attr("position_id", position_id.to_string()),
        attr("loan_amount", total_loan.to_string()),
    ]))
}

//This is what the stability pool contract will call to repay for a liquidation and get its collateral distribution
//1) Repay
//2) Send position collateral + fee
pub fn liq_repay(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    credit_asset: Asset,
) -> Result<Response, ContractError> {

    let config = CONFIG.load(deps.storage)?;
    
    //Fetch position info to repay for
    let repay_propagation = REPAY.load(deps.storage)?;

    //Can only be called by the SP contract
    if config.clone().stability_pool.is_none()
        || info.sender != config.clone().stability_pool.unwrap()
    {
        return Err(ContractError::Unauthorized {});
    }

    //These 3 checks shouldn't be possible since we are pulling the ids from state.
    //Would have to be an issue w/ the repay_progation initialization
    let basket: Basket = match BASKETS.load(
        deps.storage,
        repay_propagation.clone().basket_id.to_string(),
    ) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    let target_position = get_target_position(
        deps.storage, 
        repay_propagation.clone().basket_id,
        repay_propagation.clone().position_owner,
        repay_propagation.clone().position_id,
    )?;
    
    //Position repayment
    let res = match repay(
        deps.storage,
        deps.querier,
        deps.api,
        env.clone(),
        info.clone(),
        repay_propagation.clone().basket_id,
        repay_propagation.clone().position_id,
        Some(repay_propagation.clone().position_owner.to_string()),
        credit_asset.clone(),
    ) {
        Ok(res) => res,
        Err(e) => return Err(e),
    };

    //Split LP cAssets to their pool assets
    let collateral_assets = get_LP_pool_cAssets(
        deps.querier,
        config.clone(),
        basket.clone(),
        target_position.clone().collateral_assets,
    )?;

    //Get position's cAsset ratios
    let cAsset_ratios = get_cAsset_ratios(
        deps.storage,
        env.clone(),
        deps.querier,
        collateral_assets.clone(),
        config.clone(),
    )?;
    //Get cAsset prices
    let (_avg_borrow_LTV, _avg_max_LTV, _total_value, cAsset_prices) = get_avg_LTV(
        deps.storage,
        env.clone(),
        deps.querier,
        config.clone(),
        basket.clone(),
        collateral_assets.clone(),
    )?;

    let repay_value = decimal_multiplication(
        Decimal::from_ratio(credit_asset.amount, Uint128::new(1u128)),
        basket.credit_price,
    );

    let mut messages = vec![];
    let mut coins: Vec<Coin> = vec![];
    let mut native_repayment = Uint128::zero();

    //Stability Pool receives pro rata assets

    //Add distribute messages to the message builder, so the contract knows what to do with the received funds
    let mut distribution_assets = vec![];

    //Query SP liq fee
    let sp_liq_fee = query_stability_pool_fee(deps.querier, config.clone(), basket.clone())?;

    //Calculate distribution of assets to send from the repaid position
    for (num, cAsset) in collateral_assets.clone().into_iter().enumerate() {
        //Builds msgs to the sender (liq contract)

        let collateral_repay_value = decimal_multiplication(repay_value, cAsset_ratios[num]);
        let collateral_repay_amount = decimal_division(collateral_repay_value, cAsset_prices[num]);
        let collateral_w_fee = (decimal_multiplication(collateral_repay_amount, sp_liq_fee)
            + collateral_repay_amount)
            * Uint128::new(1u128);

        let repay_amount_per_asset = credit_asset.amount * cAsset_ratios[num];

        //Remove collateral from user's position claims
        update_position_claims(
            deps.storage,
            deps.querier,
            env.clone(),
            repay_propagation.clone().basket_id,
            repay_propagation.clone().position_id,
            repay_propagation.clone().position_owner,
            cAsset.clone().asset.info,
            collateral_w_fee,
        )?;

        //SP Distribution needs list of cAsset's and is pulling the amount from the Asset object
        match cAsset.clone().asset.info {
            AssetInfo::Token { address } => {
                //DistributionMsg builder
                //Only adding the 1 cAsset for the CW20Msg
                let distribution_msg = SP_Cw20HookMsg::Distribute {
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
                let asset = Asset {
                    amount: collateral_w_fee,
                    ..cAsset.clone().asset
                };
                //Add to the distribution_for field for native sends
                native_repayment += repay_amount_per_asset;

                distribution_assets.push(asset.clone());
                coins.push(asset_to_coin(asset)?);
            }
        }
    }

    //Adds Native token distribution msg to messages
    let distribution_msg = SP_ExecuteMsg::Distribute {
        distribution_assets: distribution_assets.clone(),
        distribution_asset_ratios: cAsset_ratios, //The distributions are based off cAsset_ratios so they shouldn't change
        credit_asset: credit_asset.info,
        distribute_for: native_repayment.clone(),
    };
    //Build the Execute msg w/ the full list of native tokens
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.clone().stability_pool.unwrap().to_string(),
        msg: to_binary(&distribution_msg)?,
        funds: coins,
    });

    messages.push(msg);

    Ok(res
        .add_messages(messages)
        .add_attribute("method", "liq_repay")
        .add_attribute("distribution_assets", format!("{:?}", distribution_assets))
        .add_attribute("distribute_for", native_repayment.clone()))
}

pub fn increase_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    basket_id: Uint128,
    position_id: Uint128,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    //Load basket
    let mut basket: Basket = match BASKETS.load(deps.storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    //get Target position
    let mut target_position = get_target_position(deps.storage, basket_id, info.clone().sender, position_id)?;

    //Accrue interest
    accrue(
        deps.storage,
        deps.querier,
        env.clone(),
        &mut target_position,
        &mut basket,
    )?;

    target_position.credit_amount += amount;

    //Test for minimum debt requirements
    if decimal_multiplication(
        Decimal::from_ratio(target_position.credit_amount, Uint128::new(1u128)),
        basket.credit_price,
    ) < Decimal::from_ratio(config.debt_minimum, Uint128::new(1u128))
    {
        return Err(ContractError::BelowMinimumDebt {});
    }

    let message: CosmosMsg;

    //Can't take credit before an oracle is set
    if basket.oracle_set {
        //If resulting LTV makes the position insolvent, error. If not construct mint msg
        //credit_value / asset_value > avg_LTV

        if insolvency_check(
            deps.storage,
            env.clone(),
            deps.querier,
            basket.clone(),
            target_position.clone().collateral_assets,
            Decimal::from_ratio(target_position.credit_amount, Uint128::new(1u128)),
            basket.credit_price,
            true,
            config.clone(),
        )?
        .0
        {
            return Err(ContractError::PositionInsolvent {});
        } else {
            message = credit_mint_msg(
                config.clone(),
                basket.clone().credit_asset,
                info.sender.clone(),
            )?;

            //Add credit amount to the position
            POSITIONS.update(
                deps.storage,
                (basket_id.to_string(), info.sender.clone()),
                |positions: Option<Vec<Position>>| -> Result<Vec<Position>, ContractError> {
                    match positions {
                        //Find the open positions from the info.sender() in this basket
                        Some(position_list) =>
                        //Find the position we are updating
                        {
                            match position_list
                                .clone()
                                .into_iter()
                                .find(|x| x.position_id == position_id.clone())
                            {
                                Some(position) => {
                                    let mut updated_positions: Vec<Position> = position_list
                                        .into_iter()
                                        .filter(|x| x.position_id != position_id)
                                        .collect::<Vec<Position>>();

                                    updated_positions.push(target_position.clone());

                                    Ok(updated_positions)
                                }
                                None => return Err(ContractError::NonExistentPosition {}),
                            }
                        }

                        None => return Err(ContractError::NoUserPositions {}),
                    }
                },
            )?;

            //Add new debt to debt-per-asset tallies
            update_basket_debt(
                deps.storage,
                env,
                deps.querier,
                config,
                &mut basket,
                target_position.collateral_assets,
                amount,
                true,
                false,
            )?;
            
            //Save updated repayment price and debts
            BASKETS.save(deps.storage, basket_id.to_string(), &basket)?;
        }
    } else {
        return Err(ContractError::NoRepaymentPrice {});
    }

    let response = Response::new()
        .add_message(message)
        .add_attribute("method", "increase_debt")
        .add_attribute("basket_id", basket_id.to_string())
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("total_loan", target_position.credit_amount.to_string());

    Ok(response)
}

pub fn update_position(
    storage: &mut dyn Storage,
    basket_id: String,
    valid_position_owner: Addr,
    new_position: Position,
) -> StdResult<()>{

    POSITIONS.update(
        storage,
        (basket_id, valid_position_owner.clone()),
        |old_positions| -> StdResult<Vec<Position>> {
            match old_positions {
                Some(old_positions) => {
                    let new_positions = old_positions
                        .into_iter()
                        .map(|mut stored_position| {
                            //Find position
                            if stored_position.position_id == new_position.position_id {
                                //Swap to target_position 
                                new_position.clone()
                            } else {
                                //Save stored_positon
                                stored_position
                            }
                        })
                        .collect::<Vec<Position>>();

                    Ok(new_positions)
                },
                None => {
                    return Err(StdError::GenericErr {
                        msg: "Invalid position owner".to_string(),
                    })
                }
            }
        },
    )?;

    Ok(())
}


//Checks if any Basket caps are set to 0
//If so the withdrawal assets have to either fully withdraw the asset from the position or only withdraw said asset
//Otherwise users could just fully withdrawal other assets and create a new position
//In a LUNA situation this would leave debt backed by an asset whose solvency we have no faith in
fn check_for_expunged(
    position_assets: Vec<cAsset>,
    withdrawal_assets: Vec<cAsset>,
    basket: Basket
)-> StdResult<()>{

    //Extract the Asset from the cAssets
    let position_assets: Vec<Asset> = position_assets
        .into_iter()
        .map(|cAsset| cAsset.asset)
        .collect::<Vec<Asset>>();

    let withdrawal_assets: Vec<Asset> = withdrawal_assets
        .into_iter()
        .map(|cAsset| cAsset.asset)
        .collect::<Vec<Asset>>();

    let mut passed = true;

    let mut invalid_withdraws = vec![];

    //For any supply cap at 0
    for cap in basket.clone().collateral_supply_caps {

        if cap.supply_cap_ratio.is_zero(){

            //If in the position
            if let Some( asset ) = position_assets.clone().into_iter().find(|asset| asset.info.equal(&cap.asset_info)){

                //Withdraw asset has to either..
                //1) Only withdraw the asset
                if withdrawal_assets[0].info.equal(&asset.info) && withdrawal_assets.len() == 1 as usize{
                    passed = true;
                
                //2) Fully withdraw the asset
                } else if let Some( withdrawal_asset ) = withdrawal_assets.clone().into_iter().find(|w_asset| w_asset.info.equal(&asset.info)){

                    if withdrawal_asset.amount == asset.amount {
                        passed = true;
                    }else {
                        passed = false;
                        invalid_withdraws.push( asset.info.to_string() );
                    } 
                } else {
                    passed = false;
                    invalid_withdraws.push( asset.info.to_string() );
                }

            }
        }
    }

    if !passed {
        return Err( StdError::GenericErr { msg: format!("These assets need to be expunged from the positon: {:?}", invalid_withdraws) } )
    }

    Ok(())
}


pub fn create_basket(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    owner: Option<String>,
    collateral_types: Vec<cAsset>,
    credit_asset: Asset,
    credit_price: Decimal,
    base_interest_rate: Option<Decimal>,
    desired_debt_cap_util: Option<Decimal>,
    credit_pool_ids: Vec<u64>,
    liquidity_multiplier_for_debt_caps: Option<Decimal>,
    liq_queue: Option<String>,
) -> Result<Response, ContractError> {
    let mut config: Config = CONFIG.load(deps.storage)?;

    let valid_owner: Addr = validate_position_owner(deps.api, info.clone(), owner)?;

    //Only contract owner can create new baskets. This will likely be governance.
    if info.sender != config.owner {
        return Err(ContractError::NotContractOwner {});
    }

    let mut new_assets = collateral_types.clone();
    let mut collateral_supply_caps = vec![];

    let mut msgs: Vec<CosmosMsg> = vec![];

    let mut new_liq_queue: Option<Addr> = None;
    if liq_queue.is_some() {
        new_liq_queue = Some(deps.api.addr_validate(&liq_queue.clone().unwrap())?);
    }

    //Minimum viable cAsset parameters
    for (i, asset) in collateral_types.iter().enumerate() {
        new_assets[i].asset.amount = Uint128::zero();

        if asset.max_borrow_LTV >= asset.max_LTV
            && asset.max_borrow_LTV
                >= Decimal::from_ratio(Uint128::new(100u128), Uint128::new(1u128))
        {
            return Err(ContractError::CustomError {
                val: "Max borrow LTV can't be greater or equal to max_LTV nor equal to 100"
                    .to_string(),
            });
        }
        //Make sure Token type addresses are valid
        if let AssetInfo::Token { address } = asset.asset.info.clone() {
            deps.api.addr_validate(&address.to_string())?;
        }

        //No LPs initially. Their pool asset's need to already be added as collateral so they can't come first.
        if asset.pool_info.is_some() {
            return Err(ContractError::CustomError {
                val: String::from("Can't add an LP when creating a basket"),
            });
        } else {
            //Asserting the Collateral Asset has an oracle
            if config.clone().oracle_contract.is_some() {
                //Query Asset Oracle
                deps.querier
                    .query::<AssetResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: config.clone().oracle_contract.unwrap().to_string(),
                        msg: to_binary(&OracleQueryMsg::Asset {
                            asset_info: asset.clone().asset.info,
                        })?,
                    }))?;

                //If it errors it means the oracle doesn't exist
            } else {
                return Err(ContractError::CustomError {
                    val: String::from("Need to setup oracle contract before adding assets"),
                });
            }

            //Create Liquidation Queue for basket assets
            if new_liq_queue.clone().is_some() {
                //Gets Liquidation Queue max premium.
                //The premium has to be at most 5% less than the difference between max_LTV and 100%
                //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
                let max_premium = match Uint128::new(95u128).checked_sub( asset.max_LTV * Uint128::new(100u128) ){
                    Ok( diff ) => diff,
                    //A default to 10 assuming that will be the lowest sp_liq_fee
                    Err( _err ) => Uint128::new(10u128) 
                    ,
                };
                //We rather the LQ liquidate than the SP if possible so its max_premium will be at most the sp_liq fee...
                //..if the first subtraction fails.
                //If it failed, allowing the LQ premium to be more than the SP fee means less efficient liquidations..
                //Since we are aiming for lowest possible fee

                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: new_liq_queue.clone().unwrap().to_string(),
                    msg: to_binary(&LQ_ExecuteMsg::AddQueue {
                        bid_for: asset.clone().asset.info,
                        max_premium,
                        bid_threshold: Uint256::from(1_000_000_000_000u128), //1 million
                    })?,
                    funds: vec![],
                }));
            }
        }

        let mut lp = false;
        if asset.pool_info.is_some() {
            lp = true;
        }
        //Push the cAsset's asset info
        collateral_supply_caps.push(SupplyCap {
            asset_info: asset.clone().asset.info,
            current_supply: Uint128::zero(),
            supply_cap_ratio: Decimal::zero(),
            debt_total: Uint128::zero(),
            lp,
        });
    }

    //Set Basket fields
    let base_interest_rate = base_interest_rate.unwrap_or_else(|| Decimal::percent(0));
    let desired_debt_cap_util = desired_debt_cap_util.unwrap_or_else(|| Decimal::percent(100));
    let liquidity_multiplier = liquidity_multiplier_for_debt_caps.unwrap_or_else(|| Decimal::one());

    let new_basket: Basket = Basket {
        owner: valid_owner.clone(),
        basket_id: config.current_basket_id.clone(),
        current_position_id: Uint128::from(1u128),
        collateral_types: new_assets,
        collateral_supply_caps,
        credit_asset: credit_asset.clone(),
        credit_price,
        base_interest_rate,
        liquidity_multiplier,
        desired_debt_cap_util,
        pending_revenue: Uint128::zero(),
        credit_last_accrued: env.block.time.seconds(),
        liq_queue: new_liq_queue,
        negative_rates: true,
        oracle_set: false,
    };

    //CreateDenom Msg
    let subdenom: String;
    let sub_msg: SubMsg;

    if let AssetInfo::NativeToken { denom } = credit_asset.clone().info {
        //Create credit as native token using a tokenfactory proxy
        sub_msg = create_denom(
            config.clone(),
            String::from(denom.clone()),
            new_basket.basket_id.to_string(),
            Some(liquidity_multiplier),
        )?;

        subdenom = denom;
    } else {
        return Err(ContractError::CustomError {
            val: "Can't create a basket without creating a native token denom".to_string(),
        });
    }

    //Add asset to liquidity check contract
    //Liquidity AddAsset Msg
    let mut msgs = vec![];
    if let Some(liquidity_contract) = config.clone().liquidity_contract {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: liquidity_contract.to_string(),
            msg: to_binary(&LiquidityExecuteMsg::AddAsset {
                asset: LiquidityInfo {
                    asset: new_basket.clone().credit_asset.info,
                    pool_ids: credit_pool_ids,
                },
            })?,
            funds: vec![],
        }));
    }

    //Save Basket
    BASKETS.update(
        deps.storage,
        new_basket.basket_id.to_string(),
        |basket| -> Result<Basket, ContractError> {
            match basket {
                Some(_basket) => {
                    //This is a new basket so there shouldn't already be one made
                    return Err(ContractError::ConfigIDError {});
                }
                None => Ok(new_basket),
            }
        },
    )?;

    config.current_basket_id += Uint128::from(1u128);
    CONFIG.save(deps.storage, &config)?;

    //Response Building
    let response = Response::new();

    Ok(response
        .add_attributes(vec![
            attr("method", "create_basket"),
            attr("basket_id", config.current_basket_id.to_string()),
            attr("position_owner", valid_owner.to_string()),
            attr("credit_asset", credit_asset.to_string()),
            attr("credit_subdenom", subdenom),
            attr("credit_price", credit_price.to_string()),
            attr(
                "liq_queue",
                liq_queue.unwrap_or_else(|| String::from("None")),
            ),
        ])
        .add_submessage(sub_msg)
        .add_messages(msgs))
}

pub fn edit_basket(
    //Can't edit basket id, current_position_id or credit_asset. Can only add cAssets. Can edit owner. Credit price can only be changed thru the accrue function.
    deps: DepsMut,
    info: MessageInfo,
    basket_id: Uint128,
    added_cAsset: Option<cAsset>,
    owner: Option<String>,
    liq_queue: Option<String>,
    pool_ids: Option<Vec<u64>>,
    liquidity_multiplier: Option<Decimal>,
    collateral_supply_caps: Option<Vec<SupplyCap>>,
    base_interest_rate: Option<Decimal>,
    desired_debt_cap_util: Option<Decimal>,
    credit_asset_twap_price_source: Option<TWAPPoolInfo>,
    negative_rates: Option<bool>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let new_owner: Option<Addr>;

    if let Some(owner) = owner {
        new_owner = Some(deps.api.addr_validate(&owner)?);
    } else {
        new_owner = None
    }

    let mut new_queue: Option<Addr> = None;
    if liq_queue.is_some() {
        new_queue = Some(deps.api.addr_validate(&liq_queue.clone().unwrap())?);
    }

    //Blank cAsset
    //This never gets added unless its edited. Here due to uninitialized errors.
    let mut new_cAsset = cAsset {
        asset: Asset {
            info: AssetInfo::NativeToken {
                denom: String::from("None"),
            },
            amount: Uint128::zero(),
        },
        max_borrow_LTV: Decimal::zero(),
        max_LTV: Decimal::zero(),
        pool_info: None,
    };

    let mut msgs: Vec<CosmosMsg> = vec![];

    let mut basket = BASKETS.load(deps.storage, basket_id.clone().to_string())?;
    //cAsset check
    if added_cAsset.is_some() {
        let mut check = true;
        //Each cAsset has to initialize amount as 0..
        new_cAsset = added_cAsset.clone().unwrap();
        new_cAsset.asset.amount = Uint128::zero();

        //No duplicates
        if let Some(_duplicate) = basket
            .clone()
            .collateral_types
            .into_iter()
            .find(|cAsset| cAsset.asset.info.equal(&new_cAsset.asset.info))
        {
            return Err(ContractError::CustomError {
                val: format!(
                    "Attempting to add duplicate asset: {}",
                    new_cAsset.asset.info
                ),
            });
        }

        if added_cAsset.clone().unwrap().pool_info.is_some() {
            //Query Pool State and Error if assets are out of order
            let pool_info = added_cAsset.clone().unwrap().pool_info.clone().unwrap();

            //Query share asset amount
            let pool_state = match deps.querier.query::<PoolStateResponse>(&QueryRequest::Wasm(
                WasmQuery::Smart {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: match to_binary(&OsmoQueryMsg::PoolState {
                        id: pool_info.pool_id,
                    }) {
                        Ok(binary) => binary,
                        Err(err) => {
                            return Err(ContractError::CustomError {
                                val: err.to_string(),
                            })
                        }
                    },
                },
            )) {
                Ok(resp) => resp,
                Err(err) => {
                    return Err(ContractError::CustomError {
                        val: err.to_string(),
                    })
                }
            };
            let pool_assets = pool_state.assets;

            //Set correct shares denom
            new_cAsset.asset.info = AssetInfo::NativeToken {
                denom: pool_state.shares.denom,
            };

            //Assert Asset order of pool_assets in PoolInfo object
            //Add assets to supply_caps
            //Check that assets have oracles
            for (i, asset) in pool_assets.iter().enumerate() {
                if asset.denom != pool_info.asset_infos[i].info.to_string() {
                    return Err( ContractError::CustomError { val: format!("cAsset #{}: PoolInfo.asset_denoms must be in the order of osmo-bindings::PoolStateResponse.assets {:?} ", i+1, pool_assets) } );
                }

                //Push each Pool asset info to collateral_supply_caps if not already found
                if let None = basket
                    .clone()
                    .collateral_supply_caps
                    .into_iter()
                    .find(|cap| {
                        cap.asset_info.equal(&AssetInfo::NativeToken {
                            denom: asset.clone().denom,
                        })
                    })
                {
                    basket.collateral_supply_caps.push(SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: asset.clone().denom,
                        },
                        current_supply: Uint128::zero(),
                        supply_cap_ratio: Decimal::zero(),
                        debt_total: Uint128::zero(),
                        lp: false,
                    });
                }

                //Asserting the Pool Asset has an oracle
                if config.clone().oracle_contract.is_some() {
                    //Query Asset Oracle
                    deps.querier
                        .query::<AssetResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                            contract_addr: config.clone().oracle_contract.unwrap().to_string(),
                            msg: to_binary(&OracleQueryMsg::Asset {
                                asset_info: AssetInfo::NativeToken {
                                    denom: asset.clone().denom,
                                },
                            })?,
                        }))?;

                    //If it errors it means the oracle doesn't exist
                } else {
                    return Err(ContractError::CustomError {
                        val: String::from("Need to setup oracle contract before adding assets"),
                    });
                }

                //Asserting that its pool assets are already added as collateral types
                if let None = basket.clone().collateral_types.into_iter().find(|cAsset| {
                    cAsset.asset.info.equal(&AssetInfo::NativeToken {
                        denom: asset.clone().denom,
                    })
                }) {
                    return Err(ContractError::CustomError {
                        val: format!(
                            "Need to add all pool assets before adding the LP. Errored on {}",
                            asset.denom
                        ),
                    });
                }

                //Create Liquidation Queue for its assets
                if basket.clone().liq_queue.is_some() {
                    //Gets Liquidation Queue max premium.
                    //The premium has to be at most 5% less than the difference between max_LTV and 100%
                    //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
                    let max_premium = match Uint128::new(95u128).checked_sub( new_cAsset.max_LTV * Uint128::new(100u128) ){
                        Ok( diff ) => diff,
                        //A default to 10 assuming that will be the lowest sp_liq_fee
                        Err( _err ) => Uint128::new(10u128) 
                        ,
                    };

                    msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: basket.clone().liq_queue.unwrap().into_string(),
                        msg: to_binary(&LQ_ExecuteMsg::AddQueue {
                            bid_for: new_cAsset.clone().asset.info,
                            max_premium,
                            bid_threshold: Uint256::from(1_000_000_000_000u128), //1 million
                        })?,
                        funds: vec![],
                    }));
                } else if new_queue.clone().is_some() {
                    //Gets Liquidation Queue max premium.
                    //The premium has to be at most 5% less than the difference between max_LTV and 100%
                    //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
                    let max_premium =
                        Uint128::new(95u128) - new_cAsset.max_LTV * Uint128::new(100u128);

                    msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: new_queue.clone().unwrap().into_string(),
                        msg: to_binary(&LQ_ExecuteMsg::AddQueue {
                            bid_for: new_cAsset.clone().asset.info,
                            max_premium,
                            bid_threshold: Uint256::from(1_000_000_000_000u128), //1 million
                        })?,
                        funds: vec![],
                    }));
                }
            }
        } else {
            //Asserting the Collateral Asset has an oracle
            if config.clone().oracle_contract.is_some() {
                //Query Asset Oracle
                deps.querier
                    .query::<AssetResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: config.clone().oracle_contract.unwrap().to_string(),
                        msg: to_binary(&OracleQueryMsg::Asset {
                            asset_info: new_cAsset.clone().asset.info,
                        })?,
                    }))?;

                //If it errors it means the oracle doesn't exist
            } else {
                return Err(ContractError::CustomError {
                    val: String::from("Need to setup oracle contract before adding assets"),
                });
            }

            //Create Liquidation Queue for its assets
            if basket.clone().liq_queue.is_some() {
                //Gets Liquidation Queue max premium.
                //The premium has to be at most 5% less than the difference between max_LTV and 100%
                //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
                let max_premium = match Uint128::new(95u128).checked_sub( new_cAsset.max_LTV * Uint128::new(100u128) ){
                    Ok( diff ) => diff,
                    //A default to 10 assuming that will be the lowest sp_liq_fee
                    Err( _err ) => Uint128::new(10u128) 
                    ,
                };

                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: basket.clone().liq_queue.unwrap().into_string(),
                    msg: to_binary(&LQ_ExecuteMsg::AddQueue {
                        bid_for: new_cAsset.clone().asset.info,
                        max_premium,
                        bid_threshold: Uint256::from(1_000_000_000_000u128), //1 million
                    })?,
                    funds: vec![],
                }));
            } else if new_queue.clone().is_some() {
                //Gets Liquidation Queue max premium.
                //The premium has to be at most 5% less than the difference between max_LTV and 100%
                //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
                let max_premium = match Uint128::new(95u128).checked_sub( new_cAsset.max_LTV * Uint128::new(100u128) ){
                    Ok( diff ) => diff,
                    //A default to 10 assuming that will be the lowest sp_liq_fee
                    Err( _err ) => Uint128::new(10u128) 
                    ,
                };

                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: new_queue.clone().unwrap().into_string(),
                    msg: to_binary(&LQ_ExecuteMsg::AddQueue {
                        bid_for: new_cAsset.clone().asset.info,
                        max_premium,
                        bid_threshold: Uint256::from(1_000_000_000_000u128), //1 million
                    })?,
                    funds: vec![],
                }));
            }
        }

        //..needs minimum viable LTV parameters
        if new_cAsset.max_borrow_LTV >= new_cAsset.max_LTV
            || new_cAsset.max_borrow_LTV
                >= Decimal::from_ratio(Uint128::new(100u128), Uint128::new(1u128))
        {
            check = false;
        }

        if !check {
            return Err(ContractError::CustomError {
                val: "Max borrow LTV can't be greater or equal to max_LTV nor equal to 100"
                    .to_string(),
            });
        }

        let mut lp = false;
        if new_cAsset.pool_info.is_some() {
            lp = true;
        }
        //Push the cAsset's asset info
        basket.collateral_supply_caps.push(SupplyCap {
            asset_info: new_cAsset.clone().asset.info,
            current_supply: Uint128::zero(),
            supply_cap_ratio: Decimal::zero(),
            debt_total: Uint128::zero(),
            lp,
        });
    }

    //Save basket's new collateral_supply_caps
    BASKETS.save(deps.storage, basket_id.to_string(), &basket)?;

    //Send credit_asset TWAP info to Oracle Contract
    let mut oracle_set = basket.oracle_set;

    if let Some(credit_twap) = credit_asset_twap_price_source {
        if config.clone().oracle_contract.is_some() {
            //Set the credit Oracle. Using EditAsset updates or adds.
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.clone().oracle_contract.unwrap().to_string(),
                msg: to_binary(&OracleExecuteMsg::EditAsset {
                    asset_info: basket.clone().credit_asset.info,
                    oracle_info: Some(AssetOracleInfo {
                        basket_id: basket.clone().basket_id,
                        osmosis_pools_for_twap: vec![credit_twap],
                        static_price: None,
                    }),
                    remove: false,
                })?,
                funds: vec![],
            }));

            oracle_set = true;
        }
    };

    let mut attrs = vec![attr("method", "edit_basket"), attr("basket_id", basket_id)];

    //Create EditAssetMsg for Liquidity contract
    if let Some(pool_ids) = pool_ids {
        attrs.push(attr("new_pool_ids", format!("{:?}", pool_ids.clone())));

        if let Some(liquidity_contract) = config.clone().liquidity_contract {
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: liquidity_contract.to_string(),
                msg: to_binary(&LiquidityExecuteMsg::EditAsset {
                    asset: LiquidityInfo {
                        asset: basket.clone().credit_asset.info,
                        pool_ids,
                    },
                })?,
                funds: vec![],
            }));
        }
    }

    //Update Basket
    BASKETS.update(
        deps.storage,
        basket_id.to_string(),
        |basket| -> Result<Basket, ContractError> {
            match basket {
                Some(mut basket) => {
                    if info.sender.clone() != config.owner && info.sender.clone() != basket.owner {
                        return Err(ContractError::Unauthorized {});
                    } else {
                        if added_cAsset.is_some() {
                            basket.collateral_types.push(new_cAsset.clone());
                            attrs.push(attr(
                                "added_cAsset",
                                new_cAsset.clone().asset.info.to_string(),
                            ));
                        }
                        if new_owner.is_some() {
                            basket.owner = new_owner.clone().unwrap();
                            attrs.push(attr("new_owner", new_owner.clone().unwrap().to_string()));
                        }
                        if liq_queue.is_some() {
                            basket.liq_queue = new_queue.clone();
                            attrs.push(attr("new_queue", new_queue.clone().unwrap().to_string()));
                        }

                        if collateral_supply_caps.is_some() {
                            //Set new caps
                            for new_cap in collateral_supply_caps.unwrap() {
                                if let Some((index, _cap)) = basket
                                    .clone()
                                    .collateral_supply_caps
                                    .into_iter()
                                    .enumerate()
                                    .find(|(_x, cap)| cap.asset_info.equal(&new_cap.asset_info))
                                {
                                    basket.collateral_supply_caps[index].supply_cap_ratio =
                                        new_cap.supply_cap_ratio;
                                }
                            }
                            attrs.push(attr("new_collateral_supply_caps", String::from("Edited")));
                        }
                        if base_interest_rate.is_some() {
                            basket.base_interest_rate = base_interest_rate.clone().unwrap();
                            attrs.push(attr(
                                "new_base_interest_rate",
                                base_interest_rate.clone().unwrap().to_string(),
                            ));
                        }
                        if desired_debt_cap_util.is_some() {
                            basket.desired_debt_cap_util = desired_debt_cap_util.clone().unwrap();
                            attrs.push(attr(
                                "new_desired_debt_cap_util",
                                desired_debt_cap_util.clone().unwrap().to_string(),
                            ));
                        }
                        if let Some(toggle) = negative_rates {
                            basket.negative_rates = toggle.clone();
                            attrs.push(attr("new_negative_rates", toggle.to_string()));
                        }
                        //Set basket specific multiplier
                        if let Some(multiplier) = liquidity_multiplier {
                            basket.liquidity_multiplier = multiplier.clone();
                            attrs.push(attr("new_liquidity_multiplier", multiplier.to_string()));
                        }

                        basket.oracle_set = oracle_set;
                    }

                    Ok(basket)
                }
                None => return Err(ContractError::NonExistentBasket {}),
            }
        },
    )?;

    //Set asset specific multiplier
    if let Some(_multiplier) = liquidity_multiplier {
        let mut credit_asset_multiplier = Decimal::zero();
        //Uint128 to int
        let range: i32 = config.current_basket_id.to_string().parse().unwrap();

        for basket_id in 1..range {
            let stored_basket = BASKETS.load(deps.storage, basket_id.to_string())?;

            //Add if same credit asset
            if stored_basket
                .credit_asset
                .info
                .equal(&basket.credit_asset.info)
            {
                credit_asset_multiplier += stored_basket.liquidity_multiplier;
            }
        }
        CREDIT_MULTI.save(
            deps.storage,
            basket.credit_asset.info.to_string(),
            &credit_asset_multiplier,
        )?;
    }

    Ok(Response::new().add_attributes(attrs).add_messages(msgs))
}


pub fn clone_basket(deps: DepsMut, basket_id: Uint128) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    //Load basket to clone from
    let base_basket = BASKETS.load(deps.storage, basket_id.to_string())?;

    //Get new credit price using the Oracle's newly upgraded logic
    let credit_price: Decimal = deps
        .querier
        .query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().oracle_contract.unwrap().to_string(),
            msg: to_binary(&OracleQueryMsg::Price {
                asset_info: base_basket.clone().credit_asset.info,
                twap_timeframe: config.clone().credit_twap_timeframe,
                basket_id: Some(config.clone().current_basket_id),
            })?,
        }))?
        .avg_price;

    let new_supply_caps = base_basket
        .clone()
        .collateral_supply_caps
        .into_iter()
        .map(|cap| SupplyCap {
            current_supply: Uint128::zero(),
            supply_cap_ratio: Decimal::zero(),
            ..cap
        })
        .collect::<Vec<SupplyCap>>();

    let new_basket = Basket {
        basket_id: config.clone().current_basket_id,
        credit_price,
        collateral_supply_caps: new_supply_caps,
        ..base_basket.clone()
    };

    //Save Config
    config.current_basket_id += Uint128::new(1u128);
    CONFIG.save(deps.storage, &config.clone())?;

    //Save new Basket
    BASKETS.save(
        deps.storage,
        new_basket.clone().basket_id.to_string(),
        &new_basket,
    )?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "clone_basket"),
        attr("cloned_basket_id", base_basket.basket_id),
        attr("new_basket_id", config.current_basket_id),
        attr("new_price", credit_price.to_string()),
    ]))
}


//create_position = check collateral types, create position object
pub fn create_position(
    deps: &mut dyn Storage,
    cAssets: Vec<cAsset>, //Assets being added into the position
    basket: &mut Basket,
    env: Env,
) -> Result<Position, ContractError> {   

    //Create Position instance
    let new_position: Position;

    new_position = Position {
        position_id: basket.current_position_id,
        collateral_assets: cAssets,
        credit_amount: Uint128::zero(),
        basket_id: basket.clone().basket_id,
        last_accrued: env.block.time.seconds(),
    };

    //increment position id
    basket.current_position_id += Uint128::from(1u128);

    return Ok(new_position);
}


pub fn credit_mint_msg(
    config: Config,
    credit_asset: Asset,
    recipient: Addr,
) -> StdResult<CosmosMsg> {
    match credit_asset.clone().info {
        AssetInfo::Token { address: _ } => {
            return Err(StdError::GenericErr {
                msg: "Credit has to be a native token".to_string(),
            })
        }
        AssetInfo::NativeToken { denom } => {
            if config.osmosis_proxy.is_some() {
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom,
                        amount: credit_asset.amount,
                        mint_to_address: recipient.to_string(),
                    })?,
                    funds: vec![],
                });
                Ok(message)
            } else {
                return Err(StdError::GenericErr {
                    msg: "No proxy contract setup".to_string(),
                });
            }
        }
    }
}

pub fn credit_burn_msg(config: Config, env: Env, credit_asset: Asset) -> StdResult<CosmosMsg> {
    match credit_asset.clone().info {
        AssetInfo::Token { address: _ } => {
            return Err(StdError::GenericErr {
                msg: "Credit has to be a native token".to_string(),
            })
        }
        AssetInfo::NativeToken { denom } => {
            if config.osmosis_proxy.is_some() {
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::BurnTokens {
                        denom,
                        amount: credit_asset.amount,
                        burn_from_address: env.contract.address.to_string(),
                    })?,
                    funds: vec![],
                });
                Ok(message)
            } else {
                return Err(StdError::GenericErr {
                    msg: "No proxy contract setup".to_string(),
                });
            }
        }
    }
}

pub fn withdrawal_msg(asset: Asset, recipient: Addr) -> StdResult<CosmosMsg> {
    match asset.clone().info {
        AssetInfo::NativeToken { denom: _ } => {
            let coin: Coin = asset_to_coin(asset)?;
            let message = CosmosMsg::Bank(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![coin],
            });
            Ok(message)
        }
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
        }
    }
}

pub fn asset_to_coin(asset: Asset) -> StdResult<Coin> {
    match asset.info {
        //
        AssetInfo::Token { address: _ } => {
            return Err(StdError::GenericErr {
                msg: "Only native assets can become Coin objects".to_string(),
            })
        }
        AssetInfo::NativeToken { denom } => Ok(Coin {
            denom: denom,
            amount: asset.amount,
        }),
    }
}

pub fn assert_credit(credit: Option<Uint128>) -> StdResult<Uint128> {
    //Check if user wants to take credit out now
    let checked_amount = if credit.is_some() && !credit.unwrap().is_zero() {
        Uint128::from(credit.unwrap())
    } else {
        Uint128::from(0u128)
    };
    Ok(checked_amount)
}

pub fn get_avg_LTV(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    basket: Basket,
    collateral_assets: Vec<cAsset>,
) -> StdResult<(Decimal, Decimal, Decimal, Vec<Decimal>)> {
    let collateral_assets =
        get_LP_pool_cAssets(querier, config.clone(), basket, collateral_assets)?;

    let (cAsset_values, cAsset_prices) = get_asset_values(
        storage,
        env,
        querier,
        collateral_assets.clone(),
        config,
        None,
    )?;

    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values {
        if total_value == Decimal::zero() {
            cAsset_ratios.push(Decimal::zero());
        } else {
            cAsset_ratios.push(decimal_division(cAsset, total_value));
        }
    }

    //converting % of value to avg_LTV by multiplying collateral LTV by % of total value
    let mut avg_max_LTV: Decimal = Decimal::zero();
    let mut avg_borrow_LTV: Decimal = Decimal::zero();

    if cAsset_ratios.len() == 0 {
        return Ok((
            Decimal::percent(0),
            Decimal::percent(0),
            Decimal::percent(0),
            vec![],
        ));
        
    }

    //Skip unecessary calculations if length is 1
    if cAsset_ratios.len() == 1 {
        return Ok((
            collateral_assets[0].max_borrow_LTV,
            collateral_assets[0].max_LTV,
            total_value,
            cAsset_prices,
        ));
    }

    for (i, _cAsset) in collateral_assets.clone().iter().enumerate() {
        avg_borrow_LTV +=
            decimal_multiplication(cAsset_ratios[i], collateral_assets[i].max_borrow_LTV);
    }

    for (i, _cAsset) in collateral_assets.clone().iter().enumerate() {
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
) -> StdResult<Vec<Decimal>> {
    let (cAsset_values, _cAsset_prices) = get_asset_values(
        storage,
        env,
        querier,
        collateral_assets.clone(),
        config,
        None,
    )?;

    let total_value: Decimal = cAsset_values.iter().sum();

    //getting each cAsset's % of total value
    let mut cAsset_ratios: Vec<Decimal> = vec![];
    for cAsset in cAsset_values {
        if total_value == Decimal::zero() {
            cAsset_ratios.push(Decimal::zero());
        } else {
            cAsset_ratios.push(decimal_division(cAsset, total_value));
        }
    }

    Ok(cAsset_ratios)
}

pub fn insolvency_check(
    //Returns true if insolvent, current_LTV and available fee to the caller if insolvent
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    basket: Basket,
    collateral_assets: Vec<cAsset>,
    credit_amount: Decimal,
    credit_price: Decimal,
    max_borrow: bool, //Toggle for either over max_borrow or over max_LTV (liquidatable), ie taking the minimum collateral ratio into account.
    config: Config,
) -> StdResult<(bool, Decimal, Uint128)> {//insolvent, current_LTV, available_fee

    //No assets but still has debt, return insolvent and skip other checks
    let total_assets: Uint128 = collateral_assets
        .iter()
        .map(|asset| asset.asset.amount)
        .collect::<Vec<Uint128>>()
        .iter()
        .sum();
    if total_assets.is_zero() && !credit_amount.is_zero() {
        return Ok((true, Decimal::percent(100), Uint128::zero()));
    }

    let avg_LTVs: (Decimal, Decimal, Decimal, Vec<Decimal>) =
        get_avg_LTV(storage, env, querier, config, basket, collateral_assets)?;

    let asset_values: Decimal = avg_LTVs.2; //pulls total_asset_value

    let check: bool;
    let current_LTV = if asset_values.is_zero() {
        Decimal::percent(100)
    } else {
        decimal_division(
            decimal_multiplication(credit_amount, credit_price),
            asset_values,
        )
    };

    match max_borrow {
        true => {
            //Checks max_borrow
            check = if asset_values.is_zero() && credit_amount.is_zero() {
                false
            } else {
                current_LTV > avg_LTVs.0
            };
        }
        false => {
            //Checks max_LTV
            check = if asset_values.is_zero() && credit_amount.is_zero() {
                false
            } else {
                current_LTV > avg_LTVs.1
            };
        }
    }

    let available_fee = if check {
        match max_borrow {
            true => {
                //Checks max_borrow
                (current_LTV - avg_LTVs.0) * Uint128::new(1u128)
            }
            false => {
                //Checks max_LTV
                (current_LTV - avg_LTVs.1) * Uint128::new(1u128)
            }
        }
    } else {
        Uint128::zero()
    };

    Ok((check, current_LTV, available_fee))
}

pub fn assert_basket_assets(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket_id: Uint128,
    assets: Vec<Asset>,
    add_to_cAsset: bool,
) -> Result<Vec<cAsset>, ContractError> {

    let mut basket: Basket = match BASKETS.load(storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    //Checking if Assets for the position are available collateral assets in the basket
    let mut valid = false;
    let mut collateral_assets: Vec<cAsset> = Vec::new();

    for asset in assets {
        for cAsset in basket.clone().collateral_types {
            match (asset.clone().info, cAsset.asset.info) {
                (
                    AssetInfo::Token { address },
                    AssetInfo::Token {
                        address: cAsset_address,
                    },
                ) => {
                    if address == cAsset_address {
                        valid = true;
                        collateral_assets.push(cAsset {
                            asset: asset.clone(),
                            ..cAsset
                        });
                    }
                }
                (
                    AssetInfo::NativeToken { denom },
                    AssetInfo::NativeToken {
                        denom: cAsset_denom,
                    },
                ) => {
                    if denom == cAsset_denom {
                        valid = true;
                        collateral_assets.push(cAsset {
                            asset: asset.clone(),
                            ..cAsset
                        });
                    }
                }
                (_, _) => continue,
            }
        }

        //Error if invalid collateral, meaning it wasn't found in the list of cAssets
        if !valid {
            return Err(ContractError::InvalidCollateral {});
        }
        valid = false;
    }

    //Add valid asset amounts to running basket total
    //This is done before deposit() so if that errors this will revert as well
    //////We don't want this to trigger for withdrawals bc debt needs to accrue on the previous basket state
    //////For deposit's its fine bc it'll error when invalid and doesn't accrue debt
    if add_to_cAsset {
        update_basket_tally(
            storage,
            querier,
            env,
            &mut basket,
            collateral_assets.clone(),
            add_to_cAsset,
        )?;
        BASKETS.save(storage, basket_id.to_string(), &basket)?;
    }

    Ok(collateral_assets)
}

fn update_basket_tally(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    collateral_assets: Vec<cAsset>,
    add_to_cAsset: bool,
) -> Result<(), ContractError> {
    let config = CONFIG.load(storage)?;

    //get LP cAssets
    let collateral_assets = get_LP_pool_cAssets(querier, config.clone(), basket.clone(), collateral_assets)?;

    for cAsset in collateral_assets.iter() {

        if let Some((index, mut cap)) = basket
            .clone()
            .collateral_supply_caps
            .into_iter()
            .enumerate()
            .find(|(_x, cap)| cap.asset_info.equal(&cAsset.asset.info))
        {
            if add_to_cAsset {
                cap.current_supply += cAsset.asset.amount;
            } else {
                cap.current_supply -= cAsset.asset.amount;
            }
            basket.collateral_supply_caps[index] = cap;
        }
    
    }

    //Map supply caps to cAssets to get new ratios
    //The functions only need Asset
    let temp_cAssets: Vec<cAsset> = basket
        .clone()
        .collateral_supply_caps
        .into_iter()
        .map(|cap| {
            if cap.lp {
                //We skip LPs bc we don't want to double count their assets
                cAsset {
                    asset: Asset {
                        info: cap.asset_info,
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::zero(),
                    max_LTV: Decimal::zero(),
                    pool_info: None,
                }
            } else {
                cAsset {
                    asset: Asset {
                        info: cap.asset_info,
                        amount: cap.current_supply,
                    },
                    max_borrow_LTV: Decimal::zero(),
                    max_LTV: Decimal::zero(),
                    pool_info: None,
                }
            }
        })
        .collect::<Vec<cAsset>>();

    let mut new_basket_ratios =
        get_cAsset_ratios(storage, env, querier, temp_cAssets.clone(), config.clone())?;

    //Add LP assets' ratios to the LP's supply cap ratios
    for (index, cap) in basket
        .clone()
        .collateral_supply_caps
        .into_iter()
        .enumerate()
    {
        //If an LP
        if cap.lp {
            //Find the LP's cAsset and get its pool_assets
            if let Some(_lp_cAsset) = temp_cAssets
                .clone()
                .into_iter()
                .find(|asset| asset.asset.info.equal(&cap.asset_info))
            {
                if let Some(basket_lp_cAsset) = basket
                    .clone()
                    .collateral_types
                    .into_iter()
                    .find(|asset| asset.asset.info.equal(&cap.asset_info))
                {
                    //Find the pool_asset's ratio of its corresponding cAsset
                    let pool_info = basket_lp_cAsset.pool_info.unwrap();
                    for (pa_index, pool_asset) in
                        pool_info.clone().asset_infos.into_iter().enumerate()
                    {
                        if let Some((i, pool_asset_cAsset)) = temp_cAssets
                            .clone()
                            .into_iter()
                            .enumerate()
                            .find(|(_x, asset)| asset.asset.info.equal(&pool_asset.info))
                        {
                            //Query share asset amount
                            let share_asset_amounts = querier
                                .query::<PoolStateResponse>(&QueryRequest::Wasm(
                                    WasmQuery::Smart {
                                        contract_addr: config
                                            .clone()
                                            .osmosis_proxy
                                            .unwrap()
                                            .to_string(),
                                        msg: to_binary(&OsmoQueryMsg::PoolState {
                                            id: pool_info.pool_id,
                                        })?,
                                    },
                                ))?
                                .shares_value(basket_lp_cAsset.asset.amount);

                            let asset_amount = share_asset_amounts[pa_index].amount;

                            if !pool_asset_cAsset.asset.amount.is_zero() {
                                let ratio = decimal_division(
                                    Decimal::from_ratio(asset_amount, Uint128::new(1u128)),
                                    Decimal::from_ratio(
                                        pool_asset_cAsset.asset.amount,
                                        Uint128::new(1u128),
                                    ),
                                );

                                //Find amount of cap in %
                                let cap_amount =
                                    decimal_multiplication(ratio, new_basket_ratios[i]);

                                //Add the ratio of the cap to the lp's
                                new_basket_ratios[index] += cap_amount;
                            }
                        }
                    }
                }
            }
        }
    }

    //Assert new ratios aren't above Collateral Supply Caps. If so, error.
    //Only for deposits
    for (i, ratio) in new_basket_ratios.into_iter().enumerate() {
        if basket.collateral_supply_caps != vec![] {
            if ratio > basket.collateral_supply_caps[i].supply_cap_ratio && add_to_cAsset {

                return Err(ContractError::CustomError {
                    val: format!(
                        "Supply cap ratio for {} is over the limit ({} > {})",
                        basket.collateral_supply_caps[i].asset_info,
                        ratio,
                        basket.collateral_supply_caps[i].supply_cap_ratio
                    ),
                });
            }
        }
    }

    Ok(())
}

//Validate Recipient
pub fn validate_position_owner(
    deps: &dyn Api,
    info: MessageInfo,
    recipient: Option<String>,
) -> StdResult<Addr> {

    let valid_recipient: Addr = if recipient.is_some() {
        deps.addr_validate(&recipient.unwrap())?
    } else {
        info.sender.clone()
    };

    Ok(valid_recipient)
}

pub fn store_price(
    storage: &mut dyn Storage,
    asset_token: &AssetInfo,
    price: &StoredPrice,
) -> StdResult<()> {
    let mut price_bucket: Bucket<StoredPrice> = Bucket::new(storage, PREFIX_PRICE);
    price_bucket.save(&to_binary(asset_token)?, price)
}

pub fn read_price(storage: &dyn Storage, asset_token: &AssetInfo) -> StdResult<StoredPrice> {
    let price_bucket: ReadonlyBucket<StoredPrice> = ReadonlyBucket::new(storage, PREFIX_PRICE);
    price_bucket.load(&to_binary(asset_token)?)
}

fn query_price(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    asset_info: AssetInfo,
    basket_id: Option<Uint128>,
) -> StdResult<Decimal> {
    //Set timeframe
    let mut twap_timeframe: u64 = config.collateral_twap_timeframe;

    if let Some(basket_id) = basket_id {
        let basket = BASKETS.load(storage, basket_id.to_string())?;
        //if AssetInfo is the basket.credit_asset
        if asset_info.equal(&basket.credit_asset.info) {
            twap_timeframe = config.credit_twap_timeframe;
        }
    }

    //Query Price
    let price = match querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().oracle_contract.unwrap().to_string(),
        msg: to_binary(&OracleQueryMsg::Price {
            asset_info: asset_info.clone(),
            twap_timeframe,
            basket_id,
        })?,
    })) {
        Ok(res) => {
            //Store new price
            store_price(
                storage,
                &asset_info,
                &StoredPrice {
                    price: res.avg_price,
                    last_time_updated: env.block.time.seconds(),
                },
            )?;
            //
            res.avg_price
        }
        Err(_err) => {
            //If the query errors, try and use a stored price
            let stored_price: StoredPrice = match read_price(storage, &asset_info) {
                Ok(info) => info,
                Err(_) => {
                    //Set time to fail in the next check. We don't want the error to stop from querying though
                    StoredPrice {
                        price: Decimal::zero(),
                        last_time_updated: env
                            .block
                            .time
                            .plus_seconds(config.oracle_time_limit + 1u64)
                            .seconds(),
                    }
                }
            };

            let time_elapsed: Option<u64> = env
                .block
                .time
                .seconds()
                .checked_sub(stored_price.last_time_updated);
            //If its None then the subtraction was negative meaning the initial read_price() errored
            if time_elapsed.is_some() {
                if time_elapsed.unwrap() <= config.oracle_time_limit {
                    stored_price.price
                } else {
                    return Err(StdError::GenericErr {
                        msg: String::from("Oracle price invalid"),
                    });
                }
            }else{
                return Err(StdError::GenericErr {
                    msg: String::from("Oracle price invalid"),
                });
            }
        }
    };

    Ok(price)
}

//Get Asset values / query oracle
pub fn get_asset_values(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    assets: Vec<cAsset>,
    config: Config,
    basket_id: Option<Uint128>,
) -> StdResult<(Vec<Decimal>, Vec<Decimal>)> {
    //Getting proportions for position collateral to calculate avg LTV
    //Using the index in the for loop to parse through the assets Vec and collateral_assets Vec
    //, as they are now aligned due to the collateral check w/ the Config's data
    let mut cAsset_values: Vec<Decimal> = vec![];
    let mut cAsset_prices: Vec<Decimal> = vec![];

    if config.clone().oracle_contract.is_some() {
        for (_i, cAsset) in assets.iter().enumerate() {
            //If an Osmosis LP
            if cAsset.pool_info.is_some() {
                let pool_info = cAsset.clone().pool_info.unwrap();
                let mut asset_prices = vec![];

                for (pool_asset) in pool_info.clone().asset_infos {
                    let price = query_price(
                        storage,
                        querier,
                        env.clone(),
                        config.clone(),
                        pool_asset.info,
                        basket_id,
                    )?;
                    //Append price
                    asset_prices.push(price);
                }

                //Calculate share value
                let cAsset_value = {
                    //Query share asset amount
                    let share_asset_amounts = querier
                        .query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                            contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                            msg: to_binary(&OsmoQueryMsg::PoolState {
                                id: pool_info.pool_id,
                            })?,
                        }))?
                        .shares_value(cAsset.asset.amount);

                    //Calculate value of cAsset
                    let mut value = Decimal::zero();
                    for (i, price) in asset_prices.into_iter().enumerate() {
                        //Assert we are pulling asset amount from the correct asset
                        let asset_share =
                            match share_asset_amounts.clone().into_iter().find(|coin| {
                                AssetInfo::NativeToken {
                                    denom: coin.denom.clone(),
                                } == pool_info.clone().asset_infos[i].info
                            }) {
                                Some(coin) => coin,
                                None => {
                                    return Err(StdError::GenericErr {
                                        msg: format!(
                                            "Invalid asset denom: {}",
                                            pool_info.clone().asset_infos[i].info
                                        ),
                                    })
                                }
                            };
                        //Normalize Asset amounts to native token decimal amounts (6 places: 1 = 1_000_000)
                        let exponent_difference = pool_info.clone().asset_infos[i]
                            .decimals
                            .checked_sub(6u64)
                            .unwrap();
                        let asset_amount = asset_share.amount
                            / Uint128::new(10u64.pow(exponent_difference as u32) as u128);
                        let decimal_asset_amount =
                            Decimal::from_ratio(asset_amount, Uint128::new(1u128));

                        //Price * # of assets in LP shares
                        value += decimal_multiplication(price, decimal_asset_amount);
                    }

                    value
                };

                //Calculate LP price
                let cAsset_price = {
                    let share_amount =
                        Decimal::from_ratio(cAsset.asset.amount, Uint128::new(1u128));
                    if !share_amount.is_zero() {
                        decimal_division(cAsset_value, share_amount)
                    } else {
                        Decimal::zero()
                    }
                };

                //Push to price and value list
                cAsset_prices.push(cAsset_price);
                cAsset_values.push(cAsset_value);
            } else {
                let price = query_price(
                    storage,
                    querier,
                    env.clone(),
                    config.clone(),
                    cAsset.clone().asset.info,
                    basket_id,
                )?;

                cAsset_prices.push(price);
                let collateral_value = decimal_multiplication(
                    Decimal::from_ratio(cAsset.asset.amount, Uint128::new(1u128)),
                    price,
                );
                cAsset_values.push(collateral_value);
            }
        }
    }

    Ok((cAsset_values, cAsset_prices))
}

pub fn update_position_claims(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: Addr,
    liquidated_asset: AssetInfo,
    liquidated_amount: Uint128,
) -> StdResult<()> {
    POSITIONS.update(
        storage,
        (basket_id.to_string(), position_owner),
        |old_positions| -> StdResult<Vec<Position>> {
            match old_positions {
                Some(old_positions) => {
                    let new_positions = old_positions
                        .into_iter()
                        .map(|mut position| {
                            //Find position
                            if position.position_id == position_id {
                                //Find asset in position
                                position.collateral_assets = position
                                    .collateral_assets
                                    .into_iter()
                                    .map(|mut c_asset| {
                                        //Subtract amount liquidated from claims
                                        if c_asset.asset.info.equal(&liquidated_asset) {
                                            c_asset.asset.amount -= liquidated_amount;
                                        }

                                        c_asset
                                    })
                                    .collect::<Vec<cAsset>>();
                            }
                            position
                        })
                        .collect::<Vec<Position>>();

                    Ok(new_positions)
                }
                None => {
                    return Err(StdError::GenericErr {
                        msg: "Invalid position owner".to_string(),
                    })
                }
            }
        },
    )?;

    //Subtract liquidated amount from total asset tally
    let collateral_assets = vec![cAsset {
        asset: Asset {
            info: liquidated_asset,
            amount: liquidated_amount,
        },
        max_borrow_LTV: Decimal::zero(),
        max_LTV: Decimal::zero(),
        pool_info: None,
    }];

    let mut basket = BASKETS.load(storage, basket_id.to_string())?;
    match update_basket_tally(storage, querier, env, &mut basket, collateral_assets, false) {
        Ok(_res) => {
            BASKETS.save(storage, basket_id.to_string(), &basket)?;
        }
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: err.to_string(),
            })
        }
    };

    Ok(())
}

fn get_basket_debt_caps(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    //These are Basket specific fields
    basket: Basket,
) -> Result<Vec<Uint128>, ContractError> {
    let config: Config = CONFIG.load(storage)?;

    //Map supply caps to cAssets to get new ratios
    //The functions need Asset and Pool Info to calc value
    //Bc our LPs are aggregated w/ their paired assets, we don't need Pool Info
    let temp_cAssets: Vec<cAsset> = basket
        .clone()
        .collateral_supply_caps
        .into_iter()
        .map(|cap| {
            if cap.lp {
                //We skip LPs bc we don't want to double count their assets
                cAsset {
                    asset: Asset {
                        info: cap.asset_info,
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::zero(),
                    max_LTV: Decimal::zero(),
                    pool_info: None,
                }
            } else {
                cAsset {
                    asset: Asset {
                        info: cap.asset_info,
                        amount: cap.current_supply,
                    },
                    max_borrow_LTV: Decimal::zero(),
                    max_LTV: Decimal::zero(),
                    pool_info: None,
                }
            }
        })
        .collect::<Vec<cAsset>>();

    //Get the Basket's asset ratios
    let cAsset_ratios = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        temp_cAssets.clone(),
        config.clone(),
    )?;

    //Get credit_asset's liquidity_multiplier
    let credit_asset_multiplier = get_credit_asset_multiplier(
        storage,
        querier,
        env.clone(),
        config.clone(),
        basket.clone(),
    )?;

    //Get the base debt cap
    let mut debt_cap =
        get_asset_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?
            * credit_asset_multiplier;

    //Add SP liquidity to the cap
    debt_cap +=
        get_stability_pool_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?;

    //If debt cap is less than the minimum, set it to the minimum
    if debt_cap < (config.base_debt_cap_multiplier * config.debt_minimum) {
        debt_cap = (config.base_debt_cap_multiplier * config.debt_minimum);
    }

    let mut per_asset_debt_caps = vec![];

    for (i, cAsset) in cAsset_ratios.clone().into_iter().enumerate() {
        if !basket.clone().collateral_supply_caps[i].lp {
            // If supply cap is 0, then debt cap is 0
            if basket.clone().collateral_supply_caps != vec![] {
                if basket.clone().collateral_supply_caps[i]
                    .supply_cap_ratio
                    .is_zero()
                {
                    per_asset_debt_caps.push(Uint128::zero());
                } else {
                    per_asset_debt_caps.push(cAsset * debt_cap);
                }
            } else {
                per_asset_debt_caps.push(cAsset * debt_cap);
            }
        }
    }

    Ok(per_asset_debt_caps)
}

//Get total pooled amount for an asset
pub fn get_stability_pool_liquidity(
    querier: QuerierWrapper,
    config: Config,
    pool_asset: AssetInfo,
) -> StdResult<Uint128> {
    if let Some(sp_addr) = config.clone().stability_pool {
        //Query the SP Asset Pool
        Ok(querier
            .query::<PoolResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: sp_addr.to_string(),
                msg: to_binary(&SP_QueryMsg::AssetPool {
                    asset_info: pool_asset,
                })?,
            }))?
            .credit_asset
            .amount)
    } else {
        Ok(Uint128::zero())
    }
}

fn get_credit_asset_multiplier(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    basket: Basket,
) -> StdResult<Decimal> {
    //Find Baskets with similar credit_asset
    let mut baskets: Vec<Basket> = vec![];

    //Has to be done ugly due to an immutable borrow
    //Uint128 to int
    let range: i32 = config.current_basket_id.to_string().parse().unwrap();

    for basket_id in 1..range {
        let stored_basket = BASKETS.load(storage, basket_id.to_string())?;

        if stored_basket
            .credit_asset
            .info
            .equal(&basket.credit_asset.info)
        {
            baskets.push(stored_basket);
        }
    }

    //Calc collateral_type totals
    let mut collateral_totals: Vec<Asset> = vec![];

    for basket in baskets {
        //Find collateral's corresponding total in list
        for collateral in basket.collateral_supply_caps {
            if !collateral.lp {
                if let Some((index, _total)) = collateral_totals
                    .clone()
                    .into_iter()
                    .enumerate()
                    .find(|(_i, asset)| asset.info.equal(&collateral.asset_info))
                {
                    //Add to collateral total
                    collateral_totals[index].amount += collateral.current_supply;
                } else {
                    //Add collateral type to list
                    collateral_totals.push(Asset {
                        info: collateral.asset_info,
                        amount: collateral.current_supply,
                    });
                }
            }
        }
    }

    //Get total_collateral_value
    let temp_cAssets: Vec<cAsset> = collateral_totals
        .clone()
        .into_iter()
        .map(|asset| cAsset {
            asset,
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
        })
        .collect::<Vec<cAsset>>();

    let total_collateral_value: Decimal = get_asset_values(
        storage,
        env.clone(),
        querier,
        temp_cAssets,
        config.clone(),
        None,
    )?
    .0
    .into_iter()
    .sum();

    //Get basket_collateral_value
    let temp_cAssets: Vec<cAsset> = basket
        .clone()
        .collateral_supply_caps
        .into_iter()
        .map(|cap| cAsset {
            asset: Asset {
                info: cap.asset_info,
                amount: cap.current_supply,
            },
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
        })
        .collect::<Vec<cAsset>>();

    let basket_collateral_value: Decimal = get_asset_values(
        storage,
        env.clone(),
        querier,
        temp_cAssets,
        config.clone(),
        None,
    )?
    .0
    .into_iter()
    .sum();

    //Find Basket parameter's ratio of total collateral
    let basket_tvl_ratio: Decimal = {
        if !basket_collateral_value.is_zero() {
            decimal_division(total_collateral_value, basket_collateral_value)
        } else {
            Decimal::zero()
        }
    };

    //Get credit_asset's liquidity multiplier
    let credit_asset_liquidity_multiplier =
        CREDIT_MULTI.load(storage, basket.clone().credit_asset.info.to_string())?;

    //Get Minimum between (ratio * credit_asset's multiplier) and basket's liquidity_multiplier
    let multiplier = min(
        decimal_multiplication(basket_tvl_ratio, credit_asset_liquidity_multiplier),
        basket.liquidity_multiplier,
    );

    Ok(multiplier)
}

pub fn get_asset_liquidity(
    querier: QuerierWrapper,
    config: Config,
    asset_info: AssetInfo,
) -> StdResult<Uint128> {
    if config.clone().liquidity_contract.is_some() {
        let total_pooled: Uint128 = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().liquidity_contract.unwrap().to_string(),
            msg: to_binary(&LiquidityQueryMsg::Liquidity { asset: asset_info })?,
        }))?;

        Ok(total_pooled)
    } else {
        return Err(StdError::GenericErr {
            msg: "No proxy contract setup".to_string(),
        });
    }
}

fn update_debt_per_asset_in_position(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    basket_id: Uint128,
    old_assets: Vec<cAsset>,
    new_assets: Vec<cAsset>,
    credit_amount: Decimal,
) -> Result<(), ContractError> {
    let mut basket: Basket = match BASKETS.load(storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    let new_old_assets = get_LP_pool_cAssets(querier, config.clone(), basket.clone(), old_assets)?;
    let new_new_assets = get_LP_pool_cAssets(querier, config.clone(), basket.clone(), new_assets)?;

    //Note: Vec lengths need to match
    let old_ratios = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        new_old_assets.clone(),
        config.clone(),
    )?;
    let new_ratios = get_cAsset_ratios(storage, env.clone(), querier, new_new_assets, config)?;

    let mut over_cap = false;
    let mut assets_over_cap = vec![];

    //Calculate debt per asset caps
    let cAsset_caps = get_basket_debt_caps(storage, querier, env, basket.clone())?;

    for i in 0..old_ratios.len() {
        match old_ratios[i].atomics().checked_sub(new_ratios[i].atomics()) {
            Ok(difference) => {
                //Old was > than New
                //So we subtract the % difference in debt from said asset

                basket.collateral_supply_caps = basket
                    .clone()
                    .collateral_supply_caps
                    .into_iter()
                    .filter(|cap| !cap.lp) //We don't take LP supply caps when calculating debt
                    .map(|mut cap| {
                        if cap.asset_info.equal(&new_old_assets[i].asset.info) {
                            match cap.debt_total.checked_sub(
                                decimal_multiplication(Decimal::new(difference), credit_amount)
                                    * Uint128::new(1u128),
                            ) {
                                Ok(difference) => {
                                    if cap.current_supply.is_zero() {
                                        //This removes rounding errors that would slowly increase resting interest rates
                                        //Doesn't effect checks for bad debt since its basket debt not position.credit_amount
                                        //its a .000001 error, so shouldn't effect overall calcs and shouldn't be profitably spammable
                                        cap.debt_total = Uint128::zero();
                                    } else {
                                        cap.debt_total = difference;
                                    }
                                }
                                Err(_) => {
                                    //Can't return an Error here without inferring the map return type
                                }
                            };
                        }

                        cap
                    })
                    .collect::<Vec<SupplyCap>>();
            }
            Err(_) => {
                //Old was < than New
                //So we add the % difference in debt to said asset
                let difference = new_ratios[i] - old_ratios[i];

                basket.collateral_supply_caps = basket
                    .clone()
                    .collateral_supply_caps
                    .into_iter()
                    .enumerate()
                    .filter(|cap| !cap.1.lp) //We don't take LP supply caps when calculating debt
                    .map(|(index, mut cap)| {
                        if cap.asset_info.equal(&new_old_assets[i].asset.info) {
                            let asset_debt = decimal_multiplication(difference, credit_amount)
                                * Uint128::new(1u128);

                            //Assert its not over the cap
                            if (cap.debt_total + asset_debt) <= cAsset_caps[index] {
                                cap.debt_total += asset_debt;
                            } else {
                                over_cap = true;
                                assets_over_cap.push(cap.asset_info.to_string());
                            }
                        }

                        cap
                    })
                    .collect::<Vec<SupplyCap>>();
            }
        }
    }

    if over_cap {
        return Err(ContractError::CustomError {
            val: format!("Assets over debt cap: {:?}", assets_over_cap),
        });
    }

    BASKETS.save(storage, basket_id.to_string(), &basket)?;

    Ok(())
}

fn update_basket_debt(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    basket: &mut Basket,
    collateral_assets: Vec<cAsset>,
    credit_amount: Uint128,
    add_to_debt: bool,
    interest_accrual: bool,
) -> Result<(), ContractError> {

    let collateral_assets =
        get_LP_pool_cAssets(querier, config.clone(), basket.clone(), collateral_assets)?;

    let cAsset_ratios = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        collateral_assets.clone(),
        config,
    )?;

    let mut asset_debt = vec![];

    //Save the debt distribution per asset to a Vec
    for asset in cAsset_ratios {
        asset_debt.push(asset * credit_amount);
    }
    
    let mut over_cap = false;
    let mut assets_over_cap = vec![];

    //Calculate debt per asset caps
    let cAsset_caps = get_basket_debt_caps(storage, querier, env, basket.clone())?;

    //Update supply caps w/ new debt distribution
    for (index, cAsset) in collateral_assets.iter().enumerate() {
        basket.collateral_supply_caps = basket
            .clone()
            .collateral_supply_caps
            .into_iter()
            .enumerate()
            .filter(|cap| !cap.1.lp) //We don't take LP supply caps when calculating debt
            .map(|(i, mut cap)| {
                //Add or subtract deposited amount to/from the correlated cAsset object
                if cap.asset_info.equal(&cAsset.asset.info) {
                    if add_to_debt {
                        //Assert its not over the cap
                        //IF the debt is adding to interest then we allow it to exceed the cap
                        if (cap.debt_total + asset_debt[index]) <= cAsset_caps[i]
                            || interest_accrual
                        {
                            cap.debt_total += asset_debt[index];
                        } else {
                            over_cap = true;
                            assets_over_cap.push(cap.asset_info.to_string());
                        }
                    } else {
                        match cap.debt_total.checked_sub(asset_debt[index]) {
                            Ok(difference) => {
                                cap.debt_total = difference;
                            }
                            Err(_) => {
                                //Don't subtract bc it'll end up being an invalid repayment error anyway
                                //Can't return an Error here without inferring the map return type
                            }
                        };
                    }
                }

                cap
            })
            .collect::<Vec<SupplyCap>>();       

    }

    //Error if over the asset cap
    if over_cap {
        return Err(ContractError::CustomError {
            val: format!(
                "This increase of debt sets [ {:?} ] assets above the protocol debt cap",
                assets_over_cap
            ),
        });
    }

    Ok(())
}

pub fn get_target_position(
    storage: &dyn Storage,
    basket_id: Uint128,
    valid_position_owner: Addr,
    position_id: Uint128,
) -> Result<Position, ContractError> {
    let positions: Vec<Position> = match POSITIONS.load(
        storage,
        (basket_id.to_string(), valid_position_owner.clone()),
    ) {
        Err(_) => return Err(ContractError::NoUserPositions {}),
        Ok(positions) => positions,
    };

    match positions.into_iter().find(|x| x.position_id == position_id) {
        Some(position) => Ok(position),
        None => return Err(ContractError::NonExistentPosition {}),
    }
}

fn create_denom(
    config: Config,
    subdenom: String,
    basket_id: String,
    liquidity_multiplier: Option<Decimal>,
) -> StdResult<SubMsg> {
    if config.osmosis_proxy.is_some() {
        let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.osmosis_proxy.unwrap().to_string(),
            msg: to_binary(&OsmoExecuteMsg::CreateDenom {
                subdenom,
                basket_id,
                max_supply: Some(Uint128::new(u128::MAX)),
                liquidity_multiplier,
            })?,
            funds: vec![],
        });

        return Ok(SubMsg::reply_on_success(message, CREATE_DENOM_REPLY_ID));
    }
    return Err(StdError::GenericErr {
        msg: "No osmosis proxy added to the config yet".to_string(),
    });
}

pub fn accumulate_interest(debt: Uint128, rate: Decimal, time_elapsed: u64) -> StdResult<Uint128> {
    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    let accrued_interest = debt * applied_rate;

    Ok(accrued_interest)
}

fn get_interest_rates(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
) -> StdResult<Vec<(AssetInfo, Decimal)>> {
    let config = CONFIG.load(storage)?;

    let mut rates = vec![];

    for asset in basket.clone().collateral_types {
        //We don't get individual rates for LPs
        if asset.pool_info.is_none() {
            //Base_Rate * max collateral_ratio
            //ex: 2% * 110% = 2.2%
            //Higher rates for riskier assets

            //base * (1/max_LTV)
            rates.push(decimal_multiplication(
                basket.clone().base_interest_rate,
                decimal_division(Decimal::one(), asset.max_LTV),
            ));
        }
    }

    //Get proportion of debt && supply caps filled
    let mut debt_proportions = vec![];
    let mut supply_proportions = vec![];

    let debt_caps = match get_basket_debt_caps(storage, querier, env.clone(), basket.clone()) {
        Ok(caps) => caps,
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: err.to_string(),
            })
        }
    };

    //To include LP assets (but not share tokens) in the ratio calculation
    let caps_to_cAssets = basket
        .collateral_supply_caps
        .clone()
        .into_iter()
        .map(|cap| cAsset {
            asset: Asset {
                amount: cap.current_supply,
                info: cap.asset_info,
            },
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
        })
        .collect::<Vec<cAsset>>();

    let no_lp_basket: Vec<cAsset> =
        get_LP_pool_cAssets(querier, config.clone(), basket.clone(), caps_to_cAssets)?;

    //Get basket cAsset ratios
    let basket_ratios: Vec<Decimal> =
        get_cAsset_ratios(storage, env.clone(), querier, no_lp_basket, config.clone())?;

    let no_lp_caps = basket
        .collateral_supply_caps
        .clone()
        .into_iter()
        .filter(|cap| !cap.lp)
        .collect::<Vec<SupplyCap>>();

    for (i, cap) in no_lp_caps.clone().iter().enumerate() {
        //If there is 0 of an Asset then it's cap is 0 but its proportion is 100%
        if debt_caps[i].is_zero() || cap.supply_cap_ratio.is_zero() {
            debt_proportions.push(Decimal::percent(100));
            supply_proportions.push(Decimal::percent(100));
        } else {
            //Push the debt_ratio and supply_ratio
            debt_proportions.push(Decimal::from_ratio(cap.debt_total, debt_caps[i]));
            supply_proportions.push(decimal_division(basket_ratios[i], cap.supply_cap_ratio))
        }
    }

    //Gets pro-rata rate and uses multiplier if above desired utilization
    let mut two_slope_pro_rata_rates = vec![];
    for (i, _rate) in rates.iter().enumerate() {
        //If proportions are above desired utilization, the rates start multiplying
        //For every % above the desired, it adds a multiple
        //Ex: Desired = 90%, proportion = 91%, interest = 2%. New rate = 4%.
        //Acts as two_slope rate

        //The highest proportion is chosen between debt_cap and supply_cap of the asset
        if debt_proportions[i] > supply_proportions[i] {
            //Slope 2
            if debt_proportions[i] > basket.desired_debt_cap_util {
                //Ex: 91% > 90%
                ////0.01 * 100 = 1
                //1% = 1
                let percent_over_desired = decimal_multiplication(
                    decimal_subtraction(debt_proportions[i], basket.desired_debt_cap_util),
                    Decimal::percent(100_00),
                );
                let multiplier = percent_over_desired + Decimal::one();
                //Change rate of (rate) increase w/ the configuration multiplier
                let multiplier = multiplier * config.rate_slope_multiplier;

                //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
                //// rate = 3.6%
                two_slope_pro_rata_rates.push((
                    no_lp_caps[i].clone().asset_info,
                    decimal_multiplication(
                        decimal_multiplication(rates[i], debt_proportions[i]),
                        multiplier,
                    ),
                ));
            } else {
                //Slope 1
                two_slope_pro_rata_rates.push((
                    no_lp_caps[i].clone().asset_info,
                    decimal_multiplication(rates[i], debt_proportions[i]),
                ));
            }
        } else {
            //Slope 2
            if supply_proportions[i] > Decimal::one() {
                //Ex: 91% > 90%
                ////0.01 * 100 = 1
                //1% = 1
                let percent_over_desired = decimal_multiplication(
                    decimal_subtraction(supply_proportions[i], Decimal::one()),
                    Decimal::percent(100_00),
                );
                let multiplier = percent_over_desired + Decimal::one();
                //Change rate of (rate) increase w/ the configuration multiplier
                let multiplier = multiplier * config.rate_slope_multiplier;

                //Ex cont: Multiplier = 2; Pro_rata rate = 1.8%.
                //// rate = 3.6%
                two_slope_pro_rata_rates.push((
                    no_lp_caps[i].clone().asset_info,
                    decimal_multiplication(
                        decimal_multiplication(rates[i], supply_proportions[i]),
                        multiplier,
                    ),
                ));
            } else {
                //Slope 1
                two_slope_pro_rata_rates.push((
                    no_lp_caps[i].clone().asset_info,
                    decimal_multiplication(rates[i], supply_proportions[i]),
                ));
            }
        }
    }

    Ok(two_slope_pro_rata_rates)
}

//If cAssets include an LP, remove the LP share denom and add its paired assets
pub fn get_LP_pool_cAssets(
    querier: QuerierWrapper,
    config: Config,
    basket: Basket,
    position_assets: Vec<cAsset>,
) -> StdResult<Vec<cAsset>> {
    let mut new_assets = position_assets
        .clone()
        .into_iter()
        .filter(|asset| asset.pool_info.is_none())
        .collect::<Vec<cAsset>>();

    //Add LP's Assets as cAssets
    //Remove LP share token
    for cAsset in position_assets.clone() {
        if cAsset.pool_info.is_some() {
            let pool_info = cAsset.pool_info.unwrap();

            //Query share asset amount
            let share_asset_amounts = querier
                .query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoQueryMsg::PoolState {
                        id: pool_info.pool_id,
                    })?,
                }))?
                .shares_value(cAsset.asset.amount);

            for pool_coin in share_asset_amounts {
                let info = AssetInfo::NativeToken {
                    denom: pool_coin.denom,
                };
                //Find the coin in the basket
                if let Some(basket_cAsset) = basket
                    .clone()
                    .collateral_types
                    .into_iter()
                    .find(|cAsset| cAsset.asset.info.equal(&info))
                {
                    //Check if its already in the position asset list
                    if let Some((i, _cAsset)) =
                        new_assets
                            .clone()
                            .into_iter()
                            .enumerate()
                            .find(|(_index, cAsset)| {
                                cAsset.asset.info.equal(&basket_cAsset.clone().asset.info)
                            })
                    {
                        //Add to assets
                        new_assets[i].asset.amount += pool_coin.amount;
                    } else {
                        //Push to list
                        new_assets.push(cAsset {
                            asset: Asset {
                                amount: pool_coin.amount,
                                info,
                            },
                            ..basket_cAsset
                        })
                    }
                }
                //No reason to error bc LPs can't be added if their assets aren't added first
            }
        }
    }

    Ok(new_assets)
}

fn get_position_avg_rate(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    position_assets: Vec<cAsset>,
) -> StdResult<Decimal> {
    let config = CONFIG.load(storage)?;

    let new_assets = get_LP_pool_cAssets(querier, config.clone(), basket.clone(), position_assets)?;

    let ratios = get_cAsset_ratios(storage, env.clone(), querier, new_assets.clone(), config)?;

    let interest_rates = get_interest_rates(storage, querier, env, basket)?;

    let mut avg_rate = Decimal::zero();

    for (i, cAsset) in new_assets.clone().iter().enumerate() {
        //Match asset and rate
        if let Some(rate) = interest_rates
            .clone()
            .into_iter()
            .find(|rate| rate.0.equal(&cAsset.asset.info))
        {
            avg_rate += decimal_multiplication(ratios[i], rate.1);
        }
    }

    Ok(avg_rate)
}

pub fn accrue(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    position: &mut Position,
    basket: &mut Basket,
) -> StdResult<()> {
    let config = CONFIG.load(storage)?;

    //Accrue Interest to the Repayment Price
    //--
    //Calc Time-elapsed and update last_Accrued
    let mut time_elapsed = env.block.time.seconds() - basket.credit_last_accrued;

    let mut negative_rate: bool = false;
    let mut price_difference: Decimal = Decimal::zero();

    ////Controller barriers to reduce risk of manipulation
    //Liquidity above 2M
    //At least 3% of total supply as liquidity
    let liquidity = get_asset_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?;
    //Now get % of supply
    if config.clone().osmosis_proxy.is_some() {
        let current_supply = querier
            .query::<TokenInfoResponse>(&QueryRequest::Wasm(
                (WasmQuery::Smart {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoQueryMsg::GetTokenInfo {
                        denom: basket.clone().credit_asset.info.to_string(),
                    })?,
                }),
            ))?
            .current_supply;

        let liquidity_ratio = decimal_division(
            Decimal::from_ratio(liquidity, Uint128::new(1u128)),
            Decimal::from_ratio(current_supply, Uint128::new(1u128)),
        );
        if liquidity_ratio < Decimal::percent(3) {
            //Set time_elapsed to 0 to skip accrual
            time_elapsed = 0u64;
        }
    }
    if liquidity < Uint128::new(2_000_000_000_000u128) {
        //Set time_elapsed to 0 to skip repayment accrual
        time_elapsed = 0u64;
    }

    if !(time_elapsed == 0u64) && basket.oracle_set {
        basket.credit_last_accrued = env.block.time.seconds();

        //Calculate new interest rate
        let credit_asset = cAsset {
            asset: basket.clone().credit_asset,
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info: None,
        };

        let credit_TWAP_price = get_asset_values(
            storage,
            env.clone(),
            querier,
            vec![credit_asset],
            config.clone(),
            Some(basket.clone().basket_id),
        )?
        .1[0];

        //We divide w/ the greater number first so the quotient is always 1.__
        price_difference = {
            //If market price > than repayment price
            if credit_TWAP_price > basket.clone().credit_price {
                negative_rate = true;
                decimal_subtraction(
                    decimal_division(credit_TWAP_price, basket.clone().credit_price),
                    Decimal::one(),
                )
            } else if basket.clone().credit_price > credit_TWAP_price {
                negative_rate = false;
                decimal_subtraction(
                    decimal_division(basket.clone().credit_price, credit_TWAP_price),
                    Decimal::one(),
                )
            } else {
                negative_rate = false;
                Decimal::zero()
            }
        };

        // /

        //Don't accrue interest if price is within the margin of error
        if price_difference > config.clone().cpc_margin_of_error {
            price_difference =
                decimal_subtraction(price_difference, config.clone().cpc_margin_of_error);

            //Calculate rate of change
            let mut applied_rate: Decimal;
            applied_rate = price_difference.checked_mul(Decimal::from_ratio(
                Uint128::from(time_elapsed),
                Uint128::from(SECONDS_PER_YEAR),
            ))?;

            //If a positive rate we add 1,
            //If a negative rate we subtract the applied_rate from 1
            //---
            if negative_rate {
                //Subtract applied_rate to make it .9___
                applied_rate = decimal_subtraction(Decimal::one(), applied_rate);
            } else {
                //Add 1 to make the value 1.__
                applied_rate += Decimal::one();
            }

            let mut new_price = basket.credit_price;
            //Negative repayment interest needs to be enabled by the basket
            if negative_rate && basket.negative_rates || !negative_rate {
                new_price = decimal_multiplication(basket.credit_price, applied_rate);
            } 

            basket.credit_price = new_price;
        } else {
            price_difference = Decimal::zero();
        }
    }

    /////Accrue interest to the debt
    //Calc time-elapsed
    let time_elapsed = env.clone().block.time.seconds() - position.last_accrued;
    //Update last accrued time
    position.last_accrued = env.clone().block.time.seconds();

    //Calc avg_rate for the position
    let mut avg_rate = get_position_avg_rate(
        storage,
        querier,
        env.clone(),
        basket,
        position.clone().collateral_assets,
    )?;

    //Accrue a years worth of repayment rate to interest rates
    //These aren't saved so it won't compound
    if negative_rate {
        avg_rate = decimal_multiplication(
            avg_rate,
            decimal_subtraction(Decimal::one(), price_difference),
        );
    } else {
        avg_rate = decimal_multiplication(avg_rate, (Decimal::one() + price_difference));
    }

    //Calc accrued interested
    let accrued_interest = accumulate_interest(position.credit_amount, avg_rate, time_elapsed)?;

    //Add accrued interest to the position's debt
    position.credit_amount += accrued_interest * Uint128::new(1u128);

    //Add accrued interest to the basket's pending revenue
    //Okay with rounding down here since the position's credit will round down as well
    basket.pending_revenue += accrued_interest * Uint128::new(1u128);

    //Add accrued interest to the basket's debt cap
    match update_basket_debt(
        storage,
        env.clone(),
        querier,
        config.clone(),
        basket,
        position.clone().collateral_assets,
        accrued_interest * Uint128::new(1u128),
        true,
        true,
    ) {
        Ok(_ok) => {}
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: err.to_string(),
            })
        }
    };

    Ok(())
}

pub fn mint_revenue(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    basket_id: Uint128,
    send_to: Option<String>,
    repay_for: Option<UserInfo>,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    
    //Can't send_to and repay_for at the same time
    if send_to.is_some() && repay_for.is_some() {
        return Err(ContractError::CustomError {
            val: String::from("Can only send to one address at a time"),
        });
    }

    let config = CONFIG.load(deps.storage)?;

    let mut basket = BASKETS.load(deps.storage, basket_id.to_string())?;

    if info.sender != config.owner && info.sender != basket.owner {
        return Err(ContractError::Unauthorized {});
    }

    if basket.pending_revenue.is_zero() {
        return Err(ContractError::CustomError {
            val: String::from("No revenue to mint"),
        });
    }

    //Set amount
    let amount = amount.unwrap_or_else(|| basket.pending_revenue);

    //Subtract amount from pending revenue
    basket.pending_revenue = match basket.pending_revenue.checked_sub(amount) {
        Ok(new_balance) => new_balance,
        Err(err) => {
            return Err(ContractError::CustomError {
                val: err.to_string(),
            })
        }
    }; //Save basket
    BASKETS.save(deps.storage, basket_id.to_string(), &basket)?;

    let mut message: Vec<CosmosMsg> = vec![];
    let mut repay_attr = String::from("None");

    //If send to is_some
    if send_to.is_some() {
        message.push(credit_mint_msg(
            config.clone(),
            Asset {
                amount,
                ..basket.credit_asset.clone()
            }, //Send_to or interest_collector or config.owner
            deps.api
                .addr_validate(&send_to.clone().unwrap())
                .unwrap_or(config.interest_revenue_collector.unwrap_or(basket.owner)),
        )?);
    } else if repay_for.is_some() {
        repay_attr = repay_for.clone().unwrap().to_string();

        //Need to mint credit to the contract
        message.push(credit_mint_msg(
            config.clone(),
            Asset {
                amount,
                ..basket.credit_asset.clone()
            },
            env.clone().contract.address,
        )?);

        //and then send it for repayment
        let msg = ExecuteMsg::Repay {
            basket_id: repay_for.clone().unwrap().basket_id,
            position_id: repay_for.clone().unwrap().position_id,
            position_owner: Some(repay_for.unwrap().position_owner),
        };

        message.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_binary(&msg)?,
            funds: vec![coin(amount.u128(), basket.credit_asset.info.to_string())],
        }));
    } else {
        //Mint to the interest collector
        //or to the basket.owner if not
        message.push(credit_mint_msg(
            config.clone(),
            Asset {
                amount,
                ..basket.credit_asset
            },
            config.interest_revenue_collector.unwrap_or(basket.owner),
        )?);
    }

    Ok(Response::new().add_messages(message).add_attributes(vec![
        attr("basket", basket_id.to_string()),
        attr("amount", amount.to_string()),
        attr("repay_for", repay_attr),
        attr("send_to", send_to.unwrap_or(String::from("None"))),
    ]))
}

fn assert_credit_asset(
    basket: Basket,
    credit_asset: Asset,
    msg_sender: Addr,
)-> Result<(), ContractError>{
    match credit_asset.clone().info {
        AssetInfo::Token {
            address: submitted_address,
        } => {
            if let AssetInfo::Token { address } = basket.clone().credit_asset.info {
                if submitted_address != address || msg_sender.clone() != address {
                    return Err(ContractError::InvalidCredit {})
                }
            } else {
                return Err(ContractError::InvalidCredit {})
            }
        }
        AssetInfo::NativeToken {
            denom: submitted_denom,
        } => {
            if let AssetInfo::NativeToken { denom } = basket.clone().credit_asset.info {
                if submitted_denom != denom {
                    return Err(ContractError::InvalidCredit {})
                }
            } else {
                return Err(ContractError::InvalidCredit {})
            }
        }
    }

    Ok(())
}