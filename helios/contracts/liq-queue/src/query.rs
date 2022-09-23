use std::str::FromStr;


#[cfg(not(feature = "library"))]
use cosmwasm_std::{ Deps, StdResult, StdError, Uint128, Decimal};
use membrane::liq_queue::{SlotResponse, ConfigResponse, BidResponse, ClaimsResponse, LiquidatibleResponse, QueueResponse};
use membrane::types::{AssetInfo, Bid, Queue, PremiumSlot};
use membrane::math::{ Decimal256, Uint256};


use crate::state::{ CONFIG, Config, QUEUES };
//use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, SlotResponse, ConfigResponse, BidResponse, ClaimsResponse, LiquidatibleResponse, QueueResponse};
//use crate::positions::{ExecuteMsg as CDP_ExecuteMsg, Cw20HookMsg as CDP_Cw20HookMsg};
use crate::bid::{read_bid, read_bids_by_user, read_premium_slot, calculate_remaining_bid, calculate_liquidated_collateral };


pub fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    let resp = ConfigResponse {
        owner: config.owner.to_string(),
        positions_contract: config.positions_contract.to_string(),
        waiting_period: config.waiting_period,
        added_assets: config.added_assets.unwrap_or_default(),
    };

    Ok(resp)
}

pub fn query_queue(
    deps: Deps,
    bid_for: AssetInfo
)-> StdResult<QueueResponse>{

    let queue = QUEUES.load(deps.storage, bid_for.to_string())?;
    
    Ok(
        QueueResponse {
        bid_asset: queue.bid_asset.to_string(),
        max_premium: queue.max_premium.to_string(), 
        current_bid_id: queue.current_bid_id.to_string(),
        bid_threshold: queue.bid_threshold.to_string(),
        }
    )
        
}

pub fn query_queues(
    deps: Deps,
    start_after: Option<AssetInfo>,
    limit: Option<u8>,
)-> StdResult<Vec<QueueResponse>>{

    let config: Config = CONFIG.load(deps.storage)?;

    let mut resp: Vec<QueueResponse> = vec![];

    let asset_list = config.added_assets.clone().unwrap();

    let limit = limit.unwrap_or(31u8) as usize;

    if start_after.is_some(){
        let start_after = &start_after.unwrap();

        let start = asset_list
            .iter()
            .position(|info| info.equal(&start_after));
        let start = start.unwrap_or_default();

        for index in start..asset_list.len(){

            let queue = QUEUES.load(deps.storage, asset_list[index].to_string())?;

            resp.push(
                QueueResponse {
                    bid_asset: queue.bid_asset.to_string(),
                    max_premium: queue.max_premium.to_string(), 
                    current_bid_id: queue.current_bid_id.to_string(),
                    bid_threshold: queue.bid_threshold.to_string(),
                }
                );
        }

    }else{
       
        for asset in asset_list.iter().take(limit){

            let queue = QUEUES.load(deps.storage, asset.to_string())?;

            resp.push(
                QueueResponse {
                    bid_asset: queue.bid_asset.to_string(),
                    max_premium: queue.max_premium.to_string(), 
                    current_bid_id: queue.current_bid_id.to_string(),
                    bid_threshold: queue.bid_threshold.to_string(),
                });
        }
    }
    
    Ok( resp )
}

