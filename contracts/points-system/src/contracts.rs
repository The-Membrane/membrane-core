use core::panic;

use cosmwasm_std::{
    attr, entry_point, to_json_binary, Addr, Attribute, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Order, QuerierWrapper, QueryRequest, Reply, ReplyOn, Response, StdError, StdResult, Storage, SubMsg, SubMsgResponse, Uint128, WasmMsg, WasmQuery
};
use std::str::FromStr;
use cw2::set_contract_version;

use cw_storage_plus::Bound;
use membrane::oracle::PriceResponse;
use membrane::points_system::{ClaimCheck, Config, ExecuteMsg, InstantiateMsg, QueryMsg, VaultConversionRate, UserConversionResponse, UserStats, UserStatsResponse, PointsMultipliersResponse};
use membrane::math::{decimal_division, decimal_multiplication};
use membrane::cdp::{ExecuteMsg as CDP_ExecuteMsg, MigrateMsg, QueryMsg as CDP_QueryMsg};
use membrane::stability_pool::{QueryMsg as SP_QueryMsg, ClaimsResponse};
use membrane::liq_queue::{QueryMsg as LIQ_QueryMsg, ClaimsResponse as LQ_ClaimsResponse};
use membrane::governance::{QueryMsg as GOV_QueryMsg, Proposal};
use membrane::oracle::QueryMsg as Oracle_QueryMsg;
use membrane::osmosis_proxy::ExecuteMsg as OP_ExecuteMsg;
use membrane::staking::ExecuteMsg as Staking_ExecuteMsg;
use membrane::types::{AssetInfo, Basket, UserInfo, PointsMultipliers, VaultMultiplier};
use membrane::emissions_voting::{QueryMsg as EmissionsVotingQueryMsg, ExecuteMsg as EmissionsVotingExecuteMsg, GraphType};
use membrane::range_bound_lp_vault::QueryMsg as RB_QueryMsg;
use membrane::system_discounts::{QueryMsg as SystemDiscounts_QueryMsg, UserBoostResponse};
use membrane::transmuter::ExecuteMsg as Transmuter_ExecuteMsg;

use crate::error::ContractError;
use crate::state::{LiquidationPropagation, POINTS_MULTIPLIERS, CLAIM_CHECK, CONFIG, LIQ_PROPAGATION, OWNERSHIP_TRANSFER, USER_STATS, USER_VAULT_CONVERSION_RATES, PENDING_USER};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "points_system";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Seconds per day
const SECONDS_PER_DAY: u64 = 86_400;

// Pagination defaults
const PAGINATION_DEFAULT_LIMIT: u64 = 30;

//Reply IDs
const LIQUIDATION_REPLY_ID: u64 = 1u64;
const CDP_REPAY_REPLY_ID: u64 = 3u64;
const DISCO_CLAIM_REPLY_ID: u64 = 4u64;
const TRANSMUTER_TRANSMUTE_REPLY_ID: u64 = 5u64;

//Contract
const RANGE_BOUND_VAULT: &str = "osmo17rvvd6jc9javy3ytr0cjcypxs20ru22kkhrpwx7j3ym02znuz0vqa37ffx";
const RANGE_BOUND_VAULT_TOKEN: &str = "factory/osmo17rvvd6jc9javy3ytr0cjcypxs20ru22kkhrpwx7j3ym02znuz0vqa37ffx/cdt-usdc-range-bound-lp";

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config = Config {
        owner: info.sender,
        cdt_denom: msg.cdt_denom,
        mbrn_denom: msg.mbrn_denom,
        oracle_contract: deps.api.addr_validate(&msg.oracle_contract)?,
        positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
        stability_pool_contract: deps.api.addr_validate(&msg.stability_pool_contract)?,
        liq_queue_contract: deps.api.addr_validate(&msg.liq_queue_contract)?,
        governance_contract: deps.api.addr_validate(&msg.governance_contract)?,
        osmosis_proxy_contract: deps.api.addr_validate(&msg.osmosis_proxy_contract)?,
        transmuter_contract: None,
        ltv_disco_contract: None,
        system_discounts_contract: None,
        emissions_voting_contract: None,
        revenue_distributor_contract: None,
        mbrn_per_point: Decimal::from_ratio(1_000_000u128, 1u128), //1
        total_mbrn_distribution: Uint128::zero(), 
        max_mbrn_distribution: Uint128::new(100_000_000000u128), //100_000
        points_per_dollar: Decimal::one(),
    };    

    //Set initial points multipliers
    let points_multiplier = PointsMultipliers {
        interest_rate: Decimal::percent(1_00),
        liquidation_execution: Decimal::percent(1_00),
        liquidation_claims: Decimal::percent(1_00),
        governance_votes: Decimal::percent(3_00),
        transmuter_swap_fees: Decimal::percent(1_00),
        disco_revenue: Decimal::percent(1_00),
        vault_yields: vec![
            VaultMultiplier {
                vault_address: String::from(RANGE_BOUND_VAULT),
                multiplier: Decimal::percent(100_00),
            },
        ],
    };
    POINTS_MULTIPLIERS.save(deps.storage, &points_multiplier)?;

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
    )
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig { owner, cdt_denom, mbrn_denom, oracle_contract, positions_contract, stability_pool_contract, liq_queue_contract, governance_contract, osmosis_proxy_contract, transmuter_contract, ltv_disco_contract, system_discounts_contract, emissions_voting_contract, revenue_distributor_contract, mbrn_per_point, max_mbrn_distribution, points_per_dollar, points_multipliers } => update_config(deps, env, info, owner, cdt_denom, mbrn_denom, oracle_contract, positions_contract, stability_pool_contract, liq_queue_contract, governance_contract, osmosis_proxy_contract, transmuter_contract, ltv_disco_contract, system_discounts_contract, emissions_voting_contract, revenue_distributor_contract, mbrn_per_point, max_mbrn_distribution, points_per_dollar, points_multipliers),
        ExecuteMsg::Liquidate { position_id, position_owner } => liquidate_for_user(deps, env, info, position_id, position_owner),
        ExecuteMsg::CheckClaims { sp_claims, lq_claims, vote } => check_claims(deps, env, info, sp_claims, lq_claims, vote),
        ExecuteMsg::RepayAndGivePoints { position_id, position_owner, send_excess_to } => repay_and_give_points(deps, env, info, position_id, position_owner, send_excess_to),
        ExecuteMsg::ClaimDiscoRevenueAndGivePoints { user, asset, limit, compound_action } => claim_disco_revenue_and_give_points(deps, env, info, user, asset, limit, compound_action),
        ExecuteMsg::TransmuteAndGivePoints { recipient } => transmute_and_give_points(deps, env, info, recipient),
        ExecuteMsg::GivePoints { sp_claims, lq_claims, vote } => give_points(deps, env, info, sp_claims, lq_claims, vote),
        ExecuteMsg::ClaimMBRN {} => claim_mbrn_from_points(deps, env, info),
        ExecuteMsg::ReceiveVotingResult { label, result_uint128, result_decimal } => {
            execute_receive_voting_result(deps, info, label, result_uint128, result_decimal)
        }
        ExecuteMsg::GivePointsForAffiliateFee { affiliate, fee_amount } => {
            give_points_for_affiliate_fee(deps, env, info, affiliate, fee_amount)
        }
        ExecuteMsg::GivePointsForManagerFee { manager, fee_amount } => {
            give_points_for_manager_fee(deps, env, info, manager, fee_amount)
        }
        ExecuteMsg::CDPGivesUserManagementPoints { user } => {
            cdp_gives_user_management_points(deps, info, user)
        }
        ExecuteMsg::CheckManagementPoints { user, position_id } => {
            check_management_points(deps, env, info, user, position_id)
        }
    }
}

