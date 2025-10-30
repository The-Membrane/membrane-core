use cosmwasm_std::{
    attr, to_json_binary, Addr, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, QueryRequest, Response, Storage, SubMsg, Uint128, WasmMsg, WasmQuery, QuerierWrapper
};
use std::str::FromStr;
use membrane::cdp::{LiquidationStatResponse, QueryMsg as CDP_QueryMsg};
use membrane::ltv_disco::{
    BackingDeposit, BackingDepositInput, RevenueTrackingEntry, RevenueEvent, UserLifetimeRevenueEntry, Config, DecimalMinMax, Dispersal, ActiveDispersal, LTVQueue, MaxBorrowLTVGroup, MaxLTVSlot, ExecuteMsg, TVLEntry
};
use membrane::math::{decimal_division, decimal_multiplication};
use membrane::types::{Basket, DepositDenom, AssetInfo};
use membrane::stability_pool_vault::{calculate_base_tokens, calculate_vault_tokens};
use membrane::cdp::ExecuteMsg as CDP_ExecuteMsg;
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::osmosis_proxy::ExecuteMsg as OsmosisProxy_ExecuteMsg;

use crate::error::ContractError;
use crate::state::{SwapPropagation, SWAP_PROPAGATION, REVENUE_TRACKING, RATE_ASSURANCE, REVENUE_EVENTS, USER_LIFETIME_REVENUE, BACKING_DEPOSITS, USER_DEPOSITS, CONFIG, DISPERSAL, LTV_QUEUES, DAILY_TVL_TRACKER, USER_TOTAL_DEPOSITS};

const REVENUE_TRACKING_LIMIT: usize = 100; // Limit for revenue tracking vectors
const LIFETIME_REVENUE_LIMIT: usize = 100; // Limit for user lifetime revenue tracking
const TVL_TRACKER_LIMIT: usize = 100; // Limit for TVL tracker entries
const ONE_DAY_SECONDS: u64 = 86400; // 24 hours in seconds

/// Helper to create composite key for BACKING_DEPOSITS map
fn make_deposit_key(asset: &str, ltv: &str, max_borrow_ltv: &str, user: &str) -> String {
    format!("{}:{}:{}:{}", asset, ltv, max_borrow_ltv, user)
}

/// Create a new LTV queue for an asset
pub fn create_queue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Owner or CDP contract can create queues
    if info.sender != config.owner && info.sender != config.cdp_contract {
        return Err(ContractError::Unauthorized {});
    }

    // Check if queue already exists
    if LTV_QUEUES.has(deps.storage, asset.clone()) {
        return Err(ContractError::CustomError {
            val: "Queue already exists".to_string(),
        });
    }

    // Query CDP contract for asset's max_LTV to set as minimum
    let basket: Basket = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.cdp_contract.to_string(),
        msg: to_json_binary(&CDP_QueryMsg::GetBasket {})?,
    }))?;

    // Find the asset in the basket to get its max_LTV
    let (min_liquidation_ltv, min_borrow_ltv) = basket.collateral_types
        .iter()
        .find(|c_asset| c_asset.asset.info.to_string() == asset)
        .map(|c_asset| (c_asset.max_LTV, c_asset.max_borrow_LTV))
        .ok_or_else(|| ContractError::CustomError {
            val: "Asset not found in CDP basket".to_string(),
        })?;

    // Create the LTV queue with empty slots (slots created on demand)
    let queue = LTVQueue {
        slots: Vec::new(),
        borrow_ltv: DecimalMinMax {
            min: min_borrow_ltv,
            max: config.max_ltv.clone(),
        },
        liquidation_ltv: DecimalMinMax {
            min: min_liquidation_ltv,
            max: config.max_ltv.clone(),
        },
        current_deposit_id: Uint128::new(1),
    };

    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "create_queue"),
            attr("asset", asset),
            attr("min_borrow_ltv", min_borrow_ltv.to_string()),
            attr("min_liquidation_ltv", min_liquidation_ltv.to_string()),
            attr("max_ltv", config.max_ltv.to_string()),
        ]))
}

/// Update an existing LTV queue
pub fn update_queue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
    mut max_ltv: Option<Decimal>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Only owner can set max LTV
    if info.sender != config.owner {
        max_ltv = None;
    }

    let mut queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;

    // Query CDP contract for updated minimum LTV
    let basket: Basket = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.cdp_contract.to_string(),
        msg: to_json_binary(&CDP_QueryMsg::GetBasket {})?,
    }))?;

    //Min LTV is set to the asset's max LTV bc we are using this to get HIGHER LTVs than what is set in the CDP
    let (new_min_liquidation_ltv, new_min_borrow_ltv) = basket.collateral_types
        .iter()
        .find(|c_asset| c_asset.asset.info.to_string() == asset)
        .map(|c_asset| (c_asset.max_LTV, c_asset.max_borrow_LTV))
        .ok_or_else(|| ContractError::CustomError {
            val: "Asset not found in CDP basket".to_string(),
        })?;

    let new_max_ltv = max_ltv.unwrap_or(config.max_ltv);

    // Update queue parameters
    queue.borrow_ltv.min = new_min_borrow_ltv;
    queue.borrow_ltv.max = new_max_ltv;
    queue.liquidation_ltv.min = new_min_liquidation_ltv;
    queue.liquidation_ltv.max = new_max_ltv;

    // Remove empty deposit groups (no vault tokens)
    queue.slots.iter_mut().for_each(|slot| {
        slot.deposit_groups.retain(|group| !group.total_vault_tokens.is_zero());
    });

    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "update_queue"),
            attr("asset", asset),
            attr("min_liquidation_ltv", new_min_liquidation_ltv.to_string()),
            attr("min_borrow_ltv", new_min_borrow_ltv.to_string()),
            attr("max_ltv", new_max_ltv.to_string()),
        ]))
}

