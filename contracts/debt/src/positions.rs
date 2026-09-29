#![allow(non_snake_case)]
use cosmwasm_std::{
    attr, to_json_binary, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response,
    SubMsg, Uint128, WasmMsg,
};

use membrane::chain_proxy::ExecuteMsg as ChainProxyExecuteMsg;
use membrane::collateral::ExecuteMsg as CollateralExecuteMsg;
use membrane::debt::{AccrueDebtResponse, DebtPosition};
use membrane::collateral::{
    PositionValueResponse, QueryMsg as CollateralQueryMsg,
};
use membrane::deployable_venue::{
    ExecuteMsg as DeployableVenueExecuteMsg, QueryMsg as DeployableVenueQueryMsg,
};
use membrane::math::{decimal_division, decimal_multiplication};
use membrane::types::{
    AffiliateData, AssetInfo, DebtSplit, DeploymentIntent, DeploymentVenue,
    StringEntry, UserDeploymentIntents, UserInfo,
};

use crate::debt_manager::{
    apply_delta_to_caps, apply_delta_to_peg_debt, apply_delta_to_regular_debt,
    reduce_segments_proportionally, repay_segments_default_order, repay_segments_targeted,
    SegmentSplit,
};
use crate::error::ContractError;
use crate::rates::{accrue_position, get_total_position_debt};
use crate::state::{
    set_active_deployment_venues,
    AFFILIATES, BASKET, CDT_SUPPLY, CLOSE_POSITION_PROPAGATION,
    CLOSE_POSITION_SELL_REPLY_ID, COLLATERAL_REFRESH_REPLY_ID, CONFIG, FROZEN,
    INCREASE_DEBT_PROPAGATION, POSITIONS, RATES, USER_INTENTS,
};

