use std::str::FromStr;

use cosmwasm_std::{attr, to_binary, CosmosMsg, Decimal, DepsMut, Env, Reply, Response, StdError, StdResult, Uint128, WasmMsg};

use membrane::managed_market::{Config, ExecuteMsg, MarketParams};
use membrane::types::{cAsset, Asset, AssetInfo, Basket};
use membrane::helpers::{asset_to_coin, get_contract_balances, withdrawal_msg};

use crate::positions::CDT_DENOM;
use crate::state::{LiquidationPropagation, CONFIG, LIQUIDATION, MARKET_PARAMS, POSITIONS};

//Liquidation reply
//Reply on success to:
// - edit the user position based on the CDT that was swapped for
// - edit the market's total borrowed.
#[allow(unused_variables)]
pub fn handle_liquidation_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(_result) => {
            //Load liquidation propogation
            let liquidation_propagation: LiquidationPropagation = LIQUIDATION.load(deps.storage)?;

            //Query the contract balance of the credit asset
            let current_cdt_balance = deps.querier.query_balance(env.contract.address, CDT_DENOM.to_string())?.amount;

            //Calculate the balance of newly acquired CDT
            let cdt_swapped_for = current_cdt_balance
                .checked_sub(liquidation_propagation.pre_liquidation_cdt_balance)
                .map_err(|_| StdError::generic_err("CDT balance went down"))?;

            //Load user state
            let mut user_position = POSITIONS.load(deps.storage, (liquidation_propagation.position_owner.clone(), liquidation_propagation.collateral_denom.clone()))?;

            //Initialize potential excess swap amount
            let mut excess_swap_amount: Uint128 = Uint128::zero();
            let mut msgs = vec![];

            //Edit user position with the debt purchased
            user_position.debt_amount = match user_position.debt_amount
                .checked_sub(cdt_swapped_for){
                Ok(debt_amount) => {
                    debt_amount
                },
                Err(_) => {
                    //If the amount is more than the current debt, we calculate the excess amount

                    excess_swap_amount = cdt_swapped_for - user_position.debt_amount;

                    //Set the debt to zero
                    Uint128::zero()
                }
                };
            //Save user position
            POSITIONS.save(deps.storage, (liquidation_propagation.position_owner.clone(), liquidation_propagation.collateral_denom.clone()), &user_position)?;

            //If there is an excess amount, we send it back to the user
            if excess_swap_amount > Uint128::zero() {
                //Create bank send msg
                let send_msg = withdrawal_msg(
                    Asset {
                        info: AssetInfo::NativeToken { denom: CDT_DENOM.to_string() },
                        amount: excess_swap_amount,
                    },
                    liquidation_propagation.position_owner.clone(),
                )?;
                msgs.push(send_msg);
            }

            //Edit market's total borrowed
            let mut market: MarketParams = MARKET_PARAMS.load(deps.storage, liquidation_propagation.collateral_denom.clone())?;
            market.total_borrowed = market.total_borrowed
                .checked_sub(cdt_swapped_for - excess_swap_amount)
                .map_err(|_| StdError::generic_err("The amount swapped for is more than the market's total borrowed"))?;
            MARKET_PARAMS.save(deps.storage, liquidation_propagation.collateral_denom.clone(), &market)?;

            //Remove liquidation propagation
            LIQUIDATION.remove(deps.storage);

            
            Ok(Response::new()
                .add_attribute("liquidation_position_owner", liquidation_propagation.position_owner.to_string())
                .add_attribute("debt_recovered", cdt_swapped_for)
                .add_attribute("actual_debt_repaid", cdt_swapped_for - excess_swap_amount)
                .add_attribute("cdt_returned_to_user", excess_swap_amount)
                
                .add_messages(msgs)
        )
        }        
        Err(string) => {
            //Its reply on success only
            Ok(Response::new())
        }
    }
}