/// Submit a backing deposit
pub fn submit_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    deposit_input: BackingDepositInput,
    deposit_owner: Option<String>,
) -> Result<Response, ContractError> {
    let mut msgs: Vec<CosmosMsg> = vec![];
    let config: Config = CONFIG.load(deps.storage)?;
    
    let valid_owner_addr = validate_deposit_owner(deps.api, info.clone(), deposit_owner)?;

    // Validate deposit amount
    if info.funds.len() != 1 {
        return Err(ContractError::TooManyAssets { valid: config.deposit_denom.denom.clone() });
    }

    // Validate asset denomination matches
    if info.funds[0].denom != deposit_input.asset {
        return Err(ContractError::CustomError {
            val: "Asset denomination mismatch".to_string(),
        });
    }

    let mut queue: LTVQueue = LTV_QUEUES.load(deps.storage, deposit_input.asset.clone())?;

    // Validate deposit input (asset & LTV checks)
    validate_deposit_input(deps.storage, deposit_input.clone())?;

    // Find or create the appropriate LTV slot
    let slot_index = find_or_create_ltv_slot(&mut queue, deposit_input.ltv)?;
    let mut slot = queue.slots[slot_index].clone();


    // Find or create the maxBorrowLTV group within the slot
    let group_index = find_or_create_borrow_group(&mut slot, deposit_input.max_borrow_ltv, true)?;
    let mut group = slot.deposit_groups[group_index].clone();

    // Calculate vault tokens to mint using vault-like mechanism
    let deposit_amount = info.funds[0].amount.clone();
    let vault_tokens = calculate_vault_tokens(
        deposit_amount,
        group.total_deposit_tokens,
        group.total_vault_tokens,
    )?; 

    // Event-based deposit storage key strings
    let asset_str = deposit_input.asset.clone();
    let ltv_str = slot.ltv.to_string();
    let max_borrow_ltv_str = deposit_input.max_borrow_ltv.to_string();
    let user_str = valid_owner_addr.to_string();
    let deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str);

    // Validate deposit amount
    if info.funds[0].amount < config.minimum_deposit {
        return Err(ContractError::InvalidDepositAmount {});
    }

    // If user already has a deposit in this group, auto-claim and top-up
    if let Some(mut existing) = BACKING_DEPOSITS.may_load(deps.storage, deposit_key.clone())? {
        // Claim revenues for existing deposit and send
        let claimed = claim_revenue_for_deposit(
            deps.storage,
            &env,
            &mut existing,
            deposit_input.asset.clone(),
            slot.ltv,
            deposit_input.max_borrow_ltv,
            None,
        )?;
        if !claimed.is_zero() {
            msgs.push(BankMsg::Send {
                to_address: valid_owner_addr.to_string(),
                amount: vec![Coin { denom: config.cdt_denom.clone(), amount: claimed }],
            }.into());
        }
        // Add new vault tokens to existing deposit
        existing.vault_tokens += vault_tokens.clone();
        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &existing)?;
    } else {
        // New deposit: initialize last_claimed to now
        let deposit = BackingDeposit {
            user: valid_owner_addr.clone(),
            vault_tokens: vault_tokens.clone(),
            max_borrow_ltv: deposit_input.max_borrow_ltv,
            wait_end: Some(env.block.time.plus_seconds(config.waiting_period).seconds()),
            last_claimed: env.block.time.seconds(),
        };
        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;
        // Add to USER_DEPOSITS index
        let mut keys = USER_DEPOSITS
            .may_load(deps.storage, (valid_owner_addr.clone(), deposit_input.asset.clone()))?
            .unwrap_or_else(Vec::new);
        //
        keys.push(deposit_key);
        //
        USER_DEPOSITS.save(deps.storage, (valid_owner_addr.clone(), deposit_input.asset.clone()), &keys)?;
    }


    //Must do this before updating totals for Rate Assurance checks
    update_rate_assurance(deps.storage, deposit_input.asset.clone(), deposit_input.ltv, deposit_input.max_borrow_ltv, &group)?;

    //Add rate assurance callback msg
    if !group.total_deposit_tokens.is_zero() && !group.total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {
                asset: deposit_input.asset.clone(),
                max_ltv: deposit_input.ltv,
                max_borrow_ltv: deposit_input.max_borrow_ltv,
            })?,
            funds: vec![],
        }));
    }

    // Update group totals
    group.total_deposit_tokens += deposit_amount;
    group.total_vault_tokens += vault_tokens.clone();

    // Update slot totals, just for easier global tracking.
    // We use slot totals to distribute revenue more efficiently.
    slot.total_deposit_tokens += deposit_amount;
    // slot.total_vault_tokens += deposit.vault_tokens; //No need to update this bc VTS are per group

    // Update queue
    slot.deposit_groups[group_index] = group.clone();
    queue.slots[slot_index] = slot;

    LTV_QUEUES.save(deps.storage, deposit_input.asset.clone(), &queue)?;

    // Update daily TVL tracker
    update_daily_tvl_tracker(deps.storage, &env, &deps.querier)?;

    // Update user total deposits
    let user_key = valid_owner_addr.to_string();
    let current_total = USER_TOTAL_DEPOSITS
        .may_load(deps.storage, user_key.clone())?
        .unwrap_or(Uint128::zero());
    USER_TOTAL_DEPOSITS.save(deps.storage, user_key, &(current_total + deposit_amount))?;

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "submit_deposit"),
            attr("deposit_owner", valid_owner_addr.to_string()),
            attr("asset", deposit_input.asset),
            attr("ltv", deposit_input.ltv.to_string()),
            attr("max_borrow_ltv", deposit_input.max_borrow_ltv.to_string()),
            attr("amount", deposit_amount.to_string()),
            attr("vault_tokens", vault_tokens.to_string()),
            attr("action", "submitted"),
        ]))
}

/// Withdraw a backing deposit
pub fn withdraw_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    ltv: Decimal,
    max_borrow_ltv: Decimal,
    amount: Option<Uint128>, //Amount of base tokens to withdraw
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    
    //Create deposit key
    let asset_str = asset.clone();
    let ltv_str = ltv.to_string();
    let max_borrow_ltv_str = max_borrow_ltv.to_string();
    let user_str = info.sender.to_string(); //This gates withdrawals to the owner of the deposit
    let deposit_key = make_deposit_key(&asset_str, &ltv_str, &max_borrow_ltv_str, &user_str);
    // Load deposit from BACKING_DEPOSITS map
    let mut deposit = BACKING_DEPOSITS.load(deps.storage, deposit_key.clone())?;

    // Claim revenue before withdrawal and send to user
    let claimed_revenue = claim_revenue_for_deposit(
        deps.storage,
        &env,
        &mut deposit,
        asset.clone(),
        ltv,
        max_borrow_ltv,
        None, //no limit = 100
    )?;

    // Get Slot
    let slot_index = queue.slots.iter().position(|s| s.ltv == ltv)
        .ok_or_else(|| ContractError::CustomError { val: "Slot not found".to_string() })?;
    let mut slot = queue.slots[slot_index].clone();
    
    // Get Group
    let group_index = find_or_create_borrow_group(&mut slot, max_borrow_ltv, false)?;
    let mut group = slot.deposit_groups[group_index].clone();

    //Must do this before updating group totals for Rate Assurance checks
    update_rate_assurance(deps.storage, asset.clone(), slot.ltv, max_borrow_ltv, &group)?;

    // Calculate base tokens to withdraw
    let vault_tokens_to_withdraw = if let Some(amount) = amount {
        calculate_vault_tokens(
            amount,
            group.total_deposit_tokens,
            group.total_vault_tokens,
        )?
    } else {
        deposit.vault_tokens
    };

    let withdraw_vault_tokens = std::cmp::min(vault_tokens_to_withdraw, deposit.vault_tokens);

    // Calculate base tokens to withdraw using vault-like mechanism
    let base_tokens_to_withdraw = calculate_base_tokens(
        withdraw_vault_tokens,
        group.total_deposit_tokens,
        group.total_vault_tokens,
    )?;

    // Update group totals
    group.total_deposit_tokens -= base_tokens_to_withdraw;
    group.total_vault_tokens -= withdraw_vault_tokens;

    // Remove or update deposit
    let fully_withdrawn = withdraw_vault_tokens == deposit.vault_tokens;
    if fully_withdrawn {
        // Remove deposit from BACKING_DEPOSITS
        BACKING_DEPOSITS.remove(deps.storage, deposit_key.clone());
        
        // Remove from USER_DEPOSITS index
        let mut user_keys = USER_DEPOSITS
            .may_load(deps.storage, (info.sender.clone(), asset.clone()))?
            .unwrap_or_else(Vec::new);
        user_keys.retain(|k| k != &deposit_key);
        if user_keys.is_empty() {
            USER_DEPOSITS.remove(deps.storage, (info.sender.clone(), asset.clone()));
        } else {
            USER_DEPOSITS.save(deps.storage, (info.sender.clone(), asset.clone()), &user_keys)?;
        }
    } else {
        // Update deposit
        deposit.vault_tokens -= withdraw_vault_tokens;

        // Calculate remaining base tokens
        let remaining_base_tokens = calculate_base_tokens(
            deposit.vault_tokens,
            group.total_deposit_tokens,
            group.total_vault_tokens,
        )?;

        // Validate withdrawal amount
        if remaining_base_tokens < config.minimum_deposit {
            return Err(ContractError::InvalidWithdrawal {
                minimum: config.minimum_deposit,
            });
        }
        
        // Save updated deposit
        BACKING_DEPOSITS.save(deps.storage, deposit_key.clone(), &deposit)?;
    }

    // Update slot totals
    slot.total_deposit_tokens -= base_tokens_to_withdraw;
    // Update queue
    slot.deposit_groups[group_index] = group.clone();
    queue.slots[slot_index] = slot.clone();
    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    // Update daily TVL tracker
    update_daily_tvl_tracker(deps.storage, &env, &deps.querier)?;

    // Update user total deposits
    let user_key = info.sender.to_string();
    let current_total = USER_TOTAL_DEPOSITS
        .may_load(deps.storage, user_key.clone())?
        .unwrap_or(Uint128::zero());
    let new_total = current_total.saturating_sub(base_tokens_to_withdraw);
    if new_total.is_zero() {
        USER_TOTAL_DEPOSITS.remove(deps.storage, user_key);
    } else {
        USER_TOTAL_DEPOSITS.save(deps.storage, user_key, &new_total)?;
    }

    // Send tokens back to user (base tokens + claimed revenue)
    let mut msgs: Vec<CosmosMsg> = vec![];
    if !base_tokens_to_withdraw.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: asset.clone(),
                amount: base_tokens_to_withdraw,
            }],
        }.into());
    } else {
        return Err(ContractError::InvalidWithdrawal {
            minimum: Uint128::zero(),
        });
    }
    
    // Send claimed revenue
    if !claimed_revenue.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: config.cdt_denom.clone(),
                amount: claimed_revenue,
            }],
        }.into());
    }

    // Add rate assurance callback msg if remaining tokens are non-zero
    if !group.total_deposit_tokens.is_zero() && !group.total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {
                asset: asset.clone(),
                max_ltv: slot.ltv,
                max_borrow_ltv: max_borrow_ltv,
            })?,
            funds: vec![],
        }));
    }

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "withdraw_deposit"),
            attr("asset", asset),
            attr("ltv", ltv.to_string()),
            attr("max_borrow_ltv", max_borrow_ltv.to_string()),
            attr("vault_tokens", withdraw_vault_tokens.to_string()),
            attr("base_tokens", base_tokens_to_withdraw.to_string()),
            attr("claimed_revenue", claimed_revenue.to_string()),
        ]))
}

