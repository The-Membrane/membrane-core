#![allow(non_snake_case)]
use cosmwasm_std::{
    attr, to_json_binary, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response,
    SubMsg, Uint128, WasmMsg,
};

use membrane::collateral::CollateralPosition;
use membrane::debt::ExecuteMsg as DebtExecuteMsg;
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::types::{Asset, AssetInfo, cAsset, UserInfo};

use crate::ContractError;
use crate::state::{
    get_target_position, update_position, SellCollateralPropagation, WithdrawPropagation,
    BASKET, CONFIG, FROZEN, MAX_POSITIONS_PER_USER, POSITIONS,
    SELL_COLLATERAL_PROPAGATION, SELL_COLLATERAL_REPLY_ID, WITHDRAW_PROPAGATION,
    WITHDRAW_REPLY_ID,
};

// ============================================================================
// Deposit
// ============================================================================

/// Deposit collateral into a position.
/// If position_id is None, creates a new position.
///
/// CDP parity: maps to cdp/positions.rs::deposit()
pub fn deposit(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Option<Uint128>,
    position_owner: Option<String>,
    affiliate_address: Option<String>,
) -> Result<Response, ContractError> {
    // Check frozen
    if FROZEN.load(deps.storage)? {
        return Err(ContractError::Frozen {});
    }

    let basket = BASKET.load(deps.storage)?;
    let config = CONFIG.load(deps.storage)?;

    let owner = if let Some(owner_str) = position_owner.clone() {
        deps.api.addr_validate(&owner_str)?
    } else {
        info.sender.clone()
    };

    // Map info.funds to cAssets using basket.collateral_types
    let mut deposited_assets: Vec<cAsset> = vec![];
    for coin in info.funds.iter() {
        if let Some(ct) = basket.collateral_types.iter().find(|ct| {
            ct.asset.info.to_string() == coin.denom
        }) {
            let c_asset = cAsset {
                asset: Asset {
                    info: ct.asset.info.clone(),
                    amount: coin.amount,
                },
                ..ct.clone()
            };
            deposited_assets.push(c_asset);
        }
    }

    // Validate deposited assets against basket collateral types
    crate::risk_engine::assert_basket_assets(&deps, &deposited_assets)?;

    // If no valid assets were deposited, error
    if deposited_assets.is_empty() {
        return Err(ContractError::InvalidCollateral {});
    }

    let mut msgs: Vec<SubMsg> = vec![];

    match position_id {
        Some(id) => {
            // Add to existing position
            let (_index, mut position) =
                get_target_position(deps.storage, owner.clone(), id)?;

            // Add collateral assets from deposited funds to position
            for deposit in deposited_assets.iter() {
                if let Some(collateral_index) = position
                    .collateral_assets
                    .iter()
                    .position(|ca| ca.asset.info.equal(&deposit.asset.info))
                {
                    // Found existing cAsset -- increment amount
                    position.collateral_assets[collateral_index].asset.amount +=
                        deposit.asset.amount;
                } else {
                    // Add new cAsset object to position
                    position.collateral_assets.push(deposit.clone());
                }
            }

            // Refresh oracle prices locally to update STORED_PRICES + VOLATILITY
            if let Some(oracle_addr) = config.oracle_contract.clone() {
                let asset_infos: Vec<AssetInfo> = position
                    .collateral_assets
                    .iter()
                    .map(|ca| ca.asset.info.clone())
                    .collect();

                // Best-effort price refresh; non-fatal if oracle is unavailable
                let _price_result: Result<Vec<membrane::oracle::PriceResponse>, _> =
                    deps.querier.query(&cosmwasm_std::QueryRequest::Wasm(
                        cosmwasm_std::WasmQuery::Smart {
                            contract_addr: oracle_addr.to_string(),
                            msg: to_json_binary(&membrane::oracle::QueryMsg::Prices {
                                asset_infos,
                                twap_timeframe: config.collateral_twap_timeframe,
                                oracle_time_limit: config.oracle_time_limit,
                            })?,
                        },
                    ));
            }

            let position_collateral = position.collateral_assets.clone();

            // Save updated position
            update_position(deps.storage, owner.clone(), position)?;

            // Update supply caps via risk_engine (best-effort; supply caps only matter
            // when position has debt, which the Debt contract manages via ActivateSupplyCaps)
            let _ = crate::risk_engine::update_basket_tally(
                &mut deps,
                env.clone(),
                deposited_assets.clone(),
                position_collateral,
                true,  // add = true
                false, // from_liquidation = false
            );

            // Fire-and-forget SubMsg to Debt.NotifyDeposit
            if let Some(debt_contract) = config.debt_contract.clone() {
                let notify_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: debt_contract.to_string(),
                    msg: to_json_binary(&DebtExecuteMsg::NotifyDeposit {
                        position_id: id,
                        position_owner: owner.to_string(),
                        affiliate_address: affiliate_address.clone(),
                    })?,
                    funds: vec![],
                });
                msgs.push(SubMsg::new(notify_msg));
            }

            Ok(Response::new()
                .add_submessages(msgs)
                .add_attributes(vec![
                    attr("method", "deposit"),
                    attr("position_owner", owner.to_string()),
                    attr("position_id", id.to_string()),
                    attr(
                        "assets",
                        format!(
                            "{:?}",
                            deposited_assets
                                .iter()
                                .map(|a| a.asset.clone())
                                .collect::<Vec<Asset>>()
                        ),
                    ),
                ]))
        }
        None => {
            // Create new position
            let mut positions = POSITIONS
                .may_load(deps.storage, owner.clone())?
                .unwrap_or_default();
            if positions.len() >= MAX_POSITIONS_PER_USER {
                return Err(ContractError::MaxPositionsReached {});
            }

            // Allocate next position_id from BasketCollateral counter
            let mut basket = BASKET.load(deps.storage)?;
            basket.current_position_id += Uint128::one();
            let new_id = basket.current_position_id;
            BASKET.save(deps.storage, &basket)?;

            // Populate collateral_assets from deposited funds
            let new_position = CollateralPosition {
                position_id: new_id,
                collateral_assets: deposited_assets.clone(),
            };

            let position_info = UserInfo {
                position_id: new_id,
                position_owner: owner.to_string(),
            };

            positions.push(new_position);
            POSITIONS.save(deps.storage, owner.clone(), &positions)?;

            // Fire-and-forget SubMsg to Debt.NotifyDeposit
            if let Some(debt_contract) = config.debt_contract.clone() {
                let notify_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: debt_contract.to_string(),
                    msg: to_json_binary(&DebtExecuteMsg::NotifyDeposit {
                        position_id: new_id,
                        position_owner: owner.to_string(),
                        affiliate_address: affiliate_address.clone(),
                    })?,
                    funds: vec![],
                });
                msgs.push(SubMsg::new(notify_msg));
            }

            Ok(Response::new()
                .add_submessages(msgs)
                .add_attributes(vec![
                    attr("method", "deposit"),
                    attr("position_owner", position_info.position_owner),
                    attr("position_id", position_info.position_id.to_string()),
                    attr("action", "create_position"),
                    attr(
                        "assets",
                        format!(
                            "{:?}",
                            deposited_assets
                                .iter()
                                .map(|a| a.asset.clone())
                                .collect::<Vec<Asset>>()
                        ),
                    ),
                ]))
        }
    }
}