//CheckClaims & GivePoints are used to sandwich executable msgs to check for claims before giving points
//1) CDP Repayment: Save CDP's pending revenue to check its difference in GivePoints
//2) SP Claims: Save SP's pending claims to check its difference in GivePoints
//3) LQ Claims: Save LQ's pending claims to check its difference in GivePoints
//4) Governance Votes: Save unvoted proposals to check for votes in GivePoints
fn check_claims(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,    
    sp_claims: bool,
    lq_claims: bool,
    vote: Option<Vec<u64>>,
) -> Result<Response, ContractError>{
    //Load config
    let config: Config = CONFIG.load(deps.storage)?;

    if !sp_claims && !lq_claims && vote.is_none() {
        return Err(ContractError::Std(StdError::generic_err("No claims to check")));
    }

    let msgs: Vec<SubMsg> = vec![];
    
    // Note: CDP revenue no longer needs accrue message or pending_revenue tracking
    // We use the revenue attribute from the repay response directly

    let mut pending_sp_claims: ClaimsResponse = ClaimsResponse {
        claims: vec![],
    };
    //2) Check SP claims?
    if sp_claims {
        //Get SP's pending claims
        let sp_claims: ClaimsResponse = match deps.querier.query::<ClaimsResponse>(&QueryRequest::Wasm(WasmQuery::Smart { 
            contract_addr: config.clone().stability_pool_contract.to_string(), 
            msg: to_json_binary(&SP_QueryMsg::UserClaims { user: info.clone().sender.to_string() })?
        })){
            Ok(claims) => claims,
            //The SP errors if you have empty claims
            Err(_) => ClaimsResponse {
                claims: vec![],
            },
        };
        pending_sp_claims = sp_claims;
    }

    let mut pending_lq_claims: Vec<LQ_ClaimsResponse> = vec![];
    //3) Check Liquidation claims?
    if lq_claims {
        //Get Liquidation's pending claims
        let lq_claims: Vec<LQ_ClaimsResponse> = deps.querier.query::<Vec<LQ_ClaimsResponse>>(&QueryRequest::Wasm(WasmQuery::Smart { 
            contract_addr: config.clone().liq_queue_contract.to_string(), 
            msg: to_json_binary(&LIQ_QueryMsg::UserClaims { user: info.clone().sender.to_string() })?
        }))?;
        //Filter out any that are 0
        let lq_claims = lq_claims.into_iter().filter(|x| !x.pending_liquidated_collateral.is_zero()).collect::<Vec<LQ_ClaimsResponse>>();

        pending_lq_claims = lq_claims;
    }

    let mut unvoted_proposals: Vec<u64> = vec![];
    //4) Check Governance votes?
    if let Some(votes) = vote {
        for id in votes {
            //Query proposal
            let proposal: Proposal = deps.querier.query::<Proposal>(&QueryRequest::Wasm(WasmQuery::Smart { 
                contract_addr: config.clone().governance_contract.to_string(), 
                msg: to_json_binary(&GOV_QueryMsg::Proposal { proposal_id: id })?
            }))?;

            //Check queried proposal & add to the unvoted proposals list if the user hasn't voted
            let mut has_voted = false;
            //Check if user has voted
            if proposal.for_voters.contains(&info.clone().sender) 
            || proposal.against_voters.contains(&info.clone().sender) 
            || proposal.aligned_voters.contains(&info.clone().sender) 
            || proposal.removal_voters.contains(&info.clone().sender) 
            || proposal.amendment_voters.contains(&info.clone().sender) {
                has_voted = true;
            }
            if !has_voted {
                unvoted_proposals.push(id);
            }
        }
    }


    //Save Claim Check
    CLAIM_CHECK.save(deps.storage, &
        ClaimCheck {
            user: info.clone().sender,
            cdp_pending_revenue: Uint128::zero(),
            lq_pending_claims: pending_lq_claims.clone(),
            sp_pending_claims: pending_sp_claims.clone().claims,
            vote_pending: unvoted_proposals.clone(),
            check_time: env.block.time.seconds(),
        }
    )?;  

    //Set attributes
    let mut attrs: Vec<Attribute> = vec![];
        attrs.push(attr("sp_pending_claims", format!("{:?}", pending_sp_claims.clone().claims)));
        attrs.push(attr("lq_pending_claims", format!("{:?}", pending_lq_claims)));
        attrs.push(attr("vote_pending", format!("{:?}", unvoted_proposals)));



    Ok(Response::new().add_attributes(attrs).add_submessages(msgs))
}


/// Execute CDP repay and allocate points based on revenue attribute from reply
fn repay_and_give_points(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: Option<String>,
    send_excess_to: Option<String>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    
    // Store user address for reply handler
    PENDING_USER.save(deps.storage, &info.sender)?;
    
    // Create CDP repay submessage
    let repay_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.positions_contract.to_string(),
        msg: to_json_binary(&CDP_ExecuteMsg::Repay {
            position_id,
            position_owner,
            send_excess_to: Some(send_excess_to.unwrap_or(info.sender.to_string())),
        })?,
        funds: info.funds.clone(),
    });
    
    let repay_submsg = SubMsg::reply_on_success(repay_msg, CDP_REPAY_REPLY_ID);
    
    Ok(Response::new()
        .add_submessage(repay_submsg)
        .add_attribute("method", "repay_and_give_points"))
}

/// Execute disco revenue claim and allocate points based on revenue_claimed attribute from reply
fn claim_disco_revenue_and_give_points(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: String,
    asset: String,
    limit: Option<u32>,
    compound_action: Option<membrane::ltv_disco::CompoundAction>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    
    // Validate user address
    let user_addr = deps.api.addr_validate(&user)?;
    
    // Ensure caller is the user or authorized
    // if info.sender != user_addr {
    //     return Err(ContractError::Unauthorized {});
    // }
    
    // Store user address for reply handler
    PENDING_USER.save(deps.storage, &user_addr)?;
    
    // Get disco contract address
    let disco_addr = config.ltv_disco_contract.ok_or_else(|| {
        ContractError::Std(StdError::generic_err("LTV Disco contract not configured"))
    })?;
    
    // Create disco claim submessage
    // Note: max_ltv and max_borrow_ltv are required by the message but ignored in the implementation
    let claim_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: disco_addr.to_string(),
        msg: to_json_binary(&membrane::ltv_disco::ExecuteMsg::ClaimRevenueForUser {
            user,
            asset,
            max_ltv: Decimal::zero(),
            max_borrow_ltv: Decimal::zero(),
            limit,
            compound_action,
        })?,
        funds: vec![],
    });
    
    let claim_submsg = SubMsg::reply_on_success(claim_msg, DISCO_CLAIM_REPLY_ID);
    
    Ok(Response::new()
        .add_submessage(claim_submsg)
        .add_attribute("method", "claim_disco_revenue_and_give_points"))
}

/// Execute transmuter operation and allocate points based on swap_fee attribute from reply
fn transmute_and_give_points(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    
    // Store user address for reply handler
    PENDING_USER.save(deps.storage, &info.sender)?;
    
    // Get transmuter contract address
    let transmuter_addr = config.transmuter_contract.ok_or_else(|| {
        ContractError::Std(StdError::generic_err("Transmuter contract not configured"))
    })?;
    
    // Create transmuter transmute submessage
    // Note: Transmute requires funds to be sent with the message
    // The funds should be in info.funds
    let transmute_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: transmuter_addr.to_string(),
        msg: to_json_binary(&membrane::transmuter::ExecuteMsg::Transmute {
            recipient: Some(recipient.unwrap_or(info.sender.to_string())),
        })?,
        funds: info.funds.clone(),
    });
    
    let transmute_submsg = SubMsg::reply_on_success(transmute_msg, TRANSMUTER_TRANSMUTE_REPLY_ID);
    
    Ok(Response::new()
        .add_submessage(transmute_submsg)
        .add_attribute("method", "transmute_and_give_points"))
}