/// Increase debt of a position.
/// Sends SubMsg to Collateral.RefreshAndAssess to get fresh collateral values,
/// then on reply validates solvency and mints CDT.
///
/// CDP parity: maps to cdp/positions.rs::increase_debt()
#[allow(clippy::too_many_arguments)]
pub fn increase_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    amount: Option<Uint128>,
    ltv: Option<Decimal>,
    mint_to_addr: Option<String>,
    _deployment_intent: Option<DeploymentIntent>,
    debt_split: Option<DebtSplit>,
    _rollover_updates: Option<Vec<(u8, bool)>>,
    peg_debt: Option<bool>,
) -> Result<Response, ContractError> {
    // Check frozen
    if FROZEN.load(deps.storage)? {
        return Err(ContractError::Frozen {});
    }

    let config = CONFIG.load(deps.storage)?;
    let mut basket = BASKET.load(deps.storage)?;
    let owner = info.sender.clone();

    // Load position
    let (_index, mut position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;

    // Step 1: Accrue interest locally
    accrue_position(
        deps.storage,
        deps.querier,
        env.clone(),
        &config,
        &mut position,
        &mut basket,
        owner.to_string(),
    )?;

    // Save accrued position and basket
    crate::state::update_position(deps.storage, owner.clone(), position)?;
    BASKET.save(deps.storage, &basket)?;

    // Step 2: Save propagation state for reply handler
    INCREASE_DEBT_PROPAGATION.save(
        deps.storage,
        &crate::state::IncreaseDebtPropagation {
            position_owner: owner.to_string(),
            position_id,
            amount,
            ltv,
            mint_to_addr,
            peg_debt: peg_debt.unwrap_or(false),
            debt_split,
        },
    )?;

    // Step 3: Send SubMsg to Collateral.RefreshAndAssess to get fresh values
    let collateral_contract = config
        .collateral_contract
        .ok_or(ContractError::CustomError {
            val: "No collateral contract configured".to_string(),
        })?;

    let refresh_msg = WasmMsg::Execute {
        contract_addr: collateral_contract.to_string(),
        msg: to_json_binary(&CollateralExecuteMsg::RefreshAndAssess {
            position_owner: owner.to_string(),
            position_id,
        })?,
        funds: vec![],
    };

    Ok(Response::new()
        .add_submessage(SubMsg::reply_on_success(refresh_msg, COLLATERAL_REFRESH_REPLY_ID))
        .add_attribute("method", "increase_debt")
        .add_attribute("position_id", position_id.to_string()))
}

/// Repay debt of a position.
///
/// CDP parity: maps to cdp/positions.rs::repay()
pub fn repay(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: Option<String>,
    send_excess_to: Option<String>,
    debt_split: Option<DebtSplit>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut basket = BASKET.load(deps.storage)?;
    let mut rates = RATES.load(deps.storage)?;

    let owner = if let Some(owner_str) = position_owner {
        deps.api.addr_validate(&owner_str)?
    } else {
        info.sender.clone()
    };

    let (_index, mut position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;

    // Step 1: Accrue interest locally
    accrue_position(
        deps.storage,
        deps.querier,
        env.clone(),
        &config,
        &mut position,
        &mut basket,
        owner.to_string(),
    )?;

    // Record prev debt for state validation
    let _prev_credit_amount = get_total_position_debt(&position);

    // Step 2: Determine repay amount from info.funds (CDT sent by user)
    let credit_denom = match &basket.credit_asset.info {
        AssetInfo::NativeToken { denom } => denom.clone(),
        AssetInfo::Token { address } => address.to_string(),
    };
    let repay_amount = info
        .funds
        .iter()
        .find(|coin| coin.denom == credit_denom)
        .map(|coin| coin.amount)
        .unwrap_or(Uint128::zero());

    if repay_amount.is_zero() {
        return Err(ContractError::NoDebt {});
    }

    // Step 3: Apply repayment to segments
    #[allow(unused_assignments)]
    let mut regular_deltas = crate::debt_manager::SegmentDeltas::default();
    let mut peg_deltas = crate::debt_manager::SegmentDeltas::default();
    let mut excess_repayment = Uint128::zero();

    if let Some(split) = debt_split {
        // Validate all split fractions sum to 1.0 (regular + peg)
        let total_split: Decimal = [
            split.variable.unwrap_or(Decimal::zero()),
            split.one_month.unwrap_or(Decimal::zero()),
            split.three_month.unwrap_or(Decimal::zero()),
            split.six_month.unwrap_or(Decimal::zero()),
            split.peg_variable.unwrap_or(Decimal::zero()),
            split.peg_one_month.unwrap_or(Decimal::zero()),
            split.peg_three_month.unwrap_or(Decimal::zero()),
            split.peg_six_month.unwrap_or(Decimal::zero()),
        ]
        .iter()
        .sum();

        if total_split != Decimal::one() {
            return Err(ContractError::InvalidDebtSplit {});
        }

        // Build splits for each pool
        let regular_split = SegmentSplit {
            variable: split.variable,
            one_month: split.one_month,
            three_month: split.three_month,
            six_month: split.six_month,
            ..SegmentSplit::default()
        };
        let peg_split = SegmentSplit {
            variable: split.peg_variable,
            one_month: split.peg_one_month,
            three_month: split.peg_three_month,
            six_month: split.peg_six_month,
            ..SegmentSplit::default()
        };

        // Targeted repay: regular pool
        let (regular_applied, reg_d) = repay_segments_targeted(
            &mut position.rate_segments,
            repay_amount,
            &regular_split,
        )?;
        regular_deltas = reg_d;

        // Targeted repay: peg pool
        let (peg_applied, peg_d) = repay_segments_targeted(
            &mut position.peg_rate_segments,
            repay_amount,
            &peg_split,
        )?;
        peg_deltas = peg_d;

        excess_repayment = repay_amount
            .checked_sub(regular_applied + peg_applied)
            .unwrap_or(Uint128::zero());
    } else {
        // Default repayment order: regular pool first (variable -> 1mo -> 3mo -> 6mo), then peg pool
        let (regular_applied, reg_d) =
            repay_segments_default_order(&mut position.rate_segments, repay_amount);
        regular_deltas = reg_d;

        let remaining = repay_amount
            .checked_sub(regular_applied)
            .unwrap_or(Uint128::zero());

        if remaining > Uint128::zero() {
            let (peg_applied, peg_d) =
                repay_segments_default_order(&mut position.peg_rate_segments, remaining);
            peg_deltas = peg_d;

            excess_repayment = remaining
                .checked_sub(peg_applied)
                .unwrap_or(Uint128::zero());
        }
    }

    let total_debt_after_repay = get_total_position_debt(&position);
    let actual_repaid = repay_amount
        .checked_sub(excess_repayment)
        .unwrap_or(Uint128::zero());

    // Step 4: Apply deltas to basket and rates
    apply_delta_to_regular_debt(&mut basket.credit_asset, &regular_deltas);
    apply_delta_to_peg_debt(&mut basket.credit_asset, &peg_deltas);
    apply_delta_to_caps(&mut rates.fixed_rate_caps, &regular_deltas);
    apply_delta_to_caps(&mut rates.fixed_rate_caps, &peg_deltas);

    // Step 5: Handle interest -> revenue
    let total_interest_paid = std::cmp::min(position.pending_interest, actual_repaid);
    position.pending_interest = position
        .pending_interest
        .checked_sub(total_interest_paid)
        .unwrap_or(Uint128::zero());

    // Step 6: If debt reaches zero, SubMsg to Collateral.DeactivateSupplyCaps
    let mut msgs: Vec<SubMsg> = vec![];

    if total_debt_after_repay.is_zero() {
        if let Some(ref collateral_contract) = config.collateral_contract {
            let deactivate_msg = WasmMsg::Execute {
                contract_addr: collateral_contract.to_string(),
                msg: to_json_binary(&CollateralExecuteMsg::DeactivateSupplyCaps {
                    position_id,
                    position_owner: owner.to_string(),
                })?,
                funds: vec![],
            };
            msgs.push(SubMsg::new(deactivate_msg));
        }
    }

    // Step 7: Burn CDT via chain proxy
    if actual_repaid > Uint128::zero() {
        if let Some(ref chain_proxy) = config.chain_proxy {
            let burn_msg = WasmMsg::Execute {
                contract_addr: chain_proxy.to_string(),
                msg: to_json_binary(&ChainProxyExecuteMsg::BurnTokens {
                    denom: credit_denom.clone(),
                    amount: actual_repaid,
                    burn_from_address: env.contract.address.to_string(),
                })?,
                funds: vec![],
            };
            msgs.push(SubMsg::new(burn_msg));
        }
        // Track CDT supply decrease
        let mut cdt_supply = CDT_SUPPLY.load(deps.storage)?;
        cdt_supply = cdt_supply
            .checked_sub(actual_repaid)
            .unwrap_or(Uint128::zero());
        CDT_SUPPLY.save(deps.storage, &cdt_supply)?;
    }

    // Step 8: Send back excess repayment
    if !excess_repayment.is_zero() {
        let send_to = if let Some(addr) = send_excess_to {
            deps.api.addr_validate(&addr)?
        } else {
            info.sender.clone()
        };
        let send_msg = BankMsg::Send {
            to_address: send_to.to_string(),
            amount: vec![Coin {
                denom: credit_denom,
                amount: excess_repayment,
            }],
        };
        msgs.push(SubMsg::new(send_msg));
    }

    // Save state
    RATES.save(deps.storage, &rates)?;
    BASKET.save(deps.storage, &basket)?;
    crate::state::update_position(deps.storage, owner, position)?;

    Ok(Response::new()
        .add_submessages(msgs)
        .add_attributes(vec![
            attr("method", "repay"),
            attr("position_id", position_id),
            attr("repay_amount", actual_repaid),
            attr("excess_repayment", excess_repayment),
            attr("total_interest_paid", total_interest_paid),
            attr("debt_after", total_debt_after_repay),
        ]))
}

/// Close a position by selling collateral and repaying debt.
///
/// CDP parity: maps to cdp/positions.rs::close_position()
pub fn close_position(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    close_percentage: Option<Decimal>,
    max_spread: Decimal,
    send_to: Option<String>,
) -> Result<Response, ContractError> {
    if FROZEN.load(deps.storage)? {
        return Err(ContractError::Frozen {});
    }

    let config = CONFIG.load(deps.storage)?;
    let mut basket = BASKET.load(deps.storage)?;
    let owner = info.sender.clone();

    let (_index, mut position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;

    let close_pct = close_percentage.unwrap_or(Decimal::one());

    // Step 1: Accrue interest
    accrue_position(
        deps.storage,
        deps.querier,
        env.clone(),
        &config,
        &mut position,
        &mut basket,
        owner.to_string(),
    )?;

    // Save accrued state
    crate::state::update_position(deps.storage, owner.clone(), position)?;
    BASKET.save(deps.storage, &basket)?;

    // Step 2: Save ClosePositionPropagation state for reply chain
    CLOSE_POSITION_PROPAGATION.save(
        deps.storage,
        &crate::state::ClosePositionPropagation {
            position_info: membrane::types::UserInfo {
                position_id,
                position_owner: owner.to_string(),
            },
            close_percentage: close_pct,
            max_spread,
            send_to: send_to.clone(),
        },
    )?;

    // Step 3: SubMsg to Collateral.AuthorizedSellCollateral
    let collateral_contract = config
        .collateral_contract
        .ok_or(ContractError::CustomError {
            val: "No collateral contract configured".to_string(),
        })?;

    let sell_msg = WasmMsg::Execute {
        contract_addr: collateral_contract.to_string(),
        msg: to_json_binary(&CollateralExecuteMsg::AuthorizedSellCollateral {
            position_id,
            position_owner: owner.to_string(),
            max_spread,
            send_proceeds_to: env.contract.address.to_string(),
            sell_amounts: None,
        })?,
        funds: vec![],
    };

    Ok(Response::new()
        .add_submessage(SubMsg::reply_on_success(sell_msg, CLOSE_POSITION_SELL_REPLY_ID))
        .add_attribute("method", "close_position")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("close_percentage", close_pct.to_string()))
}

/// Set user deployment intents.
/// Add, edit, or remove a deployment intent for a user's position.
/// If ltv_to_mint is 0 on an existing intent, removes the intent and exits the venue.
///
/// CDP parity: maps to cdp/positions.rs::set_intents()
pub fn set_user_intents(
    deps: DepsMut,
    info: MessageInfo,
    deployment_intent: DeploymentIntent,
) -> Result<Response, ContractError> {
    let mut msgs: Vec<CosmosMsg> = vec![];

    // Validate ownership by loading the target position
    let (_index, mut position) = crate::state::get_target_position(
        deps.storage,
        info.sender.clone(),
        deployment_intent.position_id,
    )?;

    // Load or create UserIntents
    let mut user_intents = USER_INTENTS
        .may_load(deps.storage, info.sender.clone())?
        .unwrap_or(UserDeploymentIntents {
            user: info.sender.to_string(),
            deployment_intents: vec![],
        });

    // Validate ltv_to_mint
    if deployment_intent.ltv_to_mint > Decimal::one() {
        return Err(ContractError::CustomError {
            val: String::from("Mint LTV is above 1, maybe you forgot to add the decimal place?"),
        });
    }

    // Add, edit, or remove intent
    if let Some((index, _)) = user_intents
        .deployment_intents
        .iter()
        .enumerate()
        .find(|(_i, intent)| {
            intent.position_id == deployment_intent.position_id
                && intent.destination == deployment_intent.destination
        })
    {
        // Existing intent found for same position+destination
        if deployment_intent.ltv_to_mint.is_zero() {
            // Remove intent
            user_intents.deployment_intents.remove(index);

            // Find venue in user's position and exit
            let dest_addr = deps.api.addr_validate(&deployment_intent.destination)?;
            let venue = position
                .deployed_to
                .iter()
                .enumerate()
                .find(|(_, v)| v.address == dest_addr);

            if let Some((venue_index, venue_data)) = venue {
                // Exit vault & repay user debt
                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deployment_intent.destination.clone(),
                    msg: to_json_binary(&DeployableVenueExecuteMsg::RepayUserDebt {
                        user_info: UserInfo {
                            position_id: deployment_intent.position_id,
                            position_owner: info.sender.to_string(),
                        },
                        repayment: venue_data.deployed_debt_amount,
                    })?,
                    funds: vec![],
                }));

                // Remove venue from position
                position.deployed_to.remove(venue_index);

                // Update active deployment venues
                set_active_deployment_venues(
                    deps.storage,
                    vec![StringEntry {
                        entry: deployment_intent.destination.to_string(),
                        remove: true,
                    }],
                )?;

                // Save updated position
                crate::state::update_position(deps.storage, info.sender.clone(), position)?;
            }
        } else {
            // Update LTV on existing intent
            user_intents.deployment_intents[index].ltv_to_mint =
                deployment_intent.ltv_to_mint;
        }
    } else {
        // New intent - add it
        user_intents
            .deployment_intents
            .push(deployment_intent.clone());
    }

    // Save or remove user intents
    if user_intents.deployment_intents.is_empty() {
        USER_INTENTS.remove(deps.storage, info.sender.clone());
    } else {
        USER_INTENTS.save(deps.storage, info.sender.clone(), &user_intents)?;
    }

    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("method", "set_user_intents")
        .add_attribute("deployment_intent", format!("{:?}", deployment_intent)))
}