/// Add bad debt to an LTV queue (CDP contract only)
pub fn add_bad_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut msgs: Vec<SubMsg> = vec![];

    // Only CDP contract can add bad debt
    if info.sender != config.cdp_contract {
        return Err(ContractError::Unauthorized {});
    }
    /////if the deposit denom is the Transmuter's vault token///////
    // DON'T DELETE.
    // if let Some(vault_info) = config.deposit_denom.vault_info.clone(){
    //     //1) Query how many vault tokens is the bad debt amount worth in the vault contract
    //     let bad_debt_as_vault_tokens = deps.querier.query_wasm_smart::<Uint128>(
    //         vault_info.vault_contract.clone(),
    //         &Transmuter_QueryMsg::DepositTokenConversion { deposit_token_amount: amount },
    //     )?;
    //     //2) Update amount to denominate as vault tokens
    //     amount = bad_debt_as_vault_tokens;
    //     //3) Withdraw the vault tokens from the vault contract
    //     msgs.push(SubMsg::reply_on_error(CosmosMsg::Wasm(WasmMsg::Execute {
    //         contract_addr: vault_info.vault_contract.clone(),
    //         msg: to_json_binary(&Transmuter_ExecuteMsg::ExitVault { 
    //             recipient: None, //set to none so we don't have to conditionally add the CDP_ExecuteMsg::FulfillBadDebt Msg at the end of this fn
    //             withdraw_as: Some(vault_info.underlying_token.clone()),
    //          })?,
    //         funds: vec![
    //             Coin {
    //                 denom: config.deposit_denom.denom.clone(),
    //                 amount: bad_debt_as_vault_tokens,
    //             }
    //         ],
    //     }), TRANSMUTER_REPLY_ID));
    //     //We reply on error, and add the errored amount to PENDING_BAD_DEBT using BAD_DEBT_PROPAGATION data 
    //     BAD_DEBT_PROPAGATION.save(deps.storage, &BadDebtPropagation {
    //         asset: asset.clone(),
    //         amount: amount,
    //     })?;
    // }

    //Bad debt is sent per asset
    let mut queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;

    // Step 1: Handle bad debt waterfall (dispersals → revenue events)
    let (revenue_fulfilled, mut remaining_bad_debt) = handle_bad_debt_waterfall(
        deps.storage,
        &queue,
        asset.clone(),
        amount,
    )?;

    // Step 2: If revenue was used to fulfill bad debt, send it to CDP
    if !revenue_fulfilled.is_zero() {
        msgs.push(SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.cdp_contract.to_string(),
            msg: to_json_binary(&CDP_ExecuteMsg::FulfillBadDebt {})?,
            funds: vec![Coin {
                denom: config.cdt_denom.clone(),
                amount: revenue_fulfilled,
            }],
        })));
    }

    // Step 3: If there's remaining bad debt, slash deposits
    let mut total_slashed = Uint128::zero();

    if !remaining_bad_debt.is_zero() {
        // Convert remaining CDT bad debt to equivalent collateral amount using oracle
        let asset_info = AssetInfo::NativeToken { denom: asset.clone() };
        let cdt_info = AssetInfo::NativeToken { denom: config.cdt_denom.clone() };
        let asset_infos = vec![asset_info.clone(), cdt_info.clone()];
        
        let price_response: Vec<PriceResponse> = deps.querier.query_wasm_smart(
            config.oracle_contract.to_string(),
            &Oracle_QueryMsg::Prices {
                asset_infos,
                twap_timeframe: 0,
                oracle_time_limit: 0,
            },
        )?;

        // Convert CDT bad debt to collateral amount
        // CDT value -> asset value -> asset amount
        let cdt_value = price_response[1].get_value(remaining_bad_debt)?; // CDT bad debt in USD value
        let collateral_amount_needed = price_response[0].get_amount(cdt_value)?; // Convert USD value to collateral amount

        let mut remaining_collateral_to_slash = collateral_amount_needed;

        // Sort slots by LTV in descending order for deposit slashing
        let mut sorted_slots: Vec<(usize, MaxLTVSlot)> = queue.slots
            .iter()
            .enumerate()
            //Skip empty slots
            .filter(|(_, slot)| !slot.total_deposit_tokens.is_zero())
            .map(|(i, slot)| (i, slot.clone()))
            .collect();
        sorted_slots.sort_by(|a, b| b.1.ltv.cmp(&a.1.ltv));

        for (slot_index, mut slot) in sorted_slots {
            if remaining_collateral_to_slash.is_zero() {
                break;
            }

            // Track deposits seen in this slot for early exit optimization
            let mut total_deposits_seen_per_slot = Uint128::zero();

            // Sort groups by max_borrow_ltv in descending order
            slot.deposit_groups.sort_by(|a, b| b.max_borrow_ltv.cmp(&a.max_borrow_ltv));

            for group in &mut slot.deposit_groups {
                if remaining_collateral_to_slash.is_zero() {
                    break;
                }

                //Skip empty groups
                if group.total_deposit_tokens.is_zero() {
                    continue;
                }
                // Track seen deposits for optimization
                total_deposits_seen_per_slot += group.total_deposit_tokens;

                // Calculate collateral to slash from this group
                let group_slash_amount = std::cmp::min(remaining_collateral_to_slash, group.total_deposit_tokens);

                // Update Group
                group.total_deposit_tokens -= group_slash_amount;
                slot.bad_debt += price_response[1].get_amount(price_response[0].get_value(group_slash_amount)?)?;
                total_slashed += group_slash_amount;
                remaining_collateral_to_slash -= group_slash_amount;

                // Update remaining_bad_debt by converting slashed collateral to CDT.
                // For attribute accuracy only.
                if !total_slashed.is_zero() {
                    let slashed_cdt_value = price_response[1].get_amount(price_response[0].get_value(group_slash_amount)?)?;
                    remaining_bad_debt = remaining_bad_debt.saturating_sub(slashed_cdt_value);
                }

                

                // Early exit if we've seen all deposits in this slot
                if total_deposits_seen_per_slot >= slot.total_deposit_tokens {
                    break;
                }
            }

            queue.slots[slot_index] = slot;
        }
        
    }

    // Step 4: If deposits were slashed, create liquidation swap message
    if !total_slashed.is_zero() {
        let swap_msg = create_liquidation_swap_msg(
            deps.storage,
            &deps.querier,
            &env,
            &config,
            asset.clone(),
            total_slashed,
        )?;
        msgs.push(swap_msg);
    }

    // Save queue
    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    Ok(Response::new()
        .add_submessages(msgs)
        .add_attributes(vec![
            attr("method", "add_bad_debt"),
            attr("asset", asset),
            attr("total_bad_debt_cdt", amount.to_string()),
            attr("fulfilled_from_revenue_cdt", revenue_fulfilled.to_string()),
            attr("slashed_collateral_amount", total_slashed.to_string()),
            attr("remaining_bad_debt_cdt", remaining_bad_debt.to_string()),
        ]))
}