//1) CDP Repayment: Calc difference btwn CDP's pending revenue to validate how much was repaid
//- The sequence for points valid repayment is: accrue, claim_check, repay, give_points.
// Otherwise we'll account for less revenue than you may have paid if any.
//2) SP Claims: Check difference btwn present & pending claims & allocate points inline with value
//3) LQ Claims: Check difference btwn present & pending claims & allocate points inline with value
//4) Governance Votes: Give points for every unvoted proposal saved in CheckClaims that is now voted on
fn give_points(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,    
    sp_claims: bool,
    lq_claims: bool,
    vote: Option<Vec<u64>>,
    // rangebound_user: Option<String>,
) -> Result<Response, ContractError>{
    //Load Points Multiplier
    let points_multiplier: PointsMultipliers = match POINTS_MULTIPLIERS.load(deps.storage){
        Ok(multiplier) => multiplier,
        Err(_) => {
            PointsMultipliers {
                interest_rate: Decimal::one(),
                vault_yields: vec![],
                liquidation_execution: Decimal::one(),
                liquidation_claims: Decimal::one(),
                governance_votes: Decimal::one(),
                transmuter_swap_fees: Decimal::one(),
                disco_revenue: Decimal::one(),
            }
        }
    };
    //Load config
    let config: Config = CONFIG.load(deps.storage)?;
    //Load Claim Check
    let claim_check: ClaimCheck = CLAIM_CHECK.load(deps.storage)?;


    
    
    let mut attrs: Vec<Attribute> = vec![];

    let mut sp_claim_diff: Vec<Coin> = vec![];
    //2) Check SP claims?
    if sp_claims {
        //Assert the caller is the same as the claim check user
        if info.clone().sender != claim_check.user {
            return Err(ContractError::Unauthorized {});
        }
        //Check if the claim check is outdated
        if claim_check.check_time != env.block.time.seconds() {
            return Err(ContractError::Std(StdError::generic_err("Claim Check is outdated")));
        }
        //Get SP's pending claims
        let sp_current_claims: ClaimsResponse = match deps.querier.query::<ClaimsResponse>(&QueryRequest::Wasm(WasmQuery::Smart { 
            contract_addr: config.clone().stability_pool_contract.to_string(), 
            msg: to_json_binary(&SP_QueryMsg::UserClaims { user: info.clone().sender.to_string() })?
        })){
            Ok(claims) => claims,
            //The SP errors if you have empty claims
            Err(_) => ClaimsResponse {
                claims: vec![],
            },
        };
        //Check difference in claims from the query & claim check
        for previous_claim in claim_check.sp_pending_claims.clone() {
            let mut found = false;             
            for current_claim in sp_current_claims.claims.clone() {
                if current_claim.denom == previous_claim.denom {
                    found = true;
                    let diff = match previous_claim.amount.checked_sub(current_claim.amount){
                        Ok(amount) => amount,
                        Err(_) => Uint128::zero(),
                    };
                    if diff > Uint128::zero() {
                        sp_claim_diff.push(Coin {
                            denom: current_claim.denom,
                            amount: diff,
                        });
                    }
                    continue;
                }
            }
            //Not found means it was fully claimed
            if !found {
                sp_claim_diff.push(previous_claim);
            }
        }
    }

    let mut lq_claim_diff: Vec<Coin> = vec![];
    //3) Check Liquidation claims?
    if lq_claims {
        //Assert the caller is the same as the claim check user
        if info.clone().sender != claim_check.user {
            return Err(ContractError::Unauthorized {});
        }
        //Check if the claim check is outdated
        if claim_check.check_time != env.block.time.seconds() {
            return Err(ContractError::Std(StdError::generic_err("Claim Check is outdated")));
        }
        //Get Liquidation's pending claims
        let lq_current_claims: Vec<LQ_ClaimsResponse> = deps.querier.query::<Vec<LQ_ClaimsResponse>>(&QueryRequest::Wasm(WasmQuery::Smart { 
            contract_addr: config.clone().liq_queue_contract.to_string(), 
            msg: to_json_binary(&LIQ_QueryMsg::UserClaims { user: info.clone().sender.to_string() })?
        }))?;        
        //Filter out any that are 0
        let lq_current_claims = lq_current_claims.into_iter().filter(|x| !x.pending_liquidated_collateral.is_zero()).collect::<Vec<LQ_ClaimsResponse>>();

        //Check difference in claims from the query & claim check
        for prev_claim in claim_check.lq_pending_claims.clone() {
            let mut found = false; 
            for current_claim in lq_current_claims.clone() {
                if current_claim.bid_for == prev_claim.bid_for {
                    found = true;
                    let diff = match prev_claim.pending_liquidated_collateral.0.checked_sub(current_claim.pending_liquidated_collateral.0){
                        Some(amount) => Uint128::new(amount.as_u128()),
                        None => Uint128::zero(),
                    };
                    if diff > Uint128::zero() {
                        lq_claim_diff.push(Coin {
                            denom: current_claim.bid_for,
                            amount: diff,
                        });
                    }
                    continue;
                }
            }
            if !found {
                lq_claim_diff.push(Coin {
                    denom: prev_claim.bid_for,
                    amount: Uint128::new(prev_claim.pending_liquidated_collateral.0.as_u128()),
                });
            }
        }
    }

    let mut newly_voted_proposals: Vec<u64> = vec![];
    //4) Check Governance votes?
    if let Some(votes) = vote {
        //Assert the caller is the same as the claim check user
        if info.clone().sender != claim_check.user {
            return Err(ContractError::Unauthorized {});
        }
        //Check if the claim check is outdated
        if claim_check.check_time != env.block.time.seconds() {
            return Err(ContractError::Std(StdError::generic_err("Claim Check is outdated")));
        }
        //Filter out proposal IDs that aren't in the claim check
        let votes = votes.into_iter().filter(|x| claim_check.vote_pending.contains(x)).collect::<Vec<u64>>();

        for id in votes {
            //Query proposal
            let proposal: Proposal = deps.querier.query::<Proposal>(&QueryRequest::Wasm(WasmQuery::Smart { 
                contract_addr: config.clone().governance_contract.to_string(), 
                msg: to_json_binary(&GOV_QueryMsg::Proposal { proposal_id: id })?
            }))?;

            //If the proposal was within the last hour, continue.
            //This blocks attempts of farming proposals since they r free.
            if proposal.start_time >= env.block.time.seconds() - SECONDS_PER_DAY/24 {
                continue;
            }

            //Check queried proposal & add to the unvoted proposals list if the user has voted
            let mut has_voted = false;
            //Check if user has voted
            if proposal.for_voters.contains(&info.clone().sender) 
            || proposal.against_voters.contains(&info.clone().sender) 
            || proposal.aligned_voters.contains(&info.clone().sender) 
            || proposal.removal_voters.contains(&info.clone().sender) 
            || proposal.amendment_voters.contains(&info.clone().sender) {
                has_voted = true;
            }
            if has_voted {
                newly_voted_proposals.push(id);
            }
        }
    }
    //5) Check Range Bound Vault conversion rates
    // if let Some(user) = rangebound_user {
    //     //Validate user address
    //     let range_bound_user_addr = deps.api.addr_validate(&user)?;
    //     //Load User's Vault Conversion info
    //     let mut user_info = USER_VAULT_CONVERSION_RATES.load(deps.storage, range_bound_user_addr.clone())?;

    //     let mut found: Option<usize> = None;
        
    //     //Find User's Range Bound Vault info
    //     let mut rangebound_info = match user_info.clone().into_iter().enumerate().find(|(_, x)| x.vault_address == RANGE_BOUND_VAULT){
    //         Some((index, info)) => {
    //             found = Some(index);
    //             info
    //         },
    //         None => return Err(ContractError::Std(StdError::generic_err(format!("{} has no Range Bound Vault info", user.clone())))),
    //     };
    //     //Query user's wallet for VT balance
    //     let user_vt_balance: Uint128 = match deps.querier.query_balance(user.clone(), String::from(RANGE_BOUND_VAULT_TOKEN)){
    //         Ok(balance) => balance.amount,
    //         Err(_) => Uint128::zero(),
    //     };
    //     //If balance is less than the last balance, update the user's info
    //     if user_vt_balance < rangebound_info.last_vt_balance {
    //         rangebound_info.last_vt_balance = user_vt_balance;
    //     } 

    //     //If balance is 0, remove the user's info
    //     if user_vt_balance == Uint128::zero() || rangebound_info.last_vt_balance == Uint128::zero() {
    //         //Remove user's Range Bound Vault info
    //         user_info.remove(found.unwrap());
    //     } else {
    //         /////Give points on the difference of the conversion rates * the VT initial/lower balance////
    //         //Query Range Bound Vault for conversion rate
    //         let conversion_rate: Uint128 = match deps.querier.query::<Uint128>(&QueryRequest::Wasm(WasmQuery::Smart { 
    //             contract_addr: RANGE_BOUND_VAULT.to_string().clone(), 
    //             msg: to_json_binary(&RB_QueryMsg::VaultTokenUnderlying { vault_token_amount: Uint128::new(1000000000000u128) })?
    //         })){
    //             Ok(rate) => rate,
    //             Err(_) => return Err(ContractError::Std(StdError::generic_err("Failed to query Range Bound Vault for conversion rate"))),
    //         };
    //         //Calc conversion rate difference
    //         let mut rate_diff = match decimal_division(
    //             Decimal::from_ratio(conversion_rate, Uint128::one()),
    //         Decimal::from_ratio(rangebound_info.last_conversion_rate, Uint128::one())
    //         ){
    //             Ok(diff) => diff,
    //             Err(_) => return Err(ContractError::Std(StdError::generic_err(format!("{} conversion rate division errored", user)))),
    //         };
    //         //Subtract 1 to get the yield gained per 1 VT
    //         rate_diff = match rate_diff.checked_sub(Decimal::one()){
    //             Ok(diff) => diff,
    //             Err(_) => return Err(ContractError::Std(StdError::generic_err(format!("{} conversion rate subtraction errored", user)))),
    //         };

    //         //Query Range Bound Vault for user's underlying token balance
    //         let underlying_deposit_token: Uint128 = match deps.querier.query::<Uint128>(&QueryRequest::Wasm(WasmQuery::Smart { 
    //             contract_addr: RANGE_BOUND_VAULT.to_string().clone(), 
    //             msg: to_json_binary(&RB_QueryMsg::VaultTokenUnderlying { vault_token_amount: rangebound_info.last_vt_balance })?
    //         })){
    //             Ok(rate) => rate,
    //             Err(_) => return Err(ContractError::Std(StdError::generic_err("Failed to query Range Bound Vault for underlying_deposit_token"))),
    //         };
    //         //Calc points to give.
    //         //We give points based on the underlying CDT * the yield gained per 1 VT = how much CDT was earned
    //         let cdt_rev_made = decimal_multiplication(
    //             rate_diff, 
    //             Decimal::from_ratio(underlying_deposit_token, Uint128::one()
    //         ))?;

    //         //Find the points multiplier for the range bound vault
    //         let mut multiplier = Decimal::one();
    //         for vault in points_multiplier.vault_yields.clone() {
    //             if vault.vault_address == RANGE_BOUND_VAULT {
    //                 multiplier = vault.multiplier;
    //                 break;
    //             }
    //         }

    //         //Add these points to the user's claimable points
    //         allocate_points(
    //             deps.storage, 
    //             deps.querier, 
    //             config.clone(), 
    //             range_bound_user_addr.clone(), 
    //             basket.clone().credit_price, 
    //             cdt_rev_made.clone().to_uint_floor() * multiplier, 
    //             vec![], 
    //             vec![], 
    //             vec![],
    //             vec![],
    //             vec![],
    //             points_multiplier.clone()
    //         )?;
    //         attrs.push(attr("range_bound_yield", cdt_rev_made.to_string()));

    //         //Update user's Range Bound Vault info
    //         user_info[found.unwrap()] = VaultConversionRate {
    //             vault_address: String::from(RANGE_BOUND_VAULT),
    //             last_conversion_rate: conversion_rate,
    //             last_vt_balance: user_vt_balance,
    //         };

    //     }

    //     //Save or remove user info
    //     if user_info.len() > 0 {
    //         USER_VAULT_CONVERSION_RATES.save(deps.storage, range_bound_user_addr, &user_info)?;
    //     } else {
    //         USER_VAULT_CONVERSION_RATES.remove(deps.storage, range_bound_user_addr);
    //     }
    // }

    let transmuter_fee_diff: Vec<Coin> = vec![];
    let disco_revenue_diff: Vec<Coin> = vec![];

    //Delete Claim Check
    CLAIM_CHECK.remove(deps.storage);

    //Allocate points
    allocate_points(
        deps.storage, 
        deps.querier, 
        config.clone(), 
        info.sender.clone(), 
        PriceResponse {
            prices: vec![],
            price: Decimal::zero(),
            decimals: 0,
        }, 
        Uint128::zero(), 
        sp_claim_diff.clone(), 
        lq_claim_diff.clone(), 
        newly_voted_proposals.clone(),
        transmuter_fee_diff.clone(),
        disco_revenue_diff.clone(),
        points_multiplier.clone()
    )?;

    //Set attributes
    // attrs.push(attr("revenue_paid", revenue_paid));
    attrs.push(attr("sp_claim_diff", format!("{:?}", sp_claim_diff)));
    attrs.push(attr("lq_claim_diff", format!("{:?}", lq_claim_diff)));
    attrs.push(attr("newly_voted_proposals", format!("{:?}", newly_voted_proposals)));



    Ok(Response::new().add_attributes(attrs))
}