/// Fulfill minting intents.
/// For each user's deployment intents, calculate available LTV space and mint
/// debt to the deployment venue.
///
/// CDP parity: maps to cdp/positions.rs::fulfill_intents()
pub fn fulfill_intents(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    users: Vec<String>,
) -> Result<Response, ContractError> {
    let mut msgs: Vec<CosmosMsg> = vec![];
    let config = CONFIG.load(deps.storage)?;
    let basket = BASKET.load(deps.storage)?;

    let collateral_contract = config
        .collateral_contract
        .clone()
        .ok_or(ContractError::CustomError {
            val: "No collateral contract configured".to_string(),
        })?;

    for user in users {
        let user_addr = deps.api.addr_validate(&user)?;

        // Load intents for user
        let intents = USER_INTENTS.load(deps.storage, user_addr.clone())?;

        for intent in intents.deployment_intents {
            // Get target position
            let (_index, mut position) = crate::state::get_target_position(
                deps.storage,
                user_addr.clone(),
                intent.position_id,
            )?;

            // Query Collateral contract for position value and LTV info
            let position_value: PositionValueResponse = deps.querier.query_wasm_smart(
                collateral_contract.to_string(),
                &CollateralQueryMsg::GetPositionValue {
                    position_owner: user.clone(),
                    position_id: intent.position_id,
                },
            )?;

            let total_value = position_value.total_value;
            let max_borrow_ltv = position_value.avg_max_borrow_ltv;

            // Calculate current LTV
            let total_debt = get_total_position_debt(&position);
            let debt_value = basket.credit_price.get_value(total_debt)?;
            let current_ltv = if total_value.is_zero() {
                Decimal::zero()
            } else {
                decimal_division(debt_value, total_value)?
            };

            // Calc LTV space to mint
            let ltv_space_to_mint = max_borrow_ltv
                .checked_sub(current_ltv)
                .unwrap_or(Decimal::zero());

            if ltv_space_to_mint > Decimal::zero() {
                // Get or create deployment venue
                let intent_dest_addr = deps.api.addr_validate(&intent.destination)?;
                let deployment_venue = position
                    .deployed_to
                    .iter()
                    .find(|v| v.address == intent_dest_addr)
                    .cloned()
                    .unwrap_or(DeploymentVenue {
                        address: intent_dest_addr.clone(),
                        deployed_debt_amount: Uint128::zero(),
                        failed_liquidation: false,
                    });

                // Calculate how much LTV the deployment venue is currently using
                let debt_value_used =
                    basket.credit_price.get_value(deployment_venue.deployed_debt_amount)?;
                let ltv_used = if total_value.is_zero() {
                    Decimal::zero()
                } else {
                    decimal_division(debt_value_used, total_value)?
                };

                // Calculate usable LTV for this venue
                let usable_ltv = intent
                    .ltv_to_mint
                    .checked_sub(ltv_used)
                    .unwrap_or(Decimal::zero());

                // Calculate amount of debt to mint
                let usable_debt_value = decimal_multiplication(total_value, usable_ltv)?;
                let usable_debt_amount = basket.credit_price.get_amount(usable_debt_value)?;

                if !usable_debt_amount.is_zero() {
                    // Create IncreaseDebt self-call
                    msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: env.contract.address.to_string(),
                        msg: to_json_binary(&membrane::debt::ExecuteMsg::IncreaseDebt {
                            position_id: intent.position_id,
                            amount: Some(usable_debt_amount),
                            LTV: None,
                            mint_to_addr: None,
                            deployment_intent: Some(intent.clone()),
                            debt_split: None,
                            rollover_updates: None,
                            peg_debt: None,
                        })?,
                        funds: vec![],
                    }));

                    // Add/update deployment venue on target position
                    if let Some(existing) = position
                        .deployed_to
                        .iter_mut()
                        .find(|v| v.address == intent_dest_addr)
                    {
                        existing.deployed_debt_amount += usable_debt_amount;
                    } else {
                        position.deployed_to.push(DeploymentVenue {
                            address: intent_dest_addr,
                            deployed_debt_amount: usable_debt_amount,
                            failed_liquidation: false,
                        });
                        // Update active deployment venues
                        set_active_deployment_venues(
                            deps.storage,
                            vec![StringEntry {
                                entry: intent.destination.to_string(),
                                remove: false,
                            }],
                        )?;
                    }

                    // Save target position
                    crate::state::update_position(
                        deps.storage,
                        user_addr.clone(),
                        position,
                    )?;
                }
            }
        }
    }

    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("method", "fulfill_intents"))
}