// ============================================================================
// Withdraw
// ============================================================================

/// Withdraw collateral from a position.
/// Calls Debt.AccrueAndValidate via SubMsg, then processes on reply.
///
/// CDP parity: maps to cdp/positions.rs::withdraw()
pub fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    assets: Vec<Asset>,
    send_to: Option<String>,
) -> Result<Response, ContractError> {
    // Check frozen
    if FROZEN.load(deps.storage)? {
        return Err(ContractError::Frozen {});
    }

    let config = CONFIG.load(deps.storage)?;
    let basket = BASKET.load(deps.storage)?;

    // Withdrawals must be done by the sender (position owner)
    let owner = info.sender.clone();

    // Set recipient
    let recipient = if let Some(ref addr_str) = send_to {
        deps.api.addr_validate(addr_str)?
    } else {
        info.sender.clone()
    };

    // Load position
    let (_index, position) =
        get_target_position(deps.storage, owner.clone(), position_id)?;

    // Map withdrawal Asset list to cAsset for circuit breaker checks
    let withdraw_cassets: Vec<cAsset> = assets
        .iter()
        .filter_map(|a| {
            basket
                .collateral_types
                .iter()
                .find(|ct| ct.asset.info.equal(&a.info))
                .map(|ct| cAsset {
                    asset: a.clone(),
                    ..ct.clone()
                })
        })
        .collect();

    // Circuit breaker: block withdrawals when any asset is frozen due to price deviation
    crate::circuit_breaker::check_assets_not_frozen(
        deps.storage,
        deps.querier,
        &env,
        &config,
        &withdraw_cassets,
    )?;

    // Validate withdrawal amounts against position balances
    for withdraw_asset in assets.iter() {
        let position_collateral = position
            .collateral_assets
            .iter()
            .find(|ca| ca.asset.info.equal(&withdraw_asset.info));

        match position_collateral {
            Some(ca) => {
                if withdraw_asset.amount > ca.asset.amount {
                    return Err(ContractError::ExcessiveWithdrawal {});
                }
            }
            None => {
                return Err(ContractError::InvalidCollateral {});
            }
        }
    }

    // Compute remaining collateral after proposed withdrawal for solvency check
    let mut remaining_collateral = position.collateral_assets.clone();
    for withdraw_asset in assets.iter() {
        if let Some(ca) = remaining_collateral
            .iter_mut()
            .find(|ca| ca.asset.info.equal(&withdraw_asset.info))
        {
            ca.asset.amount = ca
                .asset
                .amount
                .checked_sub(withdraw_asset.amount)
                .map_err(|_| ContractError::ExcessiveWithdrawal {})?;
        }
    }
    // Remove zero-amount entries
    remaining_collateral.retain(|ca| !ca.asset.amount.is_zero());

    // Refresh oracle prices locally and compute collateral values + LTVs
    let (remaining_collateral_value, avg_max_borrow_ltv, avg_max_ltv) =
        compute_remaining_collateral_metrics(
            deps.querier,
            &config,
            &remaining_collateral,
            &basket.collateral_types,
        )?;

    // Save withdrawal propagation state for reply processing
    let prop_assets: Vec<Asset> = assets
        .iter()
        .map(|a| {
            // Store the original amount in position before withdrawal
            let prev_amount = position
                .collateral_assets
                .iter()
                .find(|ca| ca.asset.info.equal(&a.info))
                .map(|ca| ca.asset.amount)
                .unwrap_or(Uint128::zero());
            Asset {
                info: a.info.clone(),
                amount: prev_amount,
            }
        })
        .collect();
    let withdraw_amounts: Vec<Uint128> = assets.iter().map(|a| a.amount).collect();

    // Get contract balances before withdrawal for state verification
    let contracts_prev_collateral_amount: Vec<Uint128> = assets
        .iter()
        .map(|a| match &a.info {
            AssetInfo::NativeToken { denom } => deps
                .querier
                .query_balance(env.contract.address.to_string(), denom)
                .map(|coin| coin.amount)
                .unwrap_or(Uint128::zero()),
            AssetInfo::Token { .. } => Uint128::zero(),
        })
        .collect();

    let withdrawal_prop = WithdrawPropagation {
        positions_prev_collateral: prop_assets,
        withdraw_amounts,
        contracts_prev_collateral_amount,
        position_info: UserInfo {
            position_id,
            position_owner: owner.to_string(),
        },
    };
    WITHDRAW_PROPAGATION.save(deps.storage, &withdrawal_prop)?;

    // Build SubMsg to Debt.AccrueAndValidate
    let debt_contract = config.debt_contract.ok_or(ContractError::CustomError {
        val: "No debt contract configured".to_string(),
    })?;

    let accrue_validate_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: debt_contract.to_string(),
        msg: to_json_binary(&DebtExecuteMsg::AccrueAndValidate {
            position_owner: owner.to_string(),
            position_id,
            remaining_collateral_value,
            avg_max_borrow_ltv,
            avg_max_ltv,
        })?,
        funds: vec![],
    });

    // Use WITHDRAW_REPLY_ID so on success we process the withdrawal in reply.rs
    let sub_msg = SubMsg::reply_on_success(accrue_validate_msg, WITHDRAW_REPLY_ID);

    Ok(Response::new()
        .add_submessage(sub_msg)
        .add_attributes(vec![
            attr("method", "withdraw"),
            attr("position_id", position_id.to_string()),
            attr("send_to", recipient.to_string()),
            attr("assets", format!("{:?}", assets)),
        ]))
}