/// Liquidate a position
/// Send fees to the caller in the reply
/// 1) Liquidator gets points for the fee
/// 2) Liquidatee gets points for what was liquidated
fn liquidate_for_user(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,    
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError>{
    //Load config
    let config: Config = CONFIG.load(deps.storage)?;
    //Verify address
    let position_owner = deps.api.addr_validate(&position_owner)?;

    //Create Liquidation message
    let liquidation_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
        contract_addr: config.clone().positions_contract.to_string(), 
        msg: to_json_binary(&CDP_ExecuteMsg::Liquidate {
                position_id,
                position_owner: position_owner.to_string(),
            })?,
        funds: vec![] 
    });
    //Create submsg
    let liquidation_submsg = SubMsg::reply_on_success(liquidation_msg, LIQUIDATION_REPLY_ID);

    //Get CDP's outstanding credit supply
    let basket: Basket = deps.querier.query::<Basket>(&QueryRequest::Wasm(WasmQuery::Smart { 
        contract_addr: config.clone().positions_contract.to_string(), 
        msg: to_json_binary(&CDP_QueryMsg::GetBasket {  })?
    }))?;
    //Save balances
    LIQ_PROPAGATION.save(deps.storage, &
        LiquidationPropagation {
            pre_liq_CDT: basket.credit_asset.amount,
            liquidator: info.sender.clone(),
            liquidatee: position_owner.clone(),
        }
    )?;
    //We will use the difference in CREDIT SUPPLY to calculate how much was liquidated//
    //Then we check this contract's balances to find & send fees to the caller.


    Ok(Response::new().add_submessage(liquidation_submsg))
}

fn claim_mbrn_from_points(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError>{
    //Load config
    let mut config: Config = CONFIG.load(deps.storage)?;
    //Load user stats
    let user_stats = match USER_STATS.load(deps.storage, info.sender.clone()){
        Ok(stats) => stats,
        Err(_) => return Err(ContractError::Std(StdError::generic_err("User has no points in our system"))),
    };

    //Calculate MBRN to claim
    let mbrn_to_claim = match user_stats.claimable_points.checked_mul(config.clone().mbrn_per_point){
        Ok(amount) => amount.to_uint_ceil(),
        Err(_) => return Err(ContractError::Std(StdError::generic_err("No MBRN to claim"))),
    };

    if mbrn_to_claim == Uint128::zero() {
        return Err(ContractError::Std(StdError::generic_err("No MBRN to claim")));
    }

    //Assert MBRN to claim is less than the max distribution
    // if mbrn_to_claim + config.clone().total_mbrn_distribution > config.clone().max_mbrn_distribution {
        
    //     //Set MBRN to claim as any remaining MBRN to reach the max distribution
    //     mbrn_to_claim = match config.clone().max_mbrn_distribution.checked_sub(config.clone().total_mbrn_distribution){
    //         Ok(amount) => amount,
    //         Err(_) => return Err(ContractError::Std(StdError::generic_err("Claimable MBRN exceeds the max distribution")))
    //     };
    // }

    //Update total MBRN distribution
    config.total_mbrn_distribution += mbrn_to_claim;
    CONFIG.save(deps.storage, &config)?;

    //Reset user's claimable levels
    let mut updated_stats = user_stats.clone();
    updated_stats.claimable_points = Decimal::zero();

    //Mint MBRN to user
    // let mbrn_mint: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
    //     contract_addr: config.clone().osmosis_proxy_contract.to_string(),
    //     msg: to_json_binary(&OP_ExecuteMsg::MintTokens { 
    //         denom: String::from("factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/umbrn"), 
    //         amount: mbrn_to_claim, 
    //         mint_to_address: info.sender.clone().to_string(), 
    //     })?,
    //     funds: vec![],
    // });

    //Send MBRN to user from the contract's balances.
    //NOTE: Using balance as the max means we won't use the config's max as a restriction.
    // let mbrn_send: CosmosMsg = CosmosMsg::Bank(BankMsg::Send {
    //     to_address:info.sender.clone().to_string(),
    //     amount: vec![
    //         Coin {
    //             denom: String::from("factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/umbrn"), 
    //             amount: mbrn_to_claim, 
    //         }
    //     ],
    // });

    //Stake the MBRN for the user
    let mbrn_stake: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: "osmo1fty83rfxqs86jm5fmlql5e340e8pe0v9j8ez0lcc6zwt2amegwvsfp3gxj".to_string(),
        msg: to_json_binary(&Staking_ExecuteMsg::Stake { 
            user: Some(info.sender.clone().to_string()),
            locked: None,
        })?,
        funds: vec![
            Coin {
                denom: config.mbrn_denom, 
                amount: mbrn_to_claim, 
            }
        ],
    });

    //Save updated stats
    USER_STATS.save(deps.storage, info.sender.clone(), &updated_stats)?;
    
    //Set attributes
    let mut attrs: Vec<Attribute> = vec![];
        attrs.push(attr("mbrn_claimed", mbrn_to_claim));
        attrs.push(attr("claimed_points", user_stats.claimable_points.to_string()));

    Ok(Response::new()
    .add_attributes(attrs)
    .add_message(mbrn_stake))
}