/// Set deployment venue for a position.
/// Adds or updates a deployment venue on the user's position.
/// If initial_deployed_debt_amount is None, queries the venue for RetrievableCDT.
///
/// CDP parity: maps to cdp/contract.rs::set_deployment_venue()
pub fn set_deployment_venue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    position_id: Uint128,
    venue_address: String,
    initial_deployed_debt_amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    // Validate ownership
    let (_index, mut position) =
        crate::state::get_target_position(deps.storage, info.sender.clone(), position_id)?;

    let venue_addr = deps.api.addr_validate(&venue_address)?;

    // Determine deployed debt amount
    let deployed_debt_amount = if let Some(amount) = initial_deployed_debt_amount {
        amount
    } else {
        // Query the deployable venue contract for RetrievableCDT
        deps.querier.query_wasm_smart::<Uint128>(
            venue_address.clone(),
            &DeployableVenueQueryMsg::RetrievableCDT {
                user: info.sender.to_string(),
            },
        )?
    };

    // Add or update deployment venue
    if let Some(existing) = position
        .deployed_to
        .iter_mut()
        .find(|v| v.address == venue_addr)
    {
        existing.deployed_debt_amount = deployed_debt_amount;
    } else {
        position.deployed_to.push(DeploymentVenue {
            address: venue_addr,
            deployed_debt_amount,
            failed_liquidation: false,
        });
        // Update active deployment venues for new venues
        set_active_deployment_venues(
            deps.storage,
            vec![StringEntry {
                entry: venue_address.clone(),
                remove: false,
            }],
        )?;
    }

    // Save position
    crate::state::update_position(deps.storage, info.sender, position)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "set_deployment_venue"),
        attr("position_id", position_id.to_string()),
        attr("venue_address", venue_address),
        attr("deployed_debt_amount", deployed_debt_amount.to_string()),
    ]))
}