/// Compute remaining collateral value and weighted average LTVs after a proposed withdrawal.
///
/// Returns (remaining_collateral_value, avg_max_borrow_ltv, avg_max_ltv).
fn compute_remaining_collateral_metrics(
    querier: cosmwasm_std::QuerierWrapper,
    config: &membrane::collateral::Config,
    remaining_collateral: &[cAsset],
    collateral_types: &[cAsset],
) -> Result<(Decimal, Decimal, Decimal), ContractError> {
    if remaining_collateral.is_empty() {
        return Ok((Decimal::zero(), Decimal::zero(), Decimal::zero()));
    }

    let oracle_addr = config
        .oracle_contract
        .clone()
        .ok_or(ContractError::CustomError {
            val: "No oracle contract configured".to_string(),
        })?;

    // Query oracle for fresh prices
    let asset_infos: Vec<AssetInfo> = remaining_collateral
        .iter()
        .map(|ca| ca.asset.info.clone())
        .collect();

    let price_resps: Vec<membrane::oracle::PriceResponse> = querier
        .query(&cosmwasm_std::QueryRequest::Wasm(
            cosmwasm_std::WasmQuery::Smart {
                contract_addr: oracle_addr.to_string(),
                msg: to_json_binary(&membrane::oracle::QueryMsg::Prices {
                    asset_infos,
                    twap_timeframe: config.collateral_twap_timeframe,
                    oracle_time_limit: config.oracle_time_limit,
                })?,
            },
        ))
        .map_err(cosmwasm_std::StdError::from)?;

    // Compute total value
    let mut total_value = Decimal::zero();
    let mut values: Vec<Decimal> = Vec::new();

    for (i, ca) in remaining_collateral.iter().enumerate() {
        let price = price_resps
            .get(i)
            .ok_or(ContractError::CustomError {
                val: "Price response length mismatch".to_string(),
            })?;

        let amount_dec = Decimal::from_ratio(ca.asset.amount, 1u128);
        let value = amount_dec * price.price;
        values.push(value);
        total_value += value;
    }

    // Compute ratios
    let ratios: Vec<Decimal> = if total_value.is_zero() {
        values.iter().map(|_| Decimal::zero()).collect()
    } else {
        values.iter().map(|v| *v / total_value).collect()
    };

    // Compute weighted average LTVs from basket's authoritative collateral_types
    let mut avg_max_ltv = Decimal::zero();
    let mut avg_max_borrow_ltv = Decimal::zero();

    for (i, ca) in remaining_collateral.iter().enumerate() {
        let ratio = ratios.get(i).copied().unwrap_or(Decimal::zero());

        if let Some(ct) = collateral_types
            .iter()
            .find(|ct| ct.asset.info.equal(&ca.asset.info))
        {
            avg_max_ltv += ratio * ct.max_LTV;
            avg_max_borrow_ltv += ratio * ct.max_borrow_LTV;
        }
    }

    Ok((total_value, avg_max_borrow_ltv, avg_max_ltv))
}