fn allocate_points(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    config: Config,
    user: Addr,
    cdt_price: PriceResponse,
    revenue_paid: Uint128,
    sp_claim_diff: Vec<Coin>,
    lq_claim_diff: Vec<Coin>,
    newly_voted_proposals: Vec<u64>,
    transmuter_fee_diff: Vec<Coin>,
    disco_revenue_diff: Vec<Coin>,
    points_multipliers: PointsMultipliers
) -> StdResult<()> {
    //Concat the sp & lq claims
    let mut claim_diffs: Vec<Coin> = sp_claim_diff.clone();
    claim_diffs.extend(lq_claim_diff.clone());

    //Create a list of unique denoms from the sp & lq claims
    let mut unique_denoms: Vec<AssetInfo> = vec![];
    for coin in claim_diffs.clone(){
        if !unique_denoms.contains(&AssetInfo::NativeToken { denom: coin.denom.clone() }){
            unique_denoms.push(
                AssetInfo::NativeToken { denom: coin.denom.clone() }
            );
        }
    }

    //Get the current price of each unique denom
    let denoms_prices: Vec<PriceResponse> = querier.query::<Vec<PriceResponse>>(&QueryRequest::Wasm(WasmQuery::Smart { 
        contract_addr: config.clone().oracle_contract.to_string(), 
        msg: to_json_binary(&Oracle_QueryMsg::Prices { 
            asset_infos: unique_denoms.clone(),
            twap_timeframe: 60u64,
            oracle_time_limit: 600u64,
        })?
    })).unwrap_or(vec![]);
    //It'll error if there are no denoms passed most likely so we just return an empty vec

    
    //Sum the value of each claim
    let mut total_value: Decimal = Decimal::zero();
    for coin in claim_diffs.clone(){
        //Find index of the denom in unique denoms
        if let Some(index) = unique_denoms.iter().position(|x| x == &AssetInfo::NativeToken { denom: coin.denom.clone() }){
            //Add the value of the claim to the total value
            total_value += decimal_multiplication(
                denoms_prices[index].get_value(coin.amount)?,
                points_multipliers.liquidation_claims.clone()
            )?;
        }
    }
    
    //Add CDT revenue to the total value
    total_value += cdt_price.get_value(revenue_paid)?;

    //Add $1 for each proposal voted
    total_value += decimal_multiplication(
        Decimal::from_ratio(newly_voted_proposals.len() as u64, 1u64),
        points_multipliers.governance_votes.clone()
    )?;

    //Add transmuter swap fees value
    for coin in transmuter_fee_diff.clone() {
        // if coin.denom == config.cdt_denom {
            total_value += decimal_multiplication(
                cdt_price.get_value(coin.amount)?,
                points_multipliers.transmuter_swap_fees.clone()
            )?;
        // } else {
        //     // Query price for non-CDT fees
        //     let fee_price: PriceResponse = querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        //         contract_addr: config.clone().oracle_contract.to_string(),
        //         msg: to_json_binary(&Oracle_QueryMsg::Prices {
        //             asset_infos: vec![AssetInfo::NativeToken { denom: coin.denom.clone() }],
        //             twap_timeframe: 60u64,
        //             oracle_time_limit: 600u64,
        //         })?
        //     }))?;
        //     total_value += decimal_multiplication(
        //         fee_price.get_value(coin.amount)?,
        //         points_multipliers.transmuter_swap_fees.clone()
        //     )?;
        // }
    }

    //Add disco revenue value
    for coin in disco_revenue_diff.clone() {
        // if coin.denom == config.cdt_denom {
            total_value += decimal_multiplication(
                cdt_price.get_value(coin.amount)?,
                points_multipliers.disco_revenue.clone()
            )?;
        // } else {
        //     // Query price for non-CDT revenue
        //     let revenue_price: PriceResponse = querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        //         contract_addr: config.clone().oracle_contract.to_string(),
        //         msg: to_json_binary(&Oracle_QueryMsg::Prices {
        //             asset_infos: vec![AssetInfo::NativeToken { denom: coin.denom.clone() }],
        //             twap_timeframe: 60u64,
        //             oracle_time_limit: 600u64,
        //         })?
        //     }))?;
        //     total_value += decimal_multiplication(
        //         revenue_price.get_value(coin.amount)?,
        //         points_multipliers.disco_revenue.clone()
        //     )?;
        // }
    }

    //Calculate base points
    let base_points = decimal_multiplication(total_value, config.clone().points_per_dollar)?;

    //Apply boost multiplier if system_discounts contract is configured
    let points = if let Some(discounts_addr) = &config.system_discounts_contract {
        // Query user's boost
        let boost_response: UserBoostResponse = querier.query::<UserBoostResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: discounts_addr.to_string(),
            msg: to_json_binary(&SystemDiscounts_QueryMsg::UserBoost {
                user: user.to_string(),
            })?
        }))?;
        
        // Calculate boost multiplier: 1 + boost_percentage
        let boost_multiplier = Decimal::one() + boost_response.boost;
        
        // Apply boost to points
        decimal_multiplication(base_points, boost_multiplier)?
    } else {
        base_points
    };

    //Save points to user
    let mut user_stats = match USER_STATS.load(storage, user.clone()){
        Ok(stats) => stats,
        Err(_) => UserStats {
            total_points: Decimal::zero(),
            claimable_points: Decimal::zero(),
        },
    };
    user_stats.total_points += points;
    user_stats.claimable_points += points;
    //Save updated stats
    USER_STATS.save(storage, user, &user_stats)?;

    Ok(())
}


/// Update contract configuration
fn update_config(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: Option<String>,
    cdt_denom: Option<String>,
    mbrn_denom: Option<String>,
    oracle_contract: Option<String>,
    positions_contract: Option<String>,
    stability_pool_contract: Option<String>,
    liq_queue_contract: Option<String>,
    governance_contract: Option<String>,
    osmosis_proxy_contract: Option<String>,
    transmuter_contract: Option<String>,
    ltv_disco_contract: Option<String>,
    system_discounts_contract: Option<String>,
    emissions_voting_contract: Option<String>,
    revenue_distributor_contract: Option<String>,
    mbrn_per_point: Option<Decimal>,
    max_mbrn_distribution: Option<Uint128>,
    points_per_dollar: Option<Decimal>,  
    points_multipliers: Option<PointsMultipliers>
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![attr("method", "update_config")];

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    //Save optionals
    if let Some(addr) = owner {
        let valid_addr = deps.api.addr_validate(&addr)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));  
    }
    if let Some(denom) = cdt_denom {
        config.cdt_denom = denom.clone();
        attrs.push(attr("cdt_denom", denom));
    }
    if let Some(addr) = oracle_contract {
        config.oracle_contract = deps.api.addr_validate(&addr)?;
        attrs.push(attr("oracle_contract", addr));
    }
    if let Some(denom) = mbrn_denom {
        config.mbrn_denom = denom.clone();
        attrs.push(attr("mbrn_denom", denom));
    }
    if let Some(addr) = positions_contract {
        config.positions_contract = deps.api.addr_validate(&addr)?;
        attrs.push(attr("positions_contract", addr));
    }
    if let Some(addr) = stability_pool_contract {
        config.stability_pool_contract = deps.api.addr_validate(&addr)?;
        attrs.push(attr("stability_pool_contract", addr));
    }
    if let Some(addr) = liq_queue_contract {
        config.liq_queue_contract = deps.api.addr_validate(&addr)?;
        attrs.push(attr("liq_queue_contract", addr));
    }
    if let Some(addr) = governance_contract {
        config.governance_contract = deps.api.addr_validate(&addr)?;
        attrs.push(attr("governance_contract", addr));
    }
    if let Some(addr) = osmosis_proxy_contract {
        config.osmosis_proxy_contract = deps.api.addr_validate(&addr)?;
        attrs.push(attr("osmosis_proxy_contract", addr));
    }
    if let Some(addr) = transmuter_contract {
        config.transmuter_contract = Some(deps.api.addr_validate(&addr)?);
        attrs.push(attr("transmuter_contract", addr));
    }
    if let Some(addr) = ltv_disco_contract {
        config.ltv_disco_contract = Some(deps.api.addr_validate(&addr)?);
        attrs.push(attr("ltv_disco_contract", addr));
    }
    if let Some(addr) = system_discounts_contract {
        config.system_discounts_contract = Some(deps.api.addr_validate(&addr)?);
        attrs.push(attr("system_discounts_contract", addr));
    }
    if let Some(addr) = emissions_voting_contract {
        config.emissions_voting_contract = Some(deps.api.addr_validate(&addr)?);
        attrs.push(attr("emissions_voting_contract", addr));
    }
    if let Some(addr) = revenue_distributor_contract {
        config.revenue_distributor_contract = Some(deps.api.addr_validate(&addr)?);
        attrs.push(attr("revenue_distributor_contract", addr));
    }
    if let Some(amount) = mbrn_per_point {
        config.mbrn_per_point = amount;
        attrs.push(attr("mbrn_per_point", amount.to_string()));
    }
    if let Some(amount) = max_mbrn_distribution {
        config.max_mbrn_distribution = amount;
        attrs.push(attr("max_mbrn_distribution", amount));
    }
    if let Some(amount) = points_per_dollar {
        config.points_per_dollar = amount;
        attrs.push(attr("points_per_dollar", amount.to_string()));
    }        
    if let Some(multipliers) = points_multipliers {
        // Get old multipliers to compare vault_yields
        let old_multipliers = POINTS_MULTIPLIERS.may_load(deps.storage)?.unwrap_or(PointsMultipliers {
            interest_rate: Decimal::one(),
            vault_yields: vec![],
            liquidation_execution: Decimal::one(),
            liquidation_claims: Decimal::one(),
            governance_votes: Decimal::one(),
            transmuter_swap_fees: Decimal::one(),
            disco_revenue: Decimal::one(),
        });

        // Find new vaults that don't exist in old multipliers
        let old_vault_addresses: Vec<String> = old_multipliers.vault_yields.iter()
            .map(|v| v.vault_address.clone())
            .collect();
        
        let new_vaults: Vec<&VaultMultiplier> = multipliers.vault_yields.iter()
            .filter(|v| !old_vault_addresses.contains(&v.vault_address))
            .collect();

        // Create graphs for new vaults if emissions_voting_contract is set
        let mut graph_msgs: Vec<CosmosMsg> = vec![];
        if let Some(emissions_voting_addr) = &config.emissions_voting_contract {
            for vault in new_vaults {
                // Check if graph already exists by querying
                let graph_exists = deps.querier.query::<membrane::emissions_voting::GraphResponse>(
                    &QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: emissions_voting_addr.to_string(),
                        msg: to_json_binary(&EmissionsVotingQueryMsg::Graph {
                            label: vault.vault_address.clone(),
                        })?,
                    })
                ).is_ok();

                // Only create if graph doesn't exist
                if !graph_exists {
                    graph_msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: emissions_voting_addr.to_string(),
                        msg: to_json_binary(&EmissionsVotingExecuteMsg::CreateGraph {
                            label: vault.vault_address.clone(),
                            graph_type: GraphType::Decimal,
                            range_min: "1.0".to_string(),
                            range_max: "10.0".to_string(),
                            period_days: 7,
                            callback_contract: env.contract.address.to_string(),
                        })?,
                        funds: vec![],
                    }));
                }
            }
        }

        POINTS_MULTIPLIERS.save(deps.storage, &multipliers)?;
        attrs.push(attr("points_multipliers", format!("{:?}", multipliers)));

        // Save Config
        CONFIG.save(deps.storage, &config)?;
        attrs.push(attr("updated_config", format!("{:?}", config)));

        let mut response = Response::new().add_attributes(attrs);
        if !graph_msgs.is_empty() {
            response = response.add_messages(graph_msgs);
        }
        Ok(response)
    } else {
        //Save Config
        CONFIG.save(deps.storage, &config)?;
        attrs.push(attr("updated_config", format!("{:?}", config)));

        Ok(Response::new().add_attributes(attrs))
    }
}