/// Add revenue to an asset's LTV queue.
/// Distributes rewards to users based on their vault token holdings
pub fn add_revenue(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Validate that only the configured CDT token is sent
    if info.funds.len() != 1 || info.funds[0].denom != config.cdt_denom {
        return Err(ContractError::CustomError {
            val: "Invalid CDT token denomination".to_string(),
        });
    }

    let mut revenue_amount = info.funds[0].amount;
    let queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;

    let response = Response::new();

    // Calculate total deposit tokens to check if any deposits exist
    let total_deposit_tokens: Uint128 = queue.slots
        .iter()
        .map(|slot| slot.total_deposit_tokens)
        .sum();

    let mut dispersal = match DISPERSAL.load(deps.storage, asset.clone()){
        Ok(dispersal) => dispersal,
        Err(_) => Dispersal {
            total_to_disperse: Uint128::zero(),
            dispersal_window: config.dispersal_window,
            active_dispersal: ActiveDispersal {
                dispersal_start: 0,
                amount_dispersed: Uint128::zero(),
            },
            pending_dispersal: Uint128::zero()
        },
    };

    // If no deposits exist, send all revenue to dispersal
    if total_deposit_tokens.is_zero() {
        if dispersal.active_dispersal.dispersal_start == 0 {
            // When dispersal is not active, add to the total to disperse for the upcoming dispersal period
            dispersal.total_to_disperse += revenue_amount;
        } else {
            // When dispersal is active, add to the pending dispersal for the next dispersal period
            dispersal.pending_dispersal += revenue_amount;
        }
        DISPERSAL.save(deps.storage, asset.clone(), &dispersal)?;
        
        return Ok(response
            .add_attributes(vec![
                attr("method", "add_revenue"),
                attr("asset", asset),
                attr("amount", revenue_amount.to_string()),
                attr("routed_to_dispersal", "all"),
            ]));
    }

    // If deposits exist, calculate portion to add to dispersal
    let revenue_decimal = Decimal::from_ratio(revenue_amount, Uint128::one());
    let disperse_percent = decimal_multiplication(revenue_decimal, config.percent_to_disperse)?;
    let disperse_amount = disperse_percent.to_uint_floor();
    if !disperse_amount.is_zero() && dispersal.active_dispersal.dispersal_start == 0 {
        //When dispersal is not active, we add to the total to disperse for the upcoming dispersal period
        revenue_amount -= disperse_amount;
        dispersal.total_to_disperse += disperse_amount;
        DISPERSAL.save(deps.storage, asset.clone(), &dispersal)?;
    } else if !disperse_amount.is_zero() && dispersal.active_dispersal.dispersal_start != 0 {
        //When dispersal is active, we add to the pending dispersal for the next dispersal period
        revenue_amount -= disperse_amount;
        dispersal.pending_dispersal += disperse_amount;
        DISPERSAL.save(deps.storage, asset.clone(), &dispersal)?;
    }
    
    // Distribute remaining revenue to users based on their vault token holdings
    distribute_revenue_to_users(deps.storage, &env, &queue, asset.clone(), revenue_amount)?;

    Ok(response
        .add_attributes(vec![
            attr("method", "add_revenue"),
            attr("asset", asset),
            attr("amount", revenue_amount.to_string()),
        ]))
}

/// Distribute revenue to users based on their vault token holdings
/// Creates RevenueEvent structs instead of immediately distributing to users
fn distribute_revenue_to_users(
    storage: &mut dyn Storage,
    env: &Env,
    queue: &LTVQueue,
    asset: String,
    revenue_amount: Uint128,
) -> Result<(), ContractError> {
    // Calculate total deposit tokens across all slots
    let total_deposit_tokens: Uint128 = queue.slots
        .iter()
        .map(|slot| slot.total_deposit_tokens)
        .sum();
    
    if total_deposit_tokens.is_zero() {
        return Ok(());
    }

    // First tier: Distribute revenue to slots based on their deposit tokens
    for slot in &queue.slots {
        if slot.total_deposit_tokens.is_zero() {
            continue;
        }
        
        let slot_share_ratio = Decimal::from_ratio(
            slot.total_deposit_tokens.u128(), 
            total_deposit_tokens.u128()
        );
        let slot_revenue = revenue_amount * slot_share_ratio;
        
        if slot_revenue.is_zero() {
            continue;
        }
        
        // Second tier: Distribute slot revenue to groups based on their deposit tokens
        for group in &slot.deposit_groups {
            if group.total_deposit_tokens.is_zero() || group.total_vault_tokens.is_zero() {
                continue;
            }
            
            let group_share_ratio = Decimal::from_ratio(
                group.total_deposit_tokens.u128(),
                slot.total_deposit_tokens.u128()
            );
            let group_revenue = slot_revenue * group_share_ratio;
            
            if group_revenue.is_zero() {
                continue;
            }
            
            // Calculate amount per 1 vault token (as Decimal)
            let amount_per_vt = Decimal::from_ratio(
                group_revenue.u128(),
                group.total_vault_tokens.u128()
            );
            
            // Create revenue event
            let event = RevenueEvent {
                timestamp: env.block.time.seconds(),
                amount_per_vt,  // Store as Decimal for direct multiplication
                amount_to_be_claimed: group_revenue,
            };
            
            // Store event
            let key = (asset.clone(), slot.ltv.to_string(), group.max_borrow_ltv.to_string());
            let mut events = REVENUE_EVENTS
                .may_load(storage, key.clone())?
                .unwrap_or_else(Vec::new);
            events.push(event);
            REVENUE_EVENTS.save(storage, key, &events)?;
            
            // Track cumulative revenue
            add_revenue_tracking_entry(
                storage,
                env.clone(),
                asset.clone(),
                slot.ltv,
                group.max_borrow_ltv,
                group_revenue
            )?;
        }
    }

    Ok(())
}

/// Add revenue tracking entry for a specific slot/group combination
fn add_revenue_tracking_entry(
    storage: &mut dyn Storage,
    env: Env,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
    revenue_amount: Uint128,
) -> Result<(), ContractError> {
    let timestamp = env.block.time.seconds();
    let key = (asset, max_ltv.to_string(), max_borrow_ltv.to_string());
    
    // Load existing entries
    let mut entries = REVENUE_TRACKING
        .may_load(storage, key.clone())?
        .unwrap_or_else(Vec::new);
    
    // Calculate new total (add to previous total or start fresh)
    let new_total = if let Some(last_entry) = entries.last() {
        last_entry.total_revenue + revenue_amount
    } else {
        revenue_amount
    };
    
    // Create new entry
    let entry = RevenueTrackingEntry {
        timestamp,
        total_revenue: new_total,
    };
    
    entries.push(entry);
    
    // Apply limit
    if entries.len() > REVENUE_TRACKING_LIMIT {
        entries.drain(0..entries.len() - REVENUE_TRACKING_LIMIT);
    }
    
    REVENUE_TRACKING.save(storage, key, &entries)?;
    Ok(())
}