/// Set affiliate for a position.
///
/// CDP parity: maps to cdp/positions.rs::set_affiliate()
pub fn set_affiliate(
    deps: DepsMut,
    info: MessageInfo,
    position_id: Uint128,
    affiliate_address: String,
    affiliate_fee: Decimal,
    label: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    if affiliate_fee > config.affiliate_fee_max {
        return Err(ContractError::CustomError {
            val: format!("Affiliate fee exceeds max: {}", config.affiliate_fee_max),
        });
    }

    let affiliate_data = AffiliateData {
        affiliate_address: affiliate_address.clone(),
        affiliate_fee,
        label,
        time_affiliated: 0,
    };
    AFFILIATES.save(deps.storage, position_id.u128(), &affiliate_data)?;

    Ok(Response::new()
        .add_attribute("method", "set_affiliate")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("affiliate_address", affiliate_address))
}

/// Fulfill bad debt with CDT.
///
/// CDP parity: maps to cdp/positions.rs::fulfill_bad_debt()
pub fn fulfill_bad_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut basket = BASKET.load(deps.storage)?;

    // Determine credit denom
    let credit_denom = match &basket.credit_asset.info {
        AssetInfo::NativeToken { denom } => denom.clone(),
        AssetInfo::Token { address } => address.to_string(),
    };

    // Accept CDT from info.funds
    let cdt_sent = info
        .funds
        .iter()
        .find(|coin| coin.denom == credit_denom)
        .map(|coin| coin.amount)
        .unwrap_or(Uint128::zero());

    if cdt_sent.is_zero() {
        return Err(ContractError::CustomError {
            val: "No CDT sent to fulfill bad debt".to_string(),
        });
    }

    // Reduce pending_bad_debt (cap at actual bad debt)
    let applied = std::cmp::min(cdt_sent, basket.pending_bad_debt);
    basket.pending_bad_debt = basket
        .pending_bad_debt
        .checked_sub(applied)
        .unwrap_or(Uint128::zero());

    BASKET.save(deps.storage, &basket)?;

    // Burn the CDT via chain proxy
    let mut msgs: Vec<SubMsg> = vec![];
    if let Some(ref chain_proxy) = config.chain_proxy {
        let burn_msg = WasmMsg::Execute {
            contract_addr: chain_proxy.to_string(),
            msg: to_json_binary(&ChainProxyExecuteMsg::BurnTokens {
                denom: credit_denom.clone(),
                amount: applied,
                burn_from_address: env.contract.address.to_string(),
            })?,
            funds: vec![],
        };
        msgs.push(SubMsg::new(burn_msg));
    }

    // Track CDT supply decrease
    let mut cdt_supply = CDT_SUPPLY.load(deps.storage)?;
    cdt_supply = cdt_supply.checked_sub(applied).unwrap_or(Uint128::zero());
    CDT_SUPPLY.save(deps.storage, &cdt_supply)?;

    // Return excess if any
    let excess = cdt_sent.checked_sub(applied).unwrap_or(Uint128::zero());
    if !excess.is_zero() {
        let send_back = BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: credit_denom,
                amount: excess,
            }],
        };
        msgs.push(SubMsg::new(send_back));
    }

    Ok(Response::new()
        .add_submessages(msgs)
        .add_attribute("method", "fulfill_bad_debt")
        .add_attribute("applied", applied.to_string())
        .add_attribute("remaining_bad_debt", basket.pending_bad_debt.to_string()))
}