#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        LIQUIDATION_REPLY_ID => handle_liq_reply(deps, env, msg),
        CDP_REPAY_REPLY_ID => handle_cdp_repay_reply(deps, env, msg),
        DISCO_CLAIM_REPLY_ID => handle_disco_claim_reply(deps, env, msg),
        TRANSMUTER_TRANSMUTE_REPLY_ID => handle_transmuter_transmute_reply(deps, env, msg),
        CHECK_MANAGEMENT_POINTS_REPLY_ID => handle_check_management_points_reply(deps, msg),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

/// On success, sell for collateral composition, redeposit & call loop fn again.
/// Increment Loop number.
fn handle_liq_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response>{
    
    match msg.result.into_result() {
        Ok(_) => {
            //Load config
            let config: Config = CONFIG.load(deps.storage)?;
            //Query new CDT SUPPLY
            let basket: Basket = deps.querier.query::<Basket>(&QueryRequest::Wasm(WasmQuery::Smart { 
                contract_addr: config.clone().positions_contract.to_string(), 
                msg: to_json_binary(&CDP_QueryMsg::GetBasket {  })?
            }))?;
            let post_liq_CDT = basket.credit_asset.amount;

            //Load Liquidation Propagation
            let liquidation_propagation: LiquidationPropagation = LIQ_PROPAGATION.load(deps.storage)?;
            
            //Calculate liquidation amount
            let liquidated_amount = match liquidation_propagation.clone().pre_liq_CDT.checked_sub(post_liq_CDT.clone()){
                Ok(amount) => amount,
                Err(_) => Uint128::zero(),
            };

            //Query contract balances to find fees
            let balances = deps.querier.query_all_balances(env.contract.address.clone())?;         

            //Filter out MBRN from balances
            let balances = balances.into_iter().filter(|x| x.denom != "factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/umbrn").collect::<Vec<Coin>>();   

            //Send fees to caller
            let fee_message: CosmosMsg = CosmosMsg::Bank(BankMsg::Send {
                to_address: liquidation_propagation.liquidator.to_string(),
                amount: balances.clone(),
            });

            //Empty Liquidation Propagation
            LIQ_PROPAGATION.remove(deps.storage);

            //Load points multipliers
            let points_multiplier: PointsMultipliers = POINTS_MULTIPLIERS.load(deps.storage)?;

            //Allocate points to the Vault owner who got liquidated
            allocate_points(
                deps.storage, 
                deps.querier, 
                config.clone(), 
                liquidation_propagation.clone().liquidatee, 
                basket.clone().credit_price, 
                liquidated_amount * points_multiplier.liquidation_execution,
                vec![],                 
                vec![],                 
                vec![],
                vec![],
                vec![],
                points_multiplier.clone()
            )?;

            //Allocate points to the Liquidation caller
            allocate_points(
                deps.storage, 
                deps.querier, 
                config.clone(), 
                liquidation_propagation.clone().liquidator, 
                basket.clone().credit_price, 
                Uint128::zero(), 
                balances.clone(),
                vec![],                 
                vec![],
                vec![],
                vec![],
                points_multiplier.clone()
            )?;
            
            Ok(Response::new()
                .add_message(fee_message)
                .add_attributes([
                    attr("pre_liquidated_CDT", liquidation_propagation.pre_liq_CDT),
                    attr("post_liquidated_CDT", post_liq_CDT),
                    attr("total_liquidated_CDT", liquidated_amount),
                    attr("fee_assets", format!("{:?}", balances)),
                    ])
            )

        },
        Err(string) => {            
            Ok(Response::new().add_attribute("we no error", string))
        }
    }
}

/// Helper function to parse attributes from reply responses
fn parse_attribute(result: &SubMsgResponse, key: &str) -> StdResult<Uint128> {
    // Search through events for the attribute
    for event in &result.events {
        for attr in &event.attributes {
            if attr.key == key {
                return Uint128::from_str(&attr.value)
                    .map_err(|_| StdError::generic_err(format!("Invalid {} value: {}", key, attr.value)));
            }
        }
    }
    Err(StdError::generic_err(format!("Attribute {} not found in reply", key)))
}

/// Helper function to parse string attribute from reply responses
fn parse_string_attribute(result: &SubMsgResponse, key: &str) -> StdResult<String> {
    // Search through events for the attribute
    for event in &result.events {
        for attr in &event.attributes {
            if attr.key == key {
                return Ok(attr.value.clone());
            }
        }
    }
    Err(StdError::generic_err(format!("Attribute {} not found in reply", key)))
}

/// Handle CDP repay reply - parse revenue attribute and allocate points
fn handle_cdp_repay_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(result) => {
            // Parse revenue attribute from response
            let revenue = parse_attribute(&result, "revenue")?;
            
            // Load user from temporary storage
            let user = PENDING_USER.load(deps.storage)?;
            PENDING_USER.remove(deps.storage);
            
            // Load config and calculate points
            let config = CONFIG.load(deps.storage)?;
            let basket: Basket = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.positions_contract.to_string(),
                msg: to_json_binary(&CDP_QueryMsg::GetBasket {})?,
            }))?;
            
            // Load points multipliers
            let points_multiplier: PointsMultipliers = POINTS_MULTIPLIERS.load(deps.storage)
                .unwrap_or_else(|_| PointsMultipliers {
                    interest_rate: Decimal::one(),
                    vault_yields: vec![],
                    liquidation_execution: Decimal::one(),
                    liquidation_claims: Decimal::one(),
                    governance_votes: Decimal::one(),
                    transmuter_swap_fees: Decimal::one(),
                    disco_revenue: Decimal::one(),
                });
            
            // Allocate points with verified revenue
            allocate_points(
                deps.storage,
                deps.querier,
                config,
                user.clone(),
                basket.credit_price,
                revenue,
                vec![], // sp_claim_diff
                vec![], // lq_claim_diff
                vec![], // newly_voted_proposals
                vec![], // transmuter_fee_diff
                vec![], // disco_revenue_diff
                points_multiplier,
            )?;
            
            Ok(Response::new()
                .add_attribute("method", "handle_cdp_repay_reply")
                .add_attribute("user", user.to_string())
                .add_attribute("revenue", revenue.to_string())
                .add_attribute("points_allocated", "true"))
        }
        Err(err) => {
            // Clean up storage and return error
            PENDING_USER.remove(deps.storage);
            Err(StdError::generic_err(format!("Repay failed: {}", err)))
        }
    }
}

/// Handle disco claim reply - parse revenue_claimed attribute and allocate points
fn handle_disco_claim_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(result) => {
            // Parse revenue_claimed attribute from response
            let revenue_claimed = parse_attribute(&result, "revenue_claimed")?;
            
            // Load user from temporary storage
            let user = PENDING_USER.load(deps.storage)?;
            PENDING_USER.remove(deps.storage);
            
            // Load config and calculate points
            let config = CONFIG.load(deps.storage)?;
            let basket: Basket = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.positions_contract.to_string(),
                msg: to_json_binary(&CDP_QueryMsg::GetBasket {})?,
            }))?;
            
            // Load points multipliers
            let points_multiplier: PointsMultipliers = POINTS_MULTIPLIERS.load(deps.storage)
                .unwrap_or_else(|_| PointsMultipliers {
                    interest_rate: Decimal::one(),
                    vault_yields: vec![],
                    liquidation_execution: Decimal::one(),
                    liquidation_claims: Decimal::one(),
                    governance_votes: Decimal::one(),
                    transmuter_swap_fees: Decimal::one(),
                    disco_revenue: Decimal::one(),
                });
            
            // Convert revenue to Coin format for allocate_points
            let disco_revenue_diff = if !revenue_claimed.is_zero() {
                vec![Coin {
                    denom: config.cdt_denom.clone(),
                    amount: revenue_claimed,
                }]
            } else {
                vec![]
            };
            
            // Allocate points with verified revenue
            allocate_points(
                deps.storage,
                deps.querier,
                config,
                user.clone(),
                basket.credit_price,
                Uint128::zero(), // revenue_paid
                vec![], // sp_claim_diff
                vec![], // lq_claim_diff
                vec![], // newly_voted_proposals
                vec![], // transmuter_fee_diff
                disco_revenue_diff, // disco_revenue_diff
                points_multiplier,
            )?;
            
            Ok(Response::new()
                .add_attribute("method", "handle_disco_claim_reply")
                .add_attribute("user", user.to_string())
                .add_attribute("revenue_claimed", revenue_claimed.to_string())
                .add_attribute("points_allocated", "true"))
        }
        Err(err) => {
            // Clean up storage and return error
            PENDING_USER.remove(deps.storage);
            Err(StdError::generic_err(format!("Disco claim failed: {}", err)))
        }
    }
}