pub fn query_liquidatible(
    deps: Deps,
    // env: Env,
    bid_for: AssetInfo,
    collateral_price: Decimal,
    collateral_amount: Uint256,
    credit_info: AssetInfo,
    credit_price: Decimal,
)-> StdResult<LiquidatibleResponse>{
    
    let queue: Queue = match QUEUES.load(deps.storage, bid_for.to_string()){
        Err(_) => return Err(StdError::GenericErr { msg: "Queue for this asset doesn't exist".to_string() }),
        Ok( queue ) => { 
            if !queue.bid_asset.info.equal(&credit_info){
                return Err(StdError::GenericErr { msg: format!("Invalid bid denomination for {}", bid_for.to_string()) })
            }

            queue
        },
    };

    let mut remaining_collateral_to_liquidate = collateral_amount;
    let mut total_credit_repaid = Uint256::zero();

    for slot in queue.slots.into_iter(){

        if slot.total_bid_amount.is_zero() || remaining_collateral_to_liquidate.is_zero(){
            continue;
        }

        let slot_total = slot.total_bid_amount;

        let collateral_price: Decimal256 = match Decimal256::from_str(&collateral_price.to_string()){
            Ok( price ) => price,
            Err( err ) => return Err( StdError::GenericErr { msg: err.to_string() } ),
        };

        let credit_price: Decimal256 = match Decimal256::from_str(&credit_price.to_string()){
            Ok( price ) => price,
            Err( err ) => return Err( StdError::GenericErr { msg: err.to_string() } ),
        };


        //price * (1- premium)
        let premium_price: Decimal256 = collateral_price * (Decimal256::one() - slot.clone().liq_premium );

        //Amount = c_amount * (collateral price in stables)
        let mut slot_required_stable: Uint256 = (remaining_collateral_to_liquidate) * (premium_price / credit_price);


        if slot_required_stable > slot_total{
            slot_required_stable = slot_total;
            //slot_required_stable / premium_price
            let slot_collateral_to_liquidate: Uint256 = slot_required_stable/ premium_price;

            remaining_collateral_to_liquidate = remaining_collateral_to_liquidate - slot_collateral_to_liquidate;
        }else{
            remaining_collateral_to_liquidate = Uint256::zero();
        }

        //Track total_credit_repaid
        total_credit_repaid += slot_required_stable;

        
    }

    //If 0, it means there is no leftover and the collateral_amount is liquidatible
    return Ok( 
        LiquidatibleResponse{
            leftover_collateral: (remaining_collateral_to_liquidate.0.to_string()),
            total_credit_repaid: total_credit_repaid.to_string(),
        })
    
}


pub fn query_premium_slot(
    deps: Deps,
    bid_for: AssetInfo,
    premium: u64, //Taken as %
) -> StdResult<SlotResponse>{
    let queue = QUEUES.load(deps.storage, bid_for.to_string())?;

    let slot = match queue.slots.into_iter().find(|temp_slot| temp_slot.liq_premium == Decimal256::percent( premium )){
        Some( slot ) => { slot },
        None => { return Err(StdError::GenericErr { msg: "Invalid premium".to_string() })},
    };

    Ok( SlotResponse {
            bids: slot.bids,
            liq_premium: slot.liq_premium.to_string(),
            sum_snapshot: slot.sum_snapshot.to_string(),
            product_snapshot: slot.product_snapshot.to_string(),
            total_bid_amount: slot.total_bid_amount.to_string(),
            current_epoch: slot.current_epoch,
            current_scale: slot.current_scale,
            residue_collateral: slot.residue_collateral.to_string(),
            residue_bid: slot.residue_bid.to_string(),
        } )
}

pub fn query_premium_slots(
    deps: Deps,
    bid_for: AssetInfo,
    start_after: Option<u64>, //Start after a premium value taken as a % (ie 50 = 50%)
    limit: Option<u8>
) -> StdResult<Vec<SlotResponse>>{
    let queue = QUEUES.load(deps.storage, bid_for.to_string())?;

    let limit = limit.unwrap_or_else(|| queue.max_premium.u128() as u8).min(queue.max_premium.u128() as u8) as usize;

    let temp = queue.slots.into_iter();

    if start_after.is_some(){
        temp
            .filter(|slot| slot.liq_premium >Decimal256::from_uint256(Uint256::from(start_after.unwrap() as u128)))
            .take(limit)
            .map(|slot| 
                Ok( SlotResponse {
                    bids: slot.bids,
                    liq_premium: slot.liq_premium.to_string(),
                    sum_snapshot: slot.sum_snapshot.to_string(),
                    product_snapshot: slot.product_snapshot.to_string(),
                    total_bid_amount: slot.total_bid_amount.to_string(),
                    current_epoch: slot.current_epoch,
                    current_scale: slot.current_scale,
                    residue_collateral: slot.residue_collateral.to_string(),
                    residue_bid: slot.residue_bid.to_string(),
                } ))
            .collect::<StdResult<Vec<SlotResponse>>>()
    }else{
        temp
            .take(limit)
            .map(|slot| 
                Ok( SlotResponse {
                    bids: slot.bids,
                    liq_premium: slot.liq_premium.to_string(),
                    sum_snapshot: slot.sum_snapshot.to_string(),
                    product_snapshot: slot.product_snapshot.to_string(),
                    total_bid_amount: slot.total_bid_amount.to_string(),
                    current_epoch: slot.current_epoch,
                    current_scale: slot.current_scale,
                    residue_collateral: slot.residue_collateral.to_string(),
                    residue_bid: slot.residue_bid.to_string(),
                } ))
            .collect::<StdResult<Vec<SlotResponse>>>()
    }

}