/// Take revenue from pending_revenue.
///
/// CDP parity: maps to cdp/positions.rs::take_revenue()
pub fn take_revenue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut basket = BASKET.load(deps.storage)?;

    // Only owner or revenue_distributor can take revenue
    if info.sender != config.owner && Some(info.sender.clone()) != config.revenue_distributor {
        return Err(ContractError::Unauthorized {});
    }

    if !basket.distribute_revenue {
        return Err(ContractError::CustomError {
            val: "Revenue distribution is disabled".to_string(),
        });
    }

    let pending = basket.pending_revenue.total_pending;
    if pending.is_zero() {
        return Err(ContractError::CustomError {
            val: "No pending revenue".to_string(),
        });
    }

    let credit_denom = match &basket.credit_asset.info {
        AssetInfo::NativeToken { denom } => denom.clone(),
        AssetInfo::Token { address } => address.to_string(),
    };

    let mut msgs: Vec<SubMsg> = vec![];

    // Mint CDT for pending revenue via chain proxy
    let revenue_recipient = config
        .revenue_distributor
        .clone()
        .unwrap_or(config.owner.clone());

    if let Some(ref chain_proxy) = config.chain_proxy {
        let mint_msg = WasmMsg::Execute {
            contract_addr: chain_proxy.to_string(),
            msg: to_json_binary(&ChainProxyExecuteMsg::MintTokens {
                denom: credit_denom,
                amount: pending,
                mint_to_address: revenue_recipient.to_string(),
            })?,
            funds: vec![],
        };
        msgs.push(SubMsg::new(mint_msg));
    }

    // Track CDT supply increase
    let mut cdt_supply = CDT_SUPPLY.load(deps.storage)?;
    cdt_supply += pending;
    CDT_SUPPLY.save(deps.storage, &cdt_supply)?;

    // Reset pending_revenue
    basket.pending_revenue.total_pending = Uint128::zero();
    basket.pending_revenue.per_asset_rev = vec![];
    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new()
        .add_submessages(msgs)
        .add_attribute("method", "take_revenue")
        .add_attribute("revenue_amount", pending.to_string())
        .add_attribute("recipient", revenue_recipient.to_string()))
}

/// Check and clear position's volatile window debt delta.
/// Called by Points contract for management points.
///
/// CDP parity: maps to cdp/positions.rs::check_and_clear_debt_delta()
pub fn check_and_clear_debt_delta(
    deps: DepsMut,
    info: MessageInfo,
    position_owner: String,
    position_id: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if Some(info.sender) != config.points_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, mut position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;

    // Check vol_window_initial_debt vs current debt
    let current_debt = get_total_position_debt(&position);
    let initial_debt = position.vol_window_initial_debt.unwrap_or(Uint128::zero());

    let debt_delta = if current_debt >= initial_debt {
        current_debt - initial_debt
    } else {
        Uint128::zero()
    };

    // Clear vol_window_initial_debt
    position.vol_window_initial_debt = None;

    crate::state::update_position(deps.storage, owner, position)?;

    Ok(Response::new()
        .add_attribute("method", "check_and_clear_debt_delta")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("debt_delta", debt_delta.to_string())
        .add_attribute("initial_debt", initial_debt.to_string())
        .add_attribute("current_debt", current_debt.to_string()))
}

// ============================================================================
// Authorized calls from Collateral contract
// ============================================================================

/// Notification of a new deposit for accrual + affiliate setup.
/// Only callable by Collateral contract.
///
/// CDP parity: this is new in the modular design - replaces inline deposit logic in CDP
pub fn notify_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
    affiliate_address: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if Some(info.sender.clone()) != config.collateral_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;

    // Create empty DebtPosition if this is a new position
    let positions = POSITIONS
        .may_load(deps.storage, owner.clone())?
        .unwrap_or_default();

    if !positions.iter().any(|p| p.position_id == position_id) {
        let new_position = DebtPosition {
            position_id,
            rate_segments: vec![],
            peg_rate_segments: vec![],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![],
        };
        let mut positions = positions;
        positions.push(new_position);
        POSITIONS.save(deps.storage, owner.clone(), &positions)?;
    }

    // Setup affiliate if provided and not already set
    if let Some(affiliate_addr) = affiliate_address {
        if AFFILIATES
            .may_load(deps.storage, position_id.u128())?
            .is_none()
        {
            let affiliate_data = AffiliateData {
                affiliate_address: affiliate_addr,
                affiliate_fee: Decimal::zero(), // Default fee, owner can update via SetAffiliate
                label: None,
                time_affiliated: env.block.time.seconds(),
            };
            AFFILIATES.save(deps.storage, position_id.u128(), &affiliate_data)?;
        }
    }

    Ok(Response::new()
        .add_attribute("method", "notify_deposit")
        .add_attribute("position_id", position_id.to_string()))
}