/// Handle transmuter transmute reply - parse swap_fee attributes and allocate points
fn handle_transmuter_transmute_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(result) => {
            // Parse swap_fee and swap_fee_denom attributes from response
            let swap_fee = parse_attribute(&result, "swap_fee")?;
            let swap_fee_denom = parse_string_attribute(&result, "swap_fee_denom")
                .unwrap_or_else(|_| String::new());
            
            // Load user from temporary storage
            let user = PENDING_USER.load(deps.storage)?;
            PENDING_USER.remove(deps.storage);
            
            // If no swap fee, just return success without allocating points
            if swap_fee.is_zero() || swap_fee_denom.is_empty() {
                return Ok(Response::new()
                    .add_attribute("method", "handle_transmuter_transmute_reply")
                    .add_attribute("user", user.to_string())
                    .add_attribute("swap_fee", "0")
                    .add_attribute("points_allocated", "false"));
            }
            
            // Load config and calculate points
            let config = CONFIG.load(deps.storage)?;
            let basket: Basket = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.positions_contract.to_string(),
                msg: to_json_binary(&CDP_QueryMsg::GetBasket {})?,
            }))?;
            
            // Load points multipliers
            let points_multiplier: PointsMultipliers = POINTS_MULTIPLIERS.load(deps.storage)
                .unwrap_or_else(|_| PointsMultipliers {
                    interest_rate: Decimal::one(),
                    vault_yields: vec![],
                    liquidation_execution: Decimal::one(),
                    liquidation_claims: Decimal::one(),
                    governance_votes: Decimal::one(),
                    transmuter_swap_fees: Decimal::one(),
                    disco_revenue: Decimal::one(),
                });
            
            // Convert swap fee to Coin format for allocate_points
            let transmuter_fee_diff = vec![Coin {
                denom: swap_fee_denom,
                amount: swap_fee,
            }];
            
            // Allocate points with verified swap fee
            allocate_points(
                deps.storage,
                deps.querier,
                config,
                user.clone(),
                basket.credit_price,
                Uint128::zero(), // revenue_paid
                vec![], // sp_claim_diff
                vec![], // lq_claim_diff
                vec![], // newly_voted_proposals
                transmuter_fee_diff, // transmuter_fee_diff
                vec![], // disco_revenue_diff
                points_multiplier,
            )?;
            
            Ok(Response::new()
                .add_attribute("method", "handle_transmuter_transmute_reply")
                .add_attribute("user", user.to_string())
                .add_attribute("swap_fee", swap_fee.to_string())
                .add_attribute("points_allocated", "true"))
        }
        Err(err) => {
            // Clean up storage and return error
            PENDING_USER.remove(deps.storage);
            Err(StdError::generic_err(format!("Transmute failed: {}", err)))
        }
    }
}

/// Reply handler for CheckManagementPoints.
/// Checks the qualifies_for_points attribute from CDP response and awards points if true.
fn handle_check_management_points_reply(
    deps: DepsMut,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(result) => {
            // Parse qualifies_for_points attribute from CDP response
            let qualifies = parse_string_attribute(&result, "qualifies_for_points")
                .map(|s| s == "true")
                .unwrap_or(false);

            let user = PENDING_USER.load(deps.storage)?;
            PENDING_USER.remove(deps.storage);

            if qualifies {
                // Award 5 points (with 6 decimals)
                let points = Decimal::from_ratio(MANAGEMENT_POINTS_REWARD, 1_000_000u128);

                let mut user_stats = USER_STATS.may_load(deps.storage, user.clone())?
                    .unwrap_or(UserStats {
                        total_points: Decimal::zero(),
                        claimable_points: Decimal::zero(),
                    });

                user_stats.total_points += points;
                user_stats.claimable_points += points;
                USER_STATS.save(deps.storage, user.clone(), &user_stats)?;

                Ok(Response::new()
                    .add_attribute("method", "check_management_points_reply")
                    .add_attribute("user", user.to_string())
                    .add_attribute("points_awarded", "5"))
            } else {
                Ok(Response::new()
                    .add_attribute("method", "check_management_points_reply")
                    .add_attribute("user", user.to_string())
                    .add_attribute("points_awarded", "0")
                    .add_attribute("reason", "positive_or_no_delta"))
            }
        }
        Err(err) => {
            PENDING_USER.remove(deps.storage);
            Err(StdError::generic_err(format!("Check debt delta failed: {}", err)))
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::ClaimCheck {} => to_json_binary(&CLAIM_CHECK.load(deps.storage)?),
        QueryMsg::UserStats { user, limit, start_after } => to_json_binary(&query_user_stats(deps, user, limit, start_after)?),
        QueryMsg::UserConversionRates { user, limit, start_after } => to_json_binary(&query_user_conversion_rates(deps, user, limit, start_after)?),
        QueryMsg::PointsMultipliers {} => to_json_binary(&membrane::points_system::PointsMultipliersResponse {
            points_multipliers: POINTS_MULTIPLIERS.load(deps.storage)?,
        }),
    }
}

/// Return a list of users with their points
fn query_user_stats(
    deps: Deps,
    user: Option<String>,
    limit: Option<u64>, //User limit
    start_after: Option<String>, //user
) -> StdResult<Vec<UserStatsResponse>> {
    if let Some(user) = user {
        let user = deps.api.addr_validate(&user)?;
        let stats = USER_STATS.load(deps.storage, user.clone())?;
        return Ok(vec![
            UserStatsResponse {
            user,
            stats
        }]);
    };

    let limit = limit.unwrap_or(PAGINATION_DEFAULT_LIMIT) as usize;
    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    } else {
        None
    };

    let mut user_stats: Vec<UserStatsResponse> = vec![];
    for user in USER_STATS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
    {
        let (user, stats) = user?;
        user_stats.push(
            UserStatsResponse {
            user,
            stats
        });
    }

    Ok(user_stats)
}


/// Return a list of users with their vault conversion rates
fn query_user_conversion_rates(
    deps: Deps,
    user: Option<String>,
    limit: Option<u64>, //User limit
    start_after: Option<String>, //user
) -> StdResult<Vec<UserConversionResponse>> {
    if let Some(user) = user {
        let user = deps.api.addr_validate(&user)?;
        let conversion_rates = USER_VAULT_CONVERSION_RATES.load(deps.storage, user.clone())?;
        return Ok(vec![
            UserConversionResponse {
            user,
            conversion_rates
        }]);
    };

    let limit = limit.unwrap_or(PAGINATION_DEFAULT_LIMIT) as usize;
    let start = if let Some(start) = start_after {
        let start_after_addr = deps.api.addr_validate(&start)?;
        Some(Bound::exclusive(start_after_addr))
    } else {
        None
    };

    let mut user_rates: Vec<UserConversionResponse> = vec![];
    for user in USER_VAULT_CONVERSION_RATES
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
    {
        let (user, conversion_rates) = user?;
        user_rates.push(
            UserConversionResponse {
            user,
            conversion_rates
        });
    }

    Ok(user_rates)
}

/// Handle voting result from emissions voting contract
/// Updates the corresponding PointsMultipliers field based on the graph label
fn execute_receive_voting_result(
    deps: DepsMut,
    info: MessageInfo,
    label: String,
    _result_uint128: Option<Uint128>,
    result_decimal: Option<Decimal>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    
    // Authorization: Only emissions_voting_contract can call this
    let emissions_voting = config.emissions_voting_contract.clone()
        .ok_or_else(|| ContractError::Unauthorized {})?;
    if info.sender != emissions_voting {
        return Err(ContractError::Unauthorized {});
    }
    
    // Get the decimal result (all points multiplier graphs are Decimal type)
    let new_value = result_decimal
        .ok_or_else(|| ContractError::Std(StdError::generic_err(
            "PointsMultipliers graphs must return Decimal result"
        )))?;
    
    // Load current multipliers
    let mut multipliers = POINTS_MULTIPLIERS.load(deps.storage)?;
    
    // Match label to the corresponding field
    let field_updated = match label.as_str() {
        "interest_rate" => {
            multipliers.interest_rate = new_value;
            true
        }
        "liquidation_execution" => {
            multipliers.liquidation_execution = new_value;
            true
        }
        "liquidation_claims" => {
            multipliers.liquidation_claims = new_value;
            true
        }
        "governance_votes" => {
            multipliers.governance_votes = new_value;
            true
        }
        "transmuter_swap_fees" => {
            multipliers.transmuter_swap_fees = new_value;
            true
        }
        "disco_revenue" => {
            multipliers.disco_revenue = new_value;
            true
        }
        _ => false, // Unknown label, ignore
    };
    
    if !field_updated {
        return Ok(Response::new()
            .add_attribute("action", "receive_voting_result")
            .add_attribute("status", "ignored")
            .add_attribute("label", label));
    }
    
    // Save updated multipliers
    POINTS_MULTIPLIERS.save(deps.storage, &multipliers)?;
    
    Ok(Response::new()
        .add_attribute("action", "receive_voting_result")
        .add_attribute("label", label)
        .add_attribute("new_value", new_value.to_string())
    )
}