/// Claim revenue for a specific deposit (internal helper)
fn claim_revenue_for_deposit(
    storage: &mut dyn Storage,
    env: &Env,
    deposit: &mut BackingDeposit,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
    limit: Option<u32>,
) -> Result<Uint128, ContractError> {
    let key = (asset.clone(), max_ltv.to_string(), max_borrow_ltv.to_string());
    let mut events = REVENUE_EVENTS
        .may_load(storage, key.clone())?
        .unwrap_or_else(Vec::new);
    
    let mut total_claimed = Uint128::zero();
    let mut events_processed = 0u32;
    let max_events = limit.unwrap_or(100);
    
    for event in events.iter_mut() {
        if events_processed >= max_events {
            break;
        }
        
        if event.timestamp <= deposit.last_claimed {
            continue;
        }
        
        if event.amount_to_be_claimed.is_zero() {
            continue;
        }
        
        // Direct multiplication: vault_tokens * amount_per_vt (Decimal) auto-floors the decimal
        let mut user_share = event.amount_per_vt * deposit.vault_tokens;
        
        if !user_share.is_zero() {
            //If the user share is greater than the amount to be claimed, set the user share to the amount to be claimed and set the amount to be claimed to zero
            //This is to prevent overflow errors
            if event.amount_to_be_claimed < user_share {
                user_share = event.amount_to_be_claimed;
                event.amount_to_be_claimed = Uint128::zero();
            } else {
                event.amount_to_be_claimed = event.amount_to_be_claimed.checked_sub(user_share)
                    .map_err(|e| ContractError::CustomError { val: format!("Overflow subtracting user share: {}", e) })?;
            }
            //Add to total claimed
            total_claimed = total_claimed.checked_add(user_share)
                .map_err(|e| ContractError::CustomError { val: format!("Overflow adding user share: {}", e) })?;
        }
        //Increment events processed
        events_processed += 1;
    }
    
    deposit.last_claimed = env.block.time.seconds();
    
    // Trim events with zero amount_to_be_claimed
    events.retain(|e| !e.amount_to_be_claimed.is_zero());
    REVENUE_EVENTS.save(storage, key, &events)?;
    
    // Update user lifetime revenue
    if !total_claimed.is_zero() {
        update_user_lifetime_revenue(storage, deposit.user.clone(), asset.clone(), total_claimed, env.block.time.seconds())?;
    }
    
    Ok(total_claimed)
}

/// Update user lifetime revenue tracking with limit
fn update_user_lifetime_revenue(
    storage: &mut dyn Storage,
    user: Addr,
    asset: String,
    amount: Uint128,
    timestamp: u64,
) -> Result<(), ContractError> {
    let mut entries = USER_LIFETIME_REVENUE
        .may_load(storage, (user.clone(), asset.clone()))?
        .unwrap_or_else(Vec::new);
    
    // Calculate new cumulative total
    let new_total = if let Some(last_entry) = entries.last() {
        last_entry.total_claimed.checked_add(amount)
            .map_err(|e| ContractError::CustomError { val: format!("Overflow adding to lifetime revenue: {}", e) })?
    } else {
        amount
    };
    
    // Create new entry
    let entry = UserLifetimeRevenueEntry {
        timestamp,
        total_claimed: new_total,
    };
    
    //Add to entries
    entries.push(entry);
    
    // Apply limit
    if entries.len() > LIFETIME_REVENUE_LIMIT {
        entries.drain(0..entries.len() - LIFETIME_REVENUE_LIMIT);
    }
    
    USER_LIFETIME_REVENUE.save(storage, (user, asset), &entries)?;
    Ok(())
}

/// Claim accumulated revenue rewards for a user (public execute)
/// Uses USER_DEPOSITS map to find all deposits for the user
pub fn claim_revenue_for_user(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: String,
    asset: String,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let user_addr = deps.api.addr_validate(&user)?;
    
    // Load all deposit keys for this user and asset
    let deposit_keys = USER_DEPOSITS
        .may_load(deps.storage, (user_addr.clone(), asset.clone()))?
        .unwrap_or_else(Vec::new);
    
    if deposit_keys.is_empty() {
        return Err(ContractError::CustomError {
            val: "No deposits found for user".to_string(),
        });
    }
    
    let mut total_claimed = Uint128::zero();
    
    // Iterate through all deposits and claim from each
    for deposit_key_str in deposit_keys {
        if let Ok(mut deposit) = BACKING_DEPOSITS.load(deps.storage, deposit_key_str.clone()) {
            // Parse LTV values from the key string (format: "asset:ltv:max_borrow_ltv:user")
            let parts: Vec<&str> = deposit_key_str.split(':').collect();
            if parts.len() != 4 {
                continue;
            }
            let max_ltv = Decimal::from_str(parts[1])
                .map_err(|_| ContractError::CustomError { val: "Invalid LTV format".to_string() })?;
            let max_borrow_ltv = Decimal::from_str(parts[2])
                .map_err(|_| ContractError::CustomError { val: "Invalid max_borrow_ltv format".to_string() })?;
            
            let claimed = claim_revenue_for_deposit(
                deps.storage,
                &env,
                &mut deposit,
                asset.clone(),
                max_ltv,
                max_borrow_ltv,
                limit,
            )?;
            
            total_claimed = total_claimed.checked_add(claimed)
                .map_err(|e| ContractError::CustomError { val: format!("Overflow adding claimed revenue: {}", e) })?;
            
            // Save updated deposit
            BACKING_DEPOSITS.save(deps.storage, deposit_key_str, &deposit)?;
        }
    }
    
    // Send revenue to user
    let mut msgs: Vec<CosmosMsg> = vec![];
    if !total_claimed.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: user_addr.to_string(),
            amount: vec![Coin {
                denom: config.cdt_denom.clone(),
                amount: total_claimed,
            }],
        }.into());
    }
    
    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "claim_revenue_for_user"),
            attr("caller", info.sender.to_string()),
            attr("user", user_addr.to_string()),
            attr("asset", asset),
            attr("claimed_amount", total_claimed.to_string()),
        ]))
}

/// Update contract configuration
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    cdp_contract: Option<String>,
    deposit_denom: Option<DepositDenom>,
    cdt_denom: Option<String>,
    minimum_deposit: Option<Uint128>,
    waiting_period: Option<u64>,
    percent_to_disperse: Option<Decimal>,
    dispersal_window: Option<u64>,
    activation_window: Option<u64>,
    oracle_contract: Option<String>,
    chain_proxy_contract: Option<String>,
) -> Result<Response, ContractError> {
    let mut config: Config = CONFIG.load(deps.storage)?;

    // Only owner can update config
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(owner) = owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }
    if let Some(cdp_contract) = cdp_contract {
        config.cdp_contract = deps.api.addr_validate(&cdp_contract)?;
    }
    if let Some(deposit_denom) = deposit_denom {
        config.deposit_denom = deposit_denom.clone();
    }
    if let Some(cdt_denom) = cdt_denom {
        config.cdt_denom = cdt_denom;
    }
    if let Some(minimum_deposit) = minimum_deposit {
        config.minimum_deposit = minimum_deposit;
    }
    if let Some(waiting_period) = waiting_period {
        config.waiting_period = waiting_period;
    }
    if let Some(percent_to_disperse) = percent_to_disperse {
        config.percent_to_disperse = percent_to_disperse;
    }
    if let Some(dispersal_window) = dispersal_window {
        config.dispersal_window = dispersal_window;
    }
    if let Some(activation_window) = activation_window {
        config.activation_window = activation_window;
    }
    if let Some(oracle_contract) = oracle_contract {
        config.oracle_contract = deps.api.addr_validate(&oracle_contract)?;
    }
    if let Some(chain_proxy_contract) = chain_proxy_contract {
        config.chain_proxy_contract = deps.api.addr_validate(&chain_proxy_contract)?;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "update_config"),
            attr("config", format!("{:?}", config)),
        ]))
}