/// Accrue interest and validate solvency for a withdrawal.
/// Returns debt data via response attributes. Errors if insolvent.
/// Only callable by Collateral contract.
///
/// CDP parity: maps to the accrue+insolvency_check part of cdp/positions.rs::withdraw()
pub fn accrue_and_validate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_owner: String,
    position_id: Uint128,
    remaining_collateral_value: Decimal,
    avg_max_borrow_ltv: Decimal,
    avg_max_ltv: Decimal,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if Some(info.sender.clone()) != config.collateral_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, mut position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;

    // Step 1: Accrue interest locally
    let mut basket = BASKET.load(deps.storage)?;
    accrue_position(
        deps.storage,
        deps.querier,
        env.clone(),
        &config,
        &mut position,
        &mut basket,
        owner.to_string(),
    )?;
    BASKET.save(deps.storage, &basket)?;

    // Step 2: Compute total debt value
    let total_debt = get_total_position_debt(&position);
    let credit_price = basket.credit_price.price;
    let debt_value = Decimal::from_ratio(total_debt, 1u128) * credit_price;

    // Step 3: Run insolvency check
    // If position has debt, check remaining collateral value vs debt
    if !total_debt.is_zero() {
        let current_ltv = membrane::risk::compute_ltv(remaining_collateral_value, debt_value);
        if !membrane::risk::check_solvency(current_ltv, avg_max_ltv) {
            return Err(ContractError::Insolvent {});
        }
        // Also check borrow LTV for withdrawals
        if !membrane::risk::can_borrow(current_ltv, avg_max_borrow_ltv) {
            return Err(ContractError::MaxBorrowLTVExceeded {});
        }
    }

    // Save updated position after accrual
    crate::state::update_position(deps.storage, owner, position)?;

    Ok(Response::new()
        .add_attribute("method", "accrue_and_validate")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("total_debt", total_debt.to_string())
        .add_attribute("debt_value", debt_value.to_string())
        .add_attribute("credit_price", credit_price.to_string()))
}

// ============================================================================
// Authorized calls from Liquidation contract
// ============================================================================

/// Accrue and return debt data for liquidation assessment.
/// Only callable by Liquidation contract.
///
/// CDP parity: this is new in the modular design - Liquidation needs debt data
pub fn accrue_and_return_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_owner: String,
    position_id: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if Some(info.sender.clone()) != config.liquidation_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, mut position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;

    // Step 1: Accrue interest locally
    let mut basket = BASKET.load(deps.storage)?;
    accrue_position(
        deps.storage,
        deps.querier,
        env.clone(),
        &config,
        &mut position,
        &mut basket,
        owner.to_string(),
    )?;
    BASKET.save(deps.storage, &basket)?;

    let total_debt = get_total_position_debt(&position);

    // ================================================================
    // Venue recall (CDP parity: get_deployable_venues_user_repay_amount)
    //
    // Query each deployed venue's RetrievableCDT. Build RepayUserDebt
    // SubMsgs to recall CDT during liquidation. Track total recall amount
    // so the liq engine can reduce the collateral it needs to sell.
    // ================================================================
    let mut venue_recall_amount = Uint128::zero();
    let mut venue_submsgs: Vec<cosmwasm_std::SubMsg> = vec![];
    let mut venue_recall_addresses: Vec<String> = vec![];
    let mut credit_repay_remaining = cosmwasm_std::Decimal::from_ratio(total_debt, 1u128);
    let deployed_venues = position.deployed_to.clone();

    for venue in &deployed_venues {
        if venue.failed_liquidation || credit_repay_remaining.is_zero() {
            continue;
        }

        // Query venue's retrievable CDT for this user
        let retrievable: Uint128 = deps.querier
            .query_wasm_smart(
                venue.address.to_string(),
                &membrane::deployable_venue::QueryMsg::RetrievableCDT {
                    user: position_owner.clone(),
                },
            )
            .unwrap_or(Uint128::zero());

        if retrievable.is_zero() {
            continue;
        }

        let retrievable_dec = cosmwasm_std::Decimal::from_ratio(retrievable, 1u128);

        // CDP line 643: cap at remaining credit to repay
        let recall_amount = if retrievable_dec > credit_repay_remaining {
            credit_repay_remaining.to_uint_floor()
        } else {
            retrievable
        };

        venue_recall_amount += recall_amount;

        // Build RepayUserDebt SubMsg (reply_always so failures don't revert liquidation)
        let repay_msg = cosmwasm_std::WasmMsg::Execute {
            contract_addr: venue.address.to_string(),
            msg: to_json_binary(&membrane::deployable_venue::ExecuteMsg::RepayUserDebt {
                user_info: membrane::types::UserInfo {
                    position_id,
                    position_owner: position_owner.clone(),
                },
                repayment: recall_amount,
            })?,
            funds: vec![],
        };
        venue_submsgs.push(cosmwasm_std::SubMsg::reply_always(
            repay_msg,
            crate::state::VENUE_RECALL_REPLY_ID,
        ));
        venue_recall_addresses.push(venue.address.to_string());

        // Update deployed_debt_amount on position
        if let Some(pos_venue) = position.deployed_to.iter_mut()
            .find(|v| v.address == venue.address)
        {
            pos_venue.deployed_debt_amount = pos_venue.deployed_debt_amount
                .saturating_sub(recall_amount);
        }

        // Reduce remaining credit to recall
        credit_repay_remaining = credit_repay_remaining
            .checked_sub(cosmwasm_std::Decimal::from_ratio(recall_amount, 1u128))
            .unwrap_or(cosmwasm_std::Decimal::zero());
    }

    // Save venue recall propagation so the reply handler knows which venue failed
    if !venue_recall_addresses.is_empty() {
        crate::state::VENUE_RECALL_PROPAGATION.save(
            deps.storage,
            &crate::state::VenueRecallPropagation {
                position_owner: position_owner.clone(),
                position_id,
                venues: venue_recall_addresses,
            },
        )?;
    }

    // Remove venues with zero deployed debt
    position.deployed_to.retain(|v| !v.deployed_debt_amount.is_zero());

    // Save updated position after accrual + venue recall adjustments
    crate::state::update_position(deps.storage, owner, position.clone())?;

    // Serialize debt data for reply parsing
    let debt_response = AccrueDebtResponse {
        total_debt,
        credit_price: basket.credit_price.clone(),
        rate_segments: position.rate_segments.clone(),
        peg_rate_segments: position.peg_rate_segments.clone(),
        venue_recall_amount,
    };
    let debt_json = to_json_binary(&debt_response)?;

    Ok(Response::new()
        .add_submessages(venue_submsgs)
        .add_attribute("method", "accrue_and_return_debt")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("total_debt", total_debt.to_string())
        .add_attribute("venue_recall_amount", venue_recall_amount.to_string())
        .add_attribute("debt_response", debt_json.to_string()))
}