/// Give points for affiliate fee distribution
fn give_points_for_affiliate_fee(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    affiliate: String,
    fee_amount: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Validate that the caller is the revenue distributor contract
    let revenue_distributor = config.revenue_distributor_contract
        .ok_or_else(|| ContractError::Std(StdError::generic_err("Revenue distributor contract not configured")))?;
    if info.sender != revenue_distributor {
        return Err(ContractError::Unauthorized {});
    }

    let querier = deps.querier;
    let storage = deps.storage;
    
    // Validate affiliate address
    let affiliate_addr = deps.api.addr_validate(&affiliate)?;
    
    // Get CDT price
    let basket: Basket = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.positions_contract.to_string(),
        msg: to_json_binary(&CDP_QueryMsg::GetBasket {})?,
    }))?;
    let cdt_price: PriceResponse = basket.credit_price;
    
    // Load points multipliers
    let points_multipliers = POINTS_MULTIPLIERS.load(storage)?;
    
    // Calculate value: fee_amount * multiplier (using interest_rate multiplier for affiliates)
    let fee_value = decimal_multiplication(
        cdt_price.get_value(fee_amount)?,
        points_multipliers.interest_rate.clone()
    )?;
    
    // Calculate base points
    let base_points = decimal_multiplication(fee_value, config.points_per_dollar)?;
    
    // Apply boost multiplier if system_discounts contract is configured
    let points = if let Some(discounts_addr) = &config.system_discounts_contract {
        // Query user's boost
        let boost_response: UserBoostResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: discounts_addr.to_string(),
            msg: to_json_binary(&SystemDiscounts_QueryMsg::UserBoost {
                user: affiliate.clone(),
            })?,
        }))?;
        
        // Calculate boost multiplier: 1 + boost_percentage
        let boost_multiplier = Decimal::one() + boost_response.boost;
        
        // Apply boost to points
        decimal_multiplication(base_points, boost_multiplier)?
    } else {
        base_points
    };
    
    // Save points to affiliate
    let mut user_stats = USER_STATS.may_load(storage, affiliate_addr.clone())?
        .unwrap_or(UserStats {
            total_points: Decimal::zero(),
            claimable_points: Decimal::zero(),
        });
    user_stats.total_points += points;
    user_stats.claimable_points += points;
    USER_STATS.save(storage, affiliate_addr, &user_stats)?;
    
    Ok(Response::new()
        .add_attribute("method", "give_points_for_affiliate_fee")
        .add_attribute("affiliate", affiliate)
        .add_attribute("fee_amount", fee_amount.to_string())
        .add_attribute("points", points.to_string()))
}

/// Give points for manager fee distribution
fn give_points_for_manager_fee(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    manager: String,
    fee_amount: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Validate that the caller is the disco contract
    if info.sender != config.ltv_disco_contract.unwrap() {
        return Err(ContractError::Unauthorized {});
    }

    let querier = deps.querier;
    let storage = deps.storage;
    
    // Validate manager address
    let manager_addr = deps.api.addr_validate(&manager)?;
    
    // Get CDT price
    let basket: Basket = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.positions_contract.to_string(),
        msg: to_json_binary(&CDP_QueryMsg::GetBasket {})?,
    }))?;
    let cdt_price: PriceResponse = basket.credit_price;
    
    // Load points multipliers
    let points_multipliers = POINTS_MULTIPLIERS.load(storage)?;
    
    // Calculate value: fee_amount * multiplier (using disco_revenue multiplier for disco managers)
    let fee_value = decimal_multiplication(
        cdt_price.get_value(fee_amount)?,
        points_multipliers.disco_revenue.clone()
    )?;
    
    // Calculate base points
    let base_points = decimal_multiplication(fee_value, config.points_per_dollar)?;
    
    // Apply boost multiplier if system_discounts contract is configured
    let points = if let Some(discounts_addr) = &config.system_discounts_contract {
        // Query user's boost
        let boost_response: UserBoostResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: discounts_addr.to_string(),
            msg: to_json_binary(&SystemDiscounts_QueryMsg::UserBoost {
                user: manager.clone(),
            })?,
        }))?;
        
        // Calculate boost multiplier: 1 + boost_percentage
        let boost_multiplier = Decimal::one() + boost_response.boost;
        
        // Apply boost to points
        decimal_multiplication(base_points, boost_multiplier)?
    } else {
        base_points
    };
    
    // Save points to manager
    let mut user_stats = USER_STATS.may_load(storage, manager_addr.clone())?
        .unwrap_or(UserStats {
            total_points: Decimal::zero(),
            claimable_points: Decimal::zero(),
        });
    user_stats.total_points += points;
    user_stats.claimable_points += points;
    USER_STATS.save(storage, manager_addr, &user_stats)?;
    
    Ok(Response::new()
        .add_attribute("method", "give_points_for_manager_fee")
        .add_attribute("manager", manager)
        .add_attribute("fee_amount", fee_amount.to_string())
        .add_attribute("points", points.to_string()))
}

/// Static points award for management through volatile window
const MANAGEMENT_POINTS_REWARD: u128 = 5_000_000; // 5 points with 6 decimals

/// Called by CDP when user exits volatile window with negative debt delta (repaid during volatility).
/// Awards 5 management points to the user.
fn cdp_gives_user_management_points(
    deps: DepsMut,
    info: MessageInfo,
    user: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only CDP contract can call this
    if info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }

    let user_addr = deps.api.addr_validate(&user)?;

    // Award 5 points (with 6 decimals)
    let points = Decimal::from_ratio(MANAGEMENT_POINTS_REWARD, 1_000_000u128);

    let mut user_stats = USER_STATS.may_load(deps.storage, user_addr.clone())?
        .unwrap_or(UserStats {
            total_points: Decimal::zero(),
            claimable_points: Decimal::zero(),
        });

    user_stats.total_points += points;
    user_stats.claimable_points += points;
    USER_STATS.save(deps.storage, user_addr, &user_stats)?;

    Ok(Response::new()
        .add_attribute("action", "cdp_gives_user_management_points")
        .add_attribute("user", user)
        .add_attribute("points_awarded", "5"))
}

/// Reply ID for check management points
const CHECK_MANAGEMENT_POINTS_REPLY_ID: u64 = 6u64;

/// Permissionless call to check and award management points.
/// Queries CDP for volatility window status and triggers debt delta check.
fn check_management_points(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    user: String,
    position_id: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let user_addr = deps.api.addr_validate(&user)?;

    // Query user's position to get collateral assets
    let positions: Vec<membrane::cdp::BasketPositionsResponse> = deps.querier.query(&QueryRequest::Wasm(
        WasmQuery::Smart {
            contract_addr: config.positions_contract.to_string(),
            msg: to_json_binary(&CDP_QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: Some(UserInfo {
                    position_owner: user.clone(),
                    position_id,
                }),
                user: None,
            })?,
        }
    ))?;

    if positions.is_empty() || positions[0].positions.is_empty() {
        return Err(ContractError::Std(StdError::generic_err("Position not found")));
    }

    // Get asset denoms from position
    let assets: Vec<String> = positions[0].positions[0].collateral_assets
        .iter()
        .map(|a| a.asset.info.to_string())
        .collect();

    // Query CDP for volatility windows
    let volatility: membrane::cdp::VolatilityWindowResponse = deps.querier.query(&QueryRequest::Wasm(
        WasmQuery::Smart {
            contract_addr: config.positions_contract.to_string(),
            msg: to_json_binary(&CDP_QueryMsg::CheckVolatilityWindow { assets })?,
        }
    ))?;

    // If ANY asset is in volatile window, cannot claim
    if volatility.in_volatile_window.iter().any(|&v| v) {
        return Err(ContractError::Std(StdError::generic_err(
            "Position is still in volatile window"
        )));
    }

    // Call CDP to check and clear debt delta
    let check_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.positions_contract.to_string(),
        msg: to_json_binary(&CDP_ExecuteMsg::CheckAndClearDebtDelta {
            position_owner: user.clone(),
            position_id,
        })?,
        funds: vec![],
    });

    // Use submessage to check result and award points based on response
    let submsg = SubMsg::reply_on_success(check_msg, CHECK_MANAGEMENT_POINTS_REPLY_ID);

    // Save user for reply handler
    PENDING_USER.save(deps.storage, &user_addr)?;

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "check_management_points")
        .add_attribute("user", user)
        .add_attribute("position_id", position_id.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {

    
    Ok(Response::default())
}