/// Disperse revenue linearly over the active window
pub fn disperse_revenue(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Only CDP contract can disperse revenue
    if info.sender != config.cdp_contract {
        return Err(ContractError::Unauthorized {});
    }

    // Load dispersal
    let mut dispersal = DISPERSAL.load(deps.storage, asset.clone())?;
    let current_time = env.block.time.seconds();

    // Check if dispersal is active (has active_dispersal)
    if dispersal.active_dispersal.dispersal_start == 0 {
        /////Check if this dispersal is activatible if not, error////

        //Calc block time of the start of the activation window
        let activation_window_in_seconds = config.clone().activation_window * 60;
        let start_of_activation_window = current_time.checked_sub(activation_window_in_seconds).ok_or(ContractError::CustomError { val: "Activation window subtraction underflow (should be impossible here)".to_string() })?;
        //Query the CDP's Liquidation history
        let liquidation_history: Vec<LiquidationStatResponse> = match deps.querier.query_wasm_smart::<Vec<LiquidationStatResponse>>(
            config.cdp_contract.to_string(),
            &CDP_QueryMsg::GetLiquidationStats { 
                start_after: Some(start_of_activation_window - 1u64), 
                limit: Some(100u32) 
            },
        ){
            Ok(liquidation_history) => liquidation_history,
            Err(_) => return Err(ContractError::CustomError { val: format!("Failed to query the CDP liquidation history starting after {}", start_of_activation_window) }),
        };

        //If the history isn't empty, it means there is an event that started within the activation window
        if !liquidation_history.is_empty(){
            //If any of the found events have the asset we're looking to disperse for
            if let Some(entry) = liquidation_history.into_iter()
            .find(|entry | entry.clone().collateral_assets.into_iter().map(|asset| asset.info.to_string()).collect::<Vec<String>>().contains(&asset.clone())){
                //Set the disperal start time to the time of the found liquidation history entry for the asset
                dispersal.active_dispersal.dispersal_start = entry.block_time;
            } else { return Err(ContractError::DispersalNotActive {}) }
        } else { return Err(ContractError::DispersalNotActive {}) }
    }

    // Calculate elapsed time since dispersal started
    let elapsed_seconds = current_time - dispersal.active_dispersal.dispersal_start;
    let elapsed_hours = elapsed_seconds / 3600; // Convert seconds to hours

    // Calculate how much should have been dispersed by now
    let total_hours = dispersal.dispersal_window;
    let dispersal_rate = decimal_division(
        Decimal::from_ratio(dispersal.total_to_disperse, Uint128::one()),
        Decimal::from_ratio(total_hours, Uint128::one()),
    )?;
    
    let total_should_disperse = decimal_multiplication(
        dispersal_rate,
        Decimal::from_ratio(elapsed_hours, Uint128::one()),
    )?.to_uint_floor();

    // Calculate how much to disperse this time (difference between what should be dispersed and what has been dispersed)
    let current_dispersed = dispersal.active_dispersal.amount_dispersed;
    let disperse_amount = if total_should_disperse > current_dispersed {
        total_should_disperse - current_dispersed
    } else {
        Uint128::zero()
    };

    // Ensure we don't disperse more than total
    let final_disperse_amount = disperse_amount.min(dispersal.total_to_disperse - current_dispersed);

    // Update dispersal tracking
    dispersal.active_dispersal.amount_dispersed += final_disperse_amount;
    if dispersal.active_dispersal.amount_dispersed > dispersal.total_to_disperse {
        return Err(ContractError::CustomError {
            val: "Dispersal amount exceeds total to disperse".to_string(),
        });
    } else if dispersal.active_dispersal.amount_dispersed == dispersal.total_to_disperse {
        let _ = DISPERSAL.save(deps.storage, asset.clone(), &Dispersal {
            total_to_disperse:  dispersal.clone().pending_dispersal,
            dispersal_window: config.dispersal_window,
            active_dispersal: ActiveDispersal {
                dispersal_start: 0,
                amount_dispersed: Uint128::zero(),
            },
            pending_dispersal: Uint128::zero()
        });
    } else {
        DISPERSAL.save(deps.storage, asset.clone(), &dispersal)?;
    }

    // Load queue and distribute the dispersed amount to claimable revenue
    let queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    distribute_revenue_to_users(deps.storage, &env, &queue, asset.clone(), final_disperse_amount)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "disperse_revenue"),
            attr("asset", asset),
            attr("elapsed_hours", elapsed_hours.to_string()),
            attr("disperse_amount", final_disperse_amount.to_string()),
        ]))
}

//DONT DELETE.
/// Retry failed bad debt, only necessary for the Transmuter's exit failures.
/// Pull from the pending bad debt map & attempt to withdraw it from the Transmuter & send to the CDP contract as a FulfillBadDebt Msg.
/// If the withdrawal errors, we don't update the pending bad debt map.
/// If it succeeds, we update the pending bad debt map.
// pub fn retry_failed_bad_debt(
//     deps: DepsMut,
//     _env: Env,
//     _info: MessageInfo,
//     asset: String,
// ) -> Result<Response, ContractError> {
//     let mut msgs: Vec<SubMsg> = vec![];
//     let config = CONFIG.load(deps.storage)?;
//     //Load the pending bad debt
//     let pending_bad_debt = PENDING_BAD_DEBT.load(deps.storage, asset.clone())?;
//     //Save the pending asset to the BAD_DEBT_PROPAGATION
//     BAD_DEBT_PROPAGATION.save(deps.storage, &BadDebtPropagation {
//         asset: asset.clone(),
//         amount: pending_bad_debt,
//     })?;

//     //Set the denom
//     let withdraw_as_denom = config.deposit_denom.vault_info.clone().unwrap().underlying_token.clone();

//     //Attempt to withdraw the pending bad debt from the Transmuter
//     msgs.push(SubMsg::reply_on_success(CosmosMsg::Wasm(WasmMsg::Execute {
//         contract_addr: config.deposit_denom.vault_info.clone().unwrap().vault_contract.clone(),
//         msg: to_json_binary(&Transmuter_ExecuteMsg::ExitVault { 
//             recipient: None, //set to none so we don't have to conditionally add the CDP_ExecuteMsg::FulfillBadDebt Msg at the end of this fn
//             withdraw_as: Some(withdraw_as_denom.clone()),
//          })?,
//         funds: vec![
//             Coin {
//                 denom: config.deposit_denom.denom.clone(),
//                 amount: pending_bad_debt,
//             }
//         ],
//     }), TRANSMUTER_REPLY_ID));

//     //convert the pending bad debt to the underlying token
//     let pending_bad_debt_as_underlying = deps.querier.query_wasm_smart::<Uint128>(
//         config.deposit_denom.vault_info.clone().unwrap().vault_contract.clone(),
//         &Transmuter_QueryMsg::VaultTokenUnderlying { vault_token_amount: pending_bad_debt },
//     )?;

//     //Add the CDP_ExecuteMsg::FulfillBadDebt Msg 
//     msgs.push(SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//         contract_addr: config.cdp_contract.to_string(),
//         msg: to_json_binary(&CDP_ExecuteMsg::FulfillBadDebt {})?,
//         funds: vec![
//             Coin {
//                 denom: withdraw_as_denom,
//                 amount: pending_bad_debt_as_underlying,
//             }
//         ],
//     })));

//     Ok(Response::new()
//         .add_submessages(msgs)
//         .add_attributes(vec![
//             attr("method", "retry_failed_bad_debt"),
//             attr("asset", asset),
//         ]))
// }