// On success, update position claims & attempt to withdraw leftover using a WithdrawMsg
// pub fn handle_close_position_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
//     match msg.result.into_result() {
//         Ok(_result) => {
//             //Load Close Position Prop
//             let state_propagation: ClosePositionPropagation = CLOSE_POSITION.load(deps.storage)?;

//             //Create user info variables
//             let valid_position_owner = deps.api.addr_validate(&state_propagation.position_info.position_owner)?;
//             let position_id = state_propagation.position_info.position_id;             

//             //Load State
//             let basket: Basket = BASKET.load(deps.storage)?;
//             let config: Config = CONFIG.load(deps.storage)?;

//             //Query contract balance of the basket credit_asset
//             let credit_asset_balance = get_contract_balances(
//                 deps.querier, 
//                 env.clone(), 
//                 vec![basket.credit_asset.info.clone()]
//             )?[0];

//             //Create repay_msg
//             let repay_msg = ExecuteMsg::Repay { 
//                 position_id, 
//                 position_owner: Some(valid_position_owner.clone().to_string()),
//                 send_excess_to: Some(valid_position_owner.clone().to_string()),
//             };

//             //Create repay_msg with queried funds
//             //This works because the contract doesn't hold excess credit_asset, all repayments are burned & revenue isn't minted
//             let repay_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
//                 contract_addr: env.contract.address.to_string(), 
//                 msg: to_binary(&repay_msg)?, 
//                 funds: vec![asset_to_coin(
//                     Asset { 
//                         info: basket.credit_asset.info.clone(),
//                         amount: credit_asset_balance.clone(),
//                     })?]
//             });

//             //Update position claims for each asset withdrawn + sold
//             for withdrawn_collateral in state_propagation.clone().withdrawn_assets {

//                 update_position_claims(
//                     deps.storage, 
//                     deps.querier, 
//                     env.clone(), 
//                     config.clone(),
//                     position_id,
//                     valid_position_owner.clone(), 
//                     withdrawn_collateral.info, 
//                     withdrawn_collateral.amount
//                 )?;
//             }

//             //Load position
//             let (_i, target_position) = match get_target_position(
//                 deps.storage, 
//                 valid_position_owner.clone(), 
//                 position_id, 
//             ){
//                 Ok(position) => position,
//                 Err(err) => return Err(StdError::GenericErr { msg: err.to_string() })
//             };

//             //Withdrawing everything thats left
//             let assets_to_withdraw: Vec<Asset> = target_position.collateral_assets
//                 .into_iter()
//                 .filter(|cAsset| cAsset.asset.amount > Uint128::zero())
//                 .map(|cAsset| cAsset.asset)
//                 .collect::<Vec<Asset>>();

//             if assets_to_withdraw.len() > 0 && target_position.credit_amount.is_zero() {     
//                 //Create WithdrawMsg
//                 let withdraw_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute { 
//                     contract_addr: env.contract.address.to_string(), 
//                     msg: to_binary(& ExecuteMsg::Withdraw { 
//                         position_id, 
//                         assets: assets_to_withdraw, 
//                         send_to: state_propagation.send_to, 
//                     })?, 
//                     funds: vec![],
//                 });

//                 //Response 
//                 Ok(Response::new()
//                     .add_message(repay_msg)
//                     .add_attribute("amount_repaid", credit_asset_balance)
//                     .add_message(withdraw_msg)
//                     .add_attribute("sold_assets", format!("{:?}", state_propagation.withdrawn_assets))            
//                 )
//             } else {
//                 //Response 
//                 Ok(Response::new()
//                     .add_message(repay_msg)
//                     .add_attribute("amount_repaid", credit_asset_balance)
//                     .add_attribute("sold_assets", format!("{:?}", state_propagation.withdrawn_assets))            
//                 )
//             }
//         },

//         Err(err) => {
//             //Its reply on success only
//             Ok(Response::new().add_attribute("error", err))
//         }
//     }
// }