/// Update debt after liquidation repayment.
/// Only callable by Liquidation contract.
///
/// CDP parity: maps to the debt reduction part of cdp/liquidations.rs
pub fn liquidation_debt_update(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
    repay_amount: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if Some(info.sender.clone()) != config.liquidation_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, mut position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;

    let mut basket = BASKET.load(deps.storage)?;
    let mut rates = RATES.load(deps.storage)?;

    // Reduce debt segments proportionally by repay_amount
    let total_debt = get_total_position_debt(&position);
    let regular_total = crate::debt_manager::get_total_from_segments(&position.rate_segments);
    let peg_total = crate::debt_manager::get_total_from_segments(&position.peg_rate_segments);

    let regular_deltas = reduce_segments_proportionally(
        &mut position.rate_segments,
        regular_total,
        total_debt,
        repay_amount,
    );
    let peg_deltas = reduce_segments_proportionally(
        &mut position.peg_rate_segments,
        peg_total,
        total_debt,
        repay_amount,
    );

    // Apply deltas to basket and rates
    apply_delta_to_regular_debt(&mut basket.credit_asset, &regular_deltas);
    apply_delta_to_peg_debt(&mut basket.credit_asset, &peg_deltas);
    apply_delta_to_caps(&mut rates.fixed_rate_caps, &regular_deltas);
    apply_delta_to_caps(&mut rates.fixed_rate_caps, &peg_deltas);

    // Check if debt reaches zero -> deactivate supply caps
    let total_debt_after = get_total_position_debt(&position);
    let mut msgs: Vec<SubMsg> = vec![];

    if total_debt_after.is_zero() {
        if let Some(ref collateral_contract) = config.collateral_contract {
            let deactivate_msg = WasmMsg::Execute {
                contract_addr: collateral_contract.to_string(),
                msg: to_json_binary(&CollateralExecuteMsg::DeactivateSupplyCaps {
                    position_id,
                    position_owner: owner.to_string(),
                })?,
                funds: vec![],
            };
            msgs.push(SubMsg::new(deactivate_msg));
        }
    }

    // Burn CDT via chain proxy
    let credit_denom = match &basket.credit_asset.info {
        AssetInfo::NativeToken { denom } => denom.clone(),
        AssetInfo::Token { address } => address.to_string(),
    };
    if let Some(ref chain_proxy) = config.chain_proxy {
        let burn_msg = WasmMsg::Execute {
            contract_addr: chain_proxy.to_string(),
            msg: to_json_binary(&ChainProxyExecuteMsg::BurnTokens {
                denom: credit_denom,
                amount: repay_amount,
                burn_from_address: env.contract.address.to_string(),
            })?,
            funds: vec![],
        };
        msgs.push(SubMsg::new(burn_msg));
    }

    // Track CDT supply decrease
    let mut cdt_supply = CDT_SUPPLY.load(deps.storage)?;
    cdt_supply = cdt_supply
        .checked_sub(repay_amount)
        .unwrap_or(Uint128::zero());
    CDT_SUPPLY.save(deps.storage, &cdt_supply)?;

    // Save state
    RATES.save(deps.storage, &rates)?;
    BASKET.save(deps.storage, &basket)?;
    crate::state::update_position(deps.storage, owner, position)?;

    Ok(Response::new()
        .add_submessages(msgs)
        .add_attribute("method", "liquidation_debt_update")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("repay_amount", repay_amount.to_string())
        .add_attribute("debt_after", total_debt_after.to_string()))
}

/// Record bad debt after liquidation.
/// Only callable by Liquidation contract.
///
/// CDP parity: maps to cdp/liquidations.rs bad debt handling
pub fn record_bad_debt(
    deps: DepsMut,
    info: MessageInfo,
    position_id: Uint128,
    _position_owner: String,
    bad_debt_amount: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if Some(info.sender.clone()) != config.liquidation_contract {
        return Err(ContractError::Unauthorized {});
    }

    let mut basket = BASKET.load(deps.storage)?;
    basket.pending_bad_debt += bad_debt_amount;
    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new()
        .add_attribute("method", "record_bad_debt")
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("bad_debt_amount", bad_debt_amount.to_string()))
}

/// Close position callback after venue repayments complete.
///
/// CDP parity: maps to cdp/reply.rs::handle_close_position_reply()
pub fn close_position_callback(
    deps: DepsMut,
    _env: Env,
    position_id: Uint128,
    _position_owner: String,
) -> Result<Response, ContractError> {
    // Clean up propagation state
    CLOSE_POSITION_PROPAGATION.remove(deps.storage);

    Ok(Response::new()
        .add_attribute("method", "close_position_callback")
        .add_attribute("position_id", position_id.to_string()))
}