/// Find or create LTV slot for a given LTV (1% increments)
fn find_or_create_ltv_slot(queue: &mut LTVQueue, ltv: Decimal) -> Result<usize, ContractError> {
    // Round LTV to nearest 1% increment
    let rounded_ltv = Decimal::from_ratio(ltv * Uint128::new(100), Uint128::new(100));

    // Check if slot exists
    if let Some(index) = queue.slots.iter().position(|slot| slot.ltv == rounded_ltv) {
        return Ok(index);
    }

    // Create new slot
    let new_slot = MaxLTVSlot {
        ltv: rounded_ltv,
        deposit_groups: Vec::new(),
        bad_debt: Uint128::zero(),
        total_deposit_tokens: Uint128::zero(),
    };

    queue.slots.push(new_slot);
    queue.slots.sort_by(|a, b| a.ltv.cmp(&b.ltv));

    Ok(queue.slots.len() - 1)
}

/// Find or create maxBorrowLTV group within a slot
fn find_or_create_borrow_group(slot: &mut MaxLTVSlot, max_borrow_ltv: Decimal, create_if_not_exists: bool) -> Result<usize, ContractError> {
    // Check if group exists
    if let Some(index) = slot.deposit_groups.iter().position(|group| group.max_borrow_ltv == max_borrow_ltv) {
        return Ok(index);
    }

    if create_if_not_exists {
        // Create new group
        let new_group = MaxBorrowLTVGroup {
            max_borrow_ltv,
            total_deposit_tokens: Uint128::zero(),
            total_vault_tokens: Uint128::zero(),
        };

        slot.deposit_groups.push(new_group);
        slot.deposit_groups.sort_by(|a, b| a.max_borrow_ltv.cmp(&b.max_borrow_ltv));

        Ok(slot.deposit_groups.len() - 1)
    } else {
        Err(ContractError::CustomError {
            val: "Group not found".to_string(),
        })
    }
}

/// Validate deposit input
fn validate_deposit_input(deps: &dyn Storage, deposit_input: BackingDepositInput) -> Result<(), ContractError> {
    match LTV_QUEUES.load(deps, deposit_input.asset.clone()) {
        Ok(queue) => {
            if deposit_input.ltv >= queue.liquidation_ltv.min && deposit_input.ltv <= queue.liquidation_ltv.max 
            && deposit_input.max_borrow_ltv >= queue.borrow_ltv.min && deposit_input.max_borrow_ltv <= queue.borrow_ltv.max {
                Ok(())
            } else {
                Err(ContractError::InvalidLTV {})
            }
        }
        Err(_) => Err(ContractError::InvalidAsset {}),
    }
}

/// Validate deposit owner
fn validate_deposit_owner(
    api: &dyn cosmwasm_std::Api,
    info: MessageInfo,
    deposit_owner: Option<String>,
) -> Result<Addr, ContractError> {
    match deposit_owner {
        Some(owner) => api.addr_validate(&owner).map_err(|_| ContractError::CustomError {
            val: "Invalid deposit owner address".to_string(),
        }),
        None => Ok(info.sender),
    }
}

/// Update rate assurance for a specific slot/group combination
fn update_rate_assurance(
    storage: &mut dyn Storage,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
    group: &MaxBorrowLTVGroup,
) -> Result<(), ContractError> {
    if !group.total_vault_tokens.is_zero() {
        let base_tokens_for_trillion = calculate_base_tokens(
            Uint128::new(1_000_000_000_000),
            group.total_deposit_tokens,
            group.total_vault_tokens,
        )?;
        
        RATE_ASSURANCE.save(
            storage,
            (asset, max_ltv.to_string(), max_borrow_ltv.to_string()),
            &base_tokens_for_trillion
        )?;
    }
    Ok(())
}

/// Post a deposit tracker entry for base token tracking
// pub fn post_deposit_tracker_entry(
//     deps: DepsMut,
//     _env: Env,
//     _info: MessageInfo,
//     asset: String,
//     max_ltv: Decimal,
//     max_borrow_ltv: Decimal,
// ) -> Result<Response, ContractError> {
//     // Only owner or CDP contract can post tracker entries
//     // if info.sender != config.owner && info.sender != config.cdp_contract {
//     //     return Err(ContractError::Unauthorized {});
//     // }

//     // Call the base token tracking function
//     // Load the group first
//     let queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;
//     if let Some(slot) = queue.slots.iter().find(|s| s.ltv == max_ltv) {
//         if let Some(group) = slot.deposit_groups.iter().find(|g| g.max_borrow_ltv == max_borrow_ltv) {
//             update_rate_assurance(deps.storage, asset.clone(), max_ltv, max_borrow_ltv, group)?;
//         }
//     }

//     Ok(Response::new()
//         .add_attributes(vec![
//             attr("method", "post_deposit_tracker_entry"),
//             attr("asset", asset),
//             attr("max_ltv", max_ltv.to_string()),
//             attr("max_borrow_ltv", max_borrow_ltv.to_string()),
//         ]))
// }

/// Rate assurance
/// Ensures that the conversion rate is static for deposits & withdrawals
pub fn execute_rate_assurance(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
) -> Result<Response, ContractError> {
    //Error if not the contract calling
    if _info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    //Load queue for the asset
    let queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    
    //Find the specific slot (by max_ltv) and group (by max_borrow_ltv)
    let slot = queue.slots.iter()
        .find(|s| s.ltv == max_ltv)
        .ok_or_else(|| ContractError::CustomError {
            val: "Slot not found".to_string(),
        })?;
    
    let group = slot.deposit_groups.iter()
        .find(|g| g.max_borrow_ltv == max_borrow_ltv)
        .ok_or_else(|| ContractError::CustomError {
            val: "Group not found".to_string(),
        })?;

    // Load last rate
    let last_rate = RATE_ASSURANCE
        .may_load(deps.storage, (asset.clone(), max_ltv.to_string(), max_borrow_ltv.to_string()))?;

    if let Some(last_rate) = last_rate {
        let current_rate = calculate_base_tokens(
            Uint128::new(1_000_000_000_000),
            group.total_deposit_tokens,
            group.total_vault_tokens,
        )?;

        if !(current_rate + Uint128::one() >= last_rate) {
            return Err(ContractError::CustomError { 
                val: format!("Rate assurance failed for asset {} (max_ltv: {}, max_borrow_ltv: {}). Previous rate: {:?}, current rate: {:?}", 
                    asset, max_ltv, max_borrow_ltv, last_rate, current_rate) 
            });
        }
    }

    Ok(Response::new())
}

/// Reduce dispersals (active and pending) to fulfill bad debt
fn reduce_dispersals(
    storage: &mut dyn Storage,
    asset: String,
    mut needed_amount: Uint128,
) -> Result<Uint128, ContractError> {
    let mut dispersal = match DISPERSAL.may_load(storage, asset.clone())? {
        Some(d) => d,
        None => return Ok(Uint128::zero()),
    };

    let mut fulfilled_amount = Uint128::zero();

    // First take from active dispersal (remaining allocation not yet dispersed)
    if dispersal.active_dispersal.dispersal_start != 0 && !needed_amount.is_zero() {
        let available_in_active = dispersal.total_to_disperse
            .checked_sub(dispersal.active_dispersal.amount_dispersed)
            .unwrap_or(Uint128::zero());
        
        let take_from_active = std::cmp::min(needed_amount, available_in_active);
        
        if !take_from_active.is_zero() {
            dispersal.total_to_disperse -= take_from_active;
            fulfilled_amount += take_from_active;
            needed_amount -= take_from_active;
        }
    }

    // Then take from pending dispersal
    if !dispersal.pending_dispersal.is_zero() && !needed_amount.is_zero() {
        let take_from_pending = std::cmp::min(needed_amount, dispersal.pending_dispersal);
        
        dispersal.pending_dispersal -= take_from_pending;
        fulfilled_amount += take_from_pending;
        needed_amount -= take_from_pending;
    }

    // Save updated dispersal
    DISPERSAL.save(storage, asset, &dispersal)?;
    
    Ok(fulfilled_amount)
}