pub fn query_bid(
    deps: Deps,
    bid_for: AssetInfo,
    bid_id: Uint128
) -> StdResult<BidResponse>{

    let bid: Bid = read_bid(deps.storage, bid_id, bid_for.clone() )?;
    let slot: PremiumSlot = match read_premium_slot(deps.storage, bid_for.clone(), bid.liq_premium){
        Ok( slot ) => slot,
        Err(_) => { return Err( StdError::GenericErr { msg: "Invalid premium".to_string() } ) },
    };

    let (bid_amount, bid_pending_liquidated_collateral) = if bid.wait_end.is_some() {
        (bid.amount, bid.pending_liquidated_collateral)
    } else {
        // calculate remaining bid amount
        let (remaining_bid, _) = calculate_remaining_bid(&bid, &slot)?;

        let bid_for = deps.api.addr_canonicalize(&bid_for.to_string())?;

        // calculate liquidated collateral
        let (liquidated_collateral, _) = calculate_liquidated_collateral(deps.storage, &bid, bid_for)?;

        (
            remaining_bid,
            bid.pending_liquidated_collateral + liquidated_collateral,
        )
    };

    Ok( BidResponse {
            user: bid.user.to_string(),
            id: bid.id,
            amount: bid_amount,
            liq_premium: bid.liq_premium,
            pending_liquidated_collateral: bid_pending_liquidated_collateral,
            product_snapshot: bid.product_snapshot,
            sum_snapshot: bid.sum_snapshot,
            wait_end: bid.wait_end,
            epoch_snapshot: bid.epoch_snapshot,
            scale_snapshot: bid.scale_snapshot,
    })
}

pub fn query_bids_by_user(
    deps: Deps,
    bid_for: AssetInfo,
    user: String,
    limit: Option<u32>,
    start_after: Option<Uint128>,
) -> StdResult<Vec<BidResponse>>{

    let valid_user = deps.api.addr_validate(&user)?;

    let user_bids = read_bids_by_user(deps.storage, bid_for.clone().to_string(), valid_user, limit, start_after)?;


    let responses = user_bids
        .into_iter()
        .map(|bid| {
            match query_bid(deps, bid_for.clone(), bid.id){
                Ok( res ) => { Ok( res ) },
                Err( err ) => {return Err( err ) }
            }
        }).collect::<StdResult<Vec<BidResponse>>>();

    responses 


}

pub fn query_user_claims(
    deps: Deps,
    user: String,
) -> StdResult<Vec<ClaimsResponse>>{

    let valid_user = deps.api.addr_validate(&user)?;

    let config: Config = CONFIG.load(deps.storage)?;

    let mut res_list: Vec<ClaimsResponse> = vec![];

    for asset in config.added_assets.unwrap(){ //Can unwrap bc added_assets is_some after instantiation
        
        let responses: Vec<BidResponse> = query_bids_by_user(deps, asset.clone(), valid_user.to_string(), None, None)?;

        let mut resp = ClaimsResponse {
            bid_for: asset.to_string(),
            pending_liquidated_collateral: Uint256::zero(),
        };

        for res in responses{
            resp.pending_liquidated_collateral += res.pending_liquidated_collateral 
        }

        res_list.push( resp );

    }

    Ok( res_list )

}

