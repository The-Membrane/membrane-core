use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128, Storage, QuerierWrapper, Env, StdResult, StdError, Binary};
use cw_storage_plus::{Item, Map};

use membrane::types::{Asset, Basket, Position, RedemptionInfo, UserInfo, AssetInfo, cAsset};
use membrane::cdp::Config;

use crate::ContractError;
use crate::risk_engine::update_basket_tally;


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiquidationPropagation {
    pub per_asset_repayment: Vec<Decimal>,
    pub liq_queue_leftovers: Decimal, //List of repayments
    pub stability_pool: Decimal,      //Value of repayment
    pub user_repay_amount: Decimal,
    pub positions_contract: Addr,
    //So the sell wall knows who to repay to
    pub position_info: UserInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WithdrawPropagation {
    pub positions_prev_collateral: Vec<Asset>, //Amount of collateral in the position before the withdrawal
    pub withdraw_amounts: Vec<Uint128>,
    pub contracts_prev_collateral_amount: Vec<Uint128>,
    pub position_info: UserInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ClosePositionPropagation {
    pub withdrawn_assets: Vec<Asset>,
    pub position_info: UserInfo,
    pub send_to: Option<String>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const BASKET: Item<Basket> = Item::new("basket"); 
pub const POSITIONS: Map<Addr, Vec<Position>> = Map::new("positions"); //owner, list of positions

/// CDT redemption premium, opt-in mechanism.
/// This is the premium that the user will pay to redeem their debt token.
pub const REDEMPTION_OPT_IN: Map<u128, Vec<RedemptionInfo>> = Map::new("redemption_opt_in"); 

/// Config ownership transfer
pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");

//Reply State Propagations
pub const WITHDRAW: Item<WithdrawPropagation> = Item::new("withdraw_propagation");
pub const LIQUIDATION: Item<LiquidationPropagation> = Item::new("repay_propagation");
pub const CLOSE_POSITION: Item<ClosePositionPropagation> = Item::new("close_position_propagation");
pub const ROUTER_REPAY_MSG: Item<Vec<Binary>> = Item::new("router_repay_msg");


//Helper functions
/// Update asset claims a Position has
pub fn update_position_claims(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    position_id: Uint128,
    position_owner: Addr,
    liquidated_asset: AssetInfo,
    liquidated_amount: Uint128,
) -> StdResult<()> {
    POSITIONS.update(
        storage,
        position_owner,
        |old_positions| -> StdResult<Vec<Position>> {
            if let Some(old_positions) = old_positions {
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
            } else {
                Err(StdError::GenericErr {
                    msg: "Invalid position owner".to_string(),
                })
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
        rate_index: Decimal::one(),
    }];

    let mut basket = BASKET.load(storage)?;    
    match update_basket_tally(storage, querier, env, &mut basket, collateral_assets, false) {
        Ok(_res) => {
            BASKET.save(storage, &basket)?;
        }
        Err(err) => {
            return Err(StdError::GenericErr {
                msg: err.to_string(),
            })
        }
    };

    Ok(())
}

/// Returns Position & index of Position in User's list
pub fn get_target_position(
    storage: &dyn Storage,
    valid_position_owner: Addr,
    position_id: Uint128,
) -> Result<(usize, Position), ContractError> {
    let positions: Vec<Position> = match POSITIONS.load(
        storage, valid_position_owner
    ){
        Err(_) => return Err(ContractError::NoUserPositions {}),
        Ok(positions) => positions,
    };

    match positions.into_iter().enumerate().find(|(_i, x)| x.position_id == position_id) {
        Some(position) => Ok(position),
        None => Err(ContractError::NonExistentPosition { id: position_id }),
    }
}


/// Replace Position data in state
pub fn update_position(
    storage: &mut dyn Storage,
    valid_position_owner: Addr,
    new_position: Position,
) -> StdResult<()>{

    POSITIONS.update(
        storage,
        valid_position_owner,
        |old_positions| -> StdResult<Vec<Position>> {
            match old_positions {
                Some(old_positions) => {
                    let new_positions = old_positions
                        .into_iter()
                        .map(|stored_position| {
                            //Find position
                            if stored_position.position_id == new_position.position_id {
                                //Swap to target_position 
                                new_position.clone()
                            } else {
                                //Don't override
                                stored_position
                            }
                        })
                        .collect::<Vec<Position>>();

                    Ok(new_positions)
                },
                None => {
                    Err(StdError::GenericErr {
                        msg: "Invalid position owner".to_string(),
                    })
                }
            }
        },
    )?;

    Ok(())
}