// ============================================================================
// Authorized Withdraw (called by Debt contract after solvency validation)
// ============================================================================

/// Execute a withdrawal after Debt validates solvency.
/// Only callable by Debt contract.
///
/// CDP parity: maps to the withdrawal execution part of cdp/positions.rs::withdraw()
pub fn authorized_withdraw(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
    assets: Vec<Asset>,
    send_to: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only Debt contract can call this
    if Some(info.sender.clone()) != config.debt_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, mut position) =
        get_target_position(deps.storage, owner.clone(), position_id)?;

    // Remove withdrawn assets from position's collateral_assets
    for withdraw_asset in assets.iter() {
        if let Some(collateral_index) = position
            .collateral_assets
            .iter()
            .position(|ca| ca.asset.info.equal(&withdraw_asset.info))
        {
            let current_amount = position.collateral_assets[collateral_index].asset.amount;
            if withdraw_asset.amount >= current_amount {
                // Fully withdraw -- remove the cAsset entry
                position.collateral_assets.remove(collateral_index);
            } else {
                // Partial withdraw -- reduce amount
                position.collateral_assets[collateral_index].asset.amount =
                    current_amount - withdraw_asset.amount;
            }
        }
    }

    // Build cAsset list for supply cap update
    let withdraw_cassets: Vec<cAsset> = {
        let basket = BASKET.load(deps.storage)?;
        assets
            .iter()
            .filter_map(|a| {
                basket
                    .collateral_types
                    .iter()
                    .find(|ct| ct.asset.info.equal(&a.info))
                    .map(|ct| cAsset {
                        asset: a.clone(),
                        ..ct.clone()
                    })
            })
            .collect()
    };

    let position_collateral = position.collateral_assets.clone();

    // Save updated position
    update_position(deps.storage, owner, position)?;

    // Update supply caps via risk_engine (remove withdrawn amounts from tally)
    let _ = crate::risk_engine::update_basket_tally(
        &mut deps,
        env.clone(),
        withdraw_cassets,
        position_collateral,
        false, // add = false (removing)
        false, // from_liquidation = false
    );

    // Create BankMsg::Send to send assets to send_to address
    let mut bank_msgs: Vec<CosmosMsg> = vec![];
    for asset in assets.iter() {
        if !asset.amount.is_zero() {
            bank_msgs.push(CosmosMsg::Bank(BankMsg::Send {
                to_address: send_to.clone(),
                amount: vec![Coin {
                    denom: asset.info.to_string(),
                    amount: asset.amount,
                }],
            }));
        }
    }

    Ok(Response::new()
        .add_messages(bank_msgs)
        .add_attributes(vec![
            attr("method", "authorized_withdraw"),
            attr("position_id", position_id.to_string()),
            attr("send_to", send_to),
            attr("assets", format!("{:?}", assets)),
        ]))
}