/// Reduce revenue events to fulfill bad debt, following waterfall order
// fn reduce_revenue_events(
//     storage: &mut dyn Storage,
//     queue: &LTVQueue,
//     asset: String,
//     mut needed_amount: Uint128,
// ) -> Result<Uint128, ContractError> {
//     let mut fulfilled_amount = Uint128::zero();

//     // Sort slots by LTV in descending order (highest first)
//     let mut sorted_slots: Vec<&MaxLTVSlot> = queue.slots.iter().collect();
//     sorted_slots.sort_by(|a, b| b.ltv.cmp(&a.ltv));

//     for slot in sorted_slots {
//         if needed_amount.is_zero() {
//             break;
//         }

//         // Sort groups by max_borrow_ltv in descending order
//         let mut sorted_groups: Vec<&MaxBorrowLTVGroup> = slot.deposit_groups.iter().collect();
//         sorted_groups.sort_by(|a, b| b.max_borrow_ltv.cmp(&a.max_borrow_ltv));

//         for group in sorted_groups {
//             if needed_amount.is_zero() {
//                 break;
//             }

//             let key = (asset.clone(), slot.ltv.to_string(), group.max_borrow_ltv.to_string());
//             let mut events = REVENUE_EVENTS
//                 .may_load(storage, key.clone())?
//                 .unwrap_or_else(Vec::new);

//             for event in events.iter_mut() {
//                 if needed_amount.is_zero() {
//                     break;
//                 }

//                 let take_from_event = std::cmp::min(needed_amount, event.amount_to_be_claimed);
                
//                 if !take_from_event.is_zero() {
//                     event.amount_to_be_claimed -= take_from_event;
//                     fulfilled_amount += take_from_event;
//                     needed_amount -= take_from_event;
//                 }
//             }

//             // Remove fully depleted events
//             events.retain(|e| !e.amount_to_be_claimed.is_zero());
//             REVENUE_EVENTS.save(storage, key, &events)?;
//         }
//     }

//     Ok(fulfilled_amount)
// }

/// Handle bad debt waterfall: dispersals → revenue events
/// Returns (amount_fulfilled_from_revenue, remaining_bad_debt)
fn handle_bad_debt_waterfall(
    storage: &mut dyn Storage,
    _queue: &LTVQueue,
    asset: String,
    bad_debt_amount: Uint128,
) -> Result<(Uint128, Uint128), ContractError> {
    let mut remaining = bad_debt_amount;
    let mut total_fulfilled = Uint128::zero();

    // 1. Take from dispersals first
    let from_dispersals = reduce_dispersals(storage, asset.clone(), remaining)?;
    total_fulfilled += from_dispersals;
    remaining = remaining.checked_sub(from_dispersals).unwrap_or(Uint128::zero());

    // 2. Take from revenue events
    // if !remaining.is_zero() {
    //     let from_events = reduce_revenue_events(storage, queue, asset, remaining)?;
    //     total_fulfilled += from_events;
    //     remaining = remaining.checked_sub(from_events).unwrap_or(Uint128::zero());
    // }
    // We won't do this because:
    // - This is revenue that should've been claimed already in the best case UX
    // - This increases runtime for a small benefit. We need liquidations to be gas efficient & bad debt flow is added to the end of liquidations.

    Ok((total_fulfilled, remaining))
}

/// Query asset price from oracle and convert to CDT amount
fn query_asset_price(
    querier: &QuerierWrapper,
    oracle_contract: Addr,
    asset_info: AssetInfo,
    cdt_denom: String,
    amount: Uint128,
) -> Result<Uint128, ContractError> {
 
    let cdt_info = AssetInfo::NativeToken { 
        denom: cdt_denom.clone()
    };
    let asset_infos = vec![asset_info.clone(), cdt_info.clone()];
    let price_response: Vec<PriceResponse> = querier.query_wasm_smart(
        oracle_contract.to_string(),
        &Oracle_QueryMsg::Prices {
            asset_infos,
            twap_timeframe: 0, // (No TWAPs)
            oracle_time_limit: 0,
        },
    )?;

    // Use PriceResponse helper functions to convert amount to value
    let value = price_response[0].get_value(amount)?; //Base token value

    // Value is already in CDT terms (USD), now we need to get CDT amount
    // Since CDT is also USD par, 1 USD value = 1 CDT
    // We need to query CDT's decimals to convert properly

    // Convert value back to CDT amount using CDT's price and decimals
    let cdt_amount = price_response[1].get_amount(value)?;

    Ok(cdt_amount)
}

/// Create liquidation swap message for slashed deposits
fn create_liquidation_swap_msg(
    storage: &mut dyn Storage,
    querier: &QuerierWrapper,
    env: &Env,
    config: &Config,
    asset_denom: String,
    amount: Uint128,
) -> Result<SubMsg, ContractError> {
    // Query oracle for CDT equivalent (for validation)
    let asset_info = AssetInfo::NativeToken { denom: asset_denom.clone() };
    let _cdt_equivalent = query_asset_price(
        querier,
        config.oracle_contract.clone(),
        asset_info.clone(),
        config.cdt_denom.clone(),
        amount,
    )?;

    // Query current CDT balance and save to SWAP_PROPAGATION
    let cdt_balance: Coin = querier.query_balance(
        env.contract.address.clone(),
        config.cdt_denom.clone(),
    )?;

    SWAP_PROPAGATION.save(storage, &SwapPropagation {
        cdt_balance_before: cdt_balance.amount,
    })?;

    // Create swap message via chain proxy using ExecuteSwaps
    let swap_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.chain_proxy_contract.to_string(),
        msg: to_json_binary(&OsmosisProxy_ExecuteMsg::ExecuteSwaps {
            token_out: config.cdt_denom.clone(),
            max_slippage: Decimal::percent(90), // 90% max slippage
        })?,
        funds: vec![Coin {
            denom: asset_denom,
            amount,
        }],
    });

    Ok(SubMsg::reply_on_success(swap_msg, crate::contract::LIQUIDATION_SWAP_REPLY_ID))
}

/// Update daily TVL tracker with current total deposit tokens
/// Only updates if it's been at least 1 day since last entry AND total has changed
fn update_daily_tvl_tracker(
    storage: &mut dyn Storage,
    env: &Env,
    querier: &QuerierWrapper,
) -> Result<(), ContractError> {
    // Load config to get deposit denomination
    let config = CONFIG.load(storage)?;
    
    // Query contract balance of deposit token
    let balance: Coin = querier.query_balance(
        env.contract.address.clone(),
        config.deposit_denom.denom,
    )?;
    
    let global_total = balance.amount;
    
    // Load existing entries
    let mut entries = DAILY_TVL_TRACKER.may_load(storage)?.unwrap_or_else(Vec::new);
    
    // Check if we should add a new entry
    let should_add = if let Some(last_entry) = entries.last() {
        // Check if at least 1 day has passed
        let time_elapsed = env.block.time.seconds().saturating_sub(last_entry.timestamp);
        time_elapsed >= ONE_DAY_SECONDS && last_entry.total_deposit_tokens != global_total
    } else {
        // First entry
        true
    };
    
    if should_add {
        let new_entry = TVLEntry {
            timestamp: env.block.time.seconds(),
            total_deposit_tokens: global_total,
        };
        
        entries.push(new_entry);
        
        // Apply limit
        if entries.len() > TVL_TRACKER_LIMIT {
            entries.drain(0..entries.len() - TVL_TRACKER_LIMIT);
        }
        
        DAILY_TVL_TRACKER.save(storage, &entries)?;
    }
    
    Ok(())
}