// ============================================================================
// Authorized Sell Collateral
// ============================================================================

/// Sell collateral for ClosePosition or liquidation.
/// Only callable by Debt or Liquidation contract.
///
/// CDP parity: maps to the sell collateral part of cdp/positions.rs::close_position()
/// and liquidations.rs
pub fn authorized_sell_collateral(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
    max_spread: cosmwasm_std::Decimal,
    send_proceeds_to: String,
    sell_amounts: Option<Vec<Asset>>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only Debt or Liquidation contract can call this
    if Some(info.sender.clone()) != config.debt_contract
        && Some(info.sender.clone()) != config.liquidation_contract
    {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, position) =
        get_target_position(deps.storage, owner.clone(), position_id)?;

    // Require chain proxy to be configured for swap execution
    let chain_proxy = config.chain_proxy.ok_or(ContractError::CustomError {
        val: "No chain proxy configured for swaps".to_string(),
    })?;

    let mut router_messages: Vec<CosmosMsg> = vec![];
    let mut sold_coins: Vec<Coin> = vec![];

    // If sell_amounts provided, sell only those specific amounts (CDP parity: leftover_repayment worth).
    // Otherwise sell all remaining position collateral (original behavior).
    if let Some(ref amounts) = sell_amounts {
        for sell_asset in amounts {
            if sell_asset.amount.is_zero() {
                continue;
            }
            // Cap at what position actually holds for this asset
            let position_amount = position
                .collateral_assets
                .iter()
                .find(|ca| ca.asset.info.equal(&sell_asset.info))
                .map(|ca| ca.asset.amount)
                .unwrap_or(Uint128::zero());
            let capped_amount = std::cmp::min(sell_asset.amount, position_amount);
            if capped_amount.is_zero() {
                continue;
            }

            let coin = Coin {
                denom: sell_asset.info.to_string(),
                amount: capped_amount,
            };
            sold_coins.push(coin.clone());

            let router_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: chain_proxy.to_string(),
                msg: to_json_binary(&OsmoExecuteMsg::ExecuteSwaps {
                    token_out: send_proceeds_to.clone(),
                    max_slippage: max_spread,
                })?,
                funds: vec![coin],
            });
            router_messages.push(router_msg);
        }
    } else {
        // Original behavior: sell all remaining position collateral
        for ca in position.collateral_assets.iter() {
            if ca.asset.amount.is_zero() {
                continue;
            }

            let coin = Coin {
                denom: ca.asset.info.to_string(),
                amount: ca.asset.amount,
            };
            sold_coins.push(coin.clone());

            let router_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: chain_proxy.to_string(),
                msg: to_json_binary(&OsmoExecuteMsg::ExecuteSwaps {
                    token_out: send_proceeds_to.clone(),
                    max_slippage: max_spread,
                })?,
                funds: vec![coin],
            });
            router_messages.push(router_msg);
        }
    }

    // Save sell collateral propagation for reply processing
    let propagation = SellCollateralPropagation {
        collateral_sold: sold_coins.clone(),
        position_info: UserInfo {
            position_id,
            position_owner: owner.to_string(),
        },
    };
    SELL_COLLATERAL_PROPAGATION.save(deps.storage, &propagation)?;

    // Build SubMsgs: all but last are fire-and-forget, last uses reply
    // to finalize position update after all swaps complete
    let mut sub_msgs: Vec<SubMsg> = vec![];
    if !router_messages.is_empty() {
        let last_msg = router_messages.pop().unwrap();
        // Fire-and-forget for all but the last
        for msg in router_messages {
            sub_msgs.push(SubMsg::new(msg));
        }
        // Last one triggers the reply to finalize position update
        sub_msgs.push(SubMsg::reply_on_success(last_msg, SELL_COLLATERAL_REPLY_ID));
    }

    Ok(Response::new()
        .add_submessages(sub_msgs)
        .add_attributes(vec![
            attr("method", "authorized_sell_collateral"),
            attr("position_id", position_id.to_string()),
            attr("send_proceeds_to", send_proceeds_to),
            attr("collateral_sold", format!("{:?}", sold_coins)),
        ]))
}

// ============================================================================
// Liquidation Claim Update
// ============================================================================

/// Update collateral claims after liquidation.
/// Only callable by Liquidation contract.
///
/// CDP parity: maps to cdp/state.rs::update_position_claims()
pub fn liquidation_claim_update(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
    collateral_reductions: Vec<Asset>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only Liquidation contract can call this
    if Some(info.sender.clone()) != config.liquidation_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, mut position) =
        get_target_position(deps.storage, owner.clone(), position_id)?;

    // Subtract collateral_reductions from position's collateral_assets
    for reduction in collateral_reductions.iter() {
        if let Some(collateral_index) = position
            .collateral_assets
            .iter()
            .position(|ca| ca.asset.info.equal(&reduction.info))
        {
            let current_amount = position.collateral_assets[collateral_index].asset.amount;
            if reduction.amount >= current_amount {
                // Fully removed -- delete the cAsset entry
                position.collateral_assets.remove(collateral_index);
            } else {
                // Partial reduction
                position.collateral_assets[collateral_index].asset.amount =
                    current_amount - reduction.amount;
            }
        }
    }

    // Build cAsset list for supply cap update
    let basket = BASKET.load(deps.storage)?;
    let reduction_cassets: Vec<cAsset> = collateral_reductions
        .iter()
        .filter_map(|a| {
            basket
                .collateral_types
                .iter()
                .find(|ct| ct.asset.info.equal(&a.info))
                .map(|ct| cAsset {
                    asset: a.clone(),
                    ..ct.clone()
                })
        })
        .collect();

    let position_collateral = position.collateral_assets.clone();

    // Save updated position
    update_position(deps.storage, owner, position)?;

    // Update supply caps (remove liquidated amounts from tally)
    let _ = crate::risk_engine::update_basket_tally(
        &mut deps,
        env,
        reduction_cassets,
        position_collateral,
        false, // add = false (removing)
        true,  // from_liquidation = true
    );

    Ok(Response::new().add_attributes(vec![
        attr("method", "liquidation_claim_update"),
        attr("position_id", position_id.to_string()),
        attr(
            "collateral_reductions",
            format!("{:?}", collateral_reductions),
        ),
    ]))
}

// ============================================================================
// Authorized Transfer (for liquidation queue)
// ============================================================================

/// Transfer collateral to liq queue during liquidation.
/// Only callable by Liquidation contract.
pub fn authorized_transfer(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
    assets: Vec<Asset>,
    recipient: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only Liquidation contract can call this
    if Some(info.sender.clone()) != config.liquidation_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, mut position) =
        get_target_position(deps.storage, owner.clone(), position_id)?;

    // Create BankMsg::Send for each asset to recipient and update position collateral
    let mut bank_msgs: Vec<CosmosMsg> = vec![];
    for asset in assets.iter() {
        if asset.amount.is_zero() {
            continue;
        }

        // Send asset to recipient
        bank_msgs.push(CosmosMsg::Bank(BankMsg::Send {
            to_address: recipient.clone(),
            amount: vec![Coin {
                denom: asset.info.to_string(),
                amount: asset.amount,
            }],
        }));

        // Update position collateral: subtract transferred amount
        if let Some(collateral_index) = position
            .collateral_assets
            .iter()
            .position(|ca| ca.asset.info.equal(&asset.info))
        {
            let current_amount = position.collateral_assets[collateral_index].asset.amount;
            if asset.amount >= current_amount {
                // Fully transferred -- remove entry
                position.collateral_assets.remove(collateral_index);
            } else {
                // Partial transfer
                position.collateral_assets[collateral_index].asset.amount =
                    current_amount - asset.amount;
            }
        }
    }

    // Save updated position
    update_position(deps.storage, owner, position)?;

    Ok(Response::new()
        .add_messages(bank_msgs)
        .add_attributes(vec![
            attr("method", "authorized_transfer"),
            attr("position_id", position_id.to_string()),
            attr("recipient", recipient),
            attr("assets", format!("{:?}", assets)),
        ]))
}

// ============================================================================
// Withdraw Callback (post-reply finalization)
// ============================================================================

/// Callback after withdrawal SubMsg completes.
/// Processes the actual removal of collateral and supply cap updates.
pub fn withdraw_callback(
    mut deps: DepsMut,
    env: Env,
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError> {
    let owner = deps.api.addr_validate(&position_owner)?;

    // Load propagation state saved during withdraw()
    let propagation = WITHDRAW_PROPAGATION.load(deps.storage)?;

    // Load position
    let (_index, mut position) =
        get_target_position(deps.storage, owner.clone(), position_id)?;

    // Remove withdrawn amounts from position collateral
    for (i, prev_asset) in propagation.positions_prev_collateral.iter().enumerate() {
        let withdraw_amount = propagation
            .withdraw_amounts
            .get(i)
            .copied()
            .unwrap_or(Uint128::zero());

        if let Some(collateral_index) = position
            .collateral_assets
            .iter()
            .position(|ca| ca.asset.info.equal(&prev_asset.info))
        {
            let current_amount = position.collateral_assets[collateral_index].asset.amount;
            if withdraw_amount >= current_amount {
                position.collateral_assets.remove(collateral_index);
            } else {
                position.collateral_assets[collateral_index].asset.amount =
                    current_amount - withdraw_amount;
            }
        }
    }

    // Build cAsset list for supply cap update
    let basket = BASKET.load(deps.storage)?;
    let withdrawn_cassets: Vec<cAsset> = propagation
        .positions_prev_collateral
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let amount = propagation
                .withdraw_amounts
                .get(i)
                .copied()
                .unwrap_or(Uint128::zero());
            if amount.is_zero() {
                return None;
            }
            basket
                .collateral_types
                .iter()
                .find(|ct| ct.asset.info.equal(&a.info))
                .map(|ct| cAsset {
                    asset: Asset {
                        info: a.info.clone(),
                        amount,
                    },
                    ..ct.clone()
                })
        })
        .collect();

    let position_collateral = position.collateral_assets.clone();

    // Save updated position
    update_position(deps.storage, owner, position)?;

    // Update supply caps (remove withdrawn amounts)
    let _ = crate::risk_engine::update_basket_tally(
        &mut deps,
        env,
        withdrawn_cassets,
        position_collateral,
        false, // add = false (removing)
        false, // from_liquidation = false
    );

    // Clear propagation state
    WITHDRAW_PROPAGATION.remove(deps.storage);

    Ok(Response::new().add_attributes(vec![
        attr("method", "withdraw_callback"),
        attr("position_id", position_id.to_string()),
    ]))
}
