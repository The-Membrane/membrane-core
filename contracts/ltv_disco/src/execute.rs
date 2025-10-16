use cosmwasm_std::{
    attr, to_json_binary, Addr, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, QueryRequest, Response, Storage, SubMsg, Uint128, WasmMsg, WasmQuery
};
use membrane::cdp::{LiquidationStatResponse, QueryMsg as CDP_QueryMsg};
use membrane::ltv_disco::{
    BackingDeposit, BackingDepositInput, BaseTokenTrackingEntry, Config, DecimalMinMax, Dispersal, ActiveDispersal, LTVQueue, MaxBorrowLTVGroup, MaxLTVSlot, ExecuteMsg
};
use membrane::math::{decimal_division, decimal_multiplication};
use membrane::types::{Basket, DepositDenom};
use membrane::stability_pool_vault::{calculate_base_tokens, calculate_vault_tokens};
use membrane::transmuter::{ExecuteMsg as Transmuter_ExecuteMsg, QueryMsg as Transmuter_QueryMsg};
use membrane::cdp::ExecuteMsg as CDP_ExecuteMsg;

use crate::error::ContractError;
use crate::state::{BadDebtPropagation, BAD_DEBT_PROPAGATION, BASE_TOKEN_TRACKING, CONFIG, DISPERSAL, LTV_QUEUES, PENDING_BAD_DEBT};
use crate::contract::TRANSMUTER_REPLY_ID;

const MAX_LIMIT: u32 = 32;
const BASE_TOKEN_TRACKING_LIMIT: usize = 100; // Limit for base token tracking vectors

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

    // Remove empty deposit groups
    queue.slots.iter_mut().for_each(|slot| {
        slot.deposit_groups.retain(|group| !group.backing_deposits.is_empty());
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

    let mut deposit_id = queue.current_deposit_id;
    // Check if user already has a deposit in this group and add to it instead of creating new
    if let Some(existing_deposit) = group.backing_deposits.iter_mut().find(|d| d.user == valid_owner_addr) {
        // Add to existing deposit
        existing_deposit.vault_tokens += vault_tokens;
        //Set deposit id for attributes
        deposit_id = existing_deposit.id;
    } else {

        // Validate deposit amount
        if info.funds[0].amount < config.minimum_deposit {
            return Err(ContractError::InvalidDepositAmount {});
        }

        // Create new backing deposit
        let deposit = BackingDeposit {
            user: valid_owner_addr.clone(),
            id: deposit_id.clone(),
            vault_tokens: vault_tokens.clone(),
            max_borrow_ltv: deposit_input.max_borrow_ltv,
            wait_end: Some(env.block.time.plus_seconds(config.waiting_period).seconds()),
        };

        // Add deposit to group
        group.backing_deposits.push(deposit.clone());

        // Update queue
        queue.current_deposit_id += Uint128::new(1u128);

    }


    //Must do this before updating totals for Rate Assurance checks
    add_base_token_tracking_entry(deps.storage, env.clone(), deposit_input.asset.clone(), deposit_input.ltv, deposit_input.max_borrow_ltv)?;

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

    // Update slot totals, just for easier global tracking
    slot.total_deposit_tokens += deposit_amount;
    // slot.total_vault_tokens += deposit.vault_tokens; //No need to update this bc VTS are per group

    // Update queue
    slot.deposit_groups[group_index] = group.clone();
    queue.slots[slot_index] = slot;

    LTV_QUEUES.save(deps.storage, deposit_input.asset.clone(), &queue)?;

    // Track base token amounts


    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "submit_deposit"),
            attr("deposit_owner", valid_owner_addr.to_string()),
            attr("asset", deposit_input.asset),
            attr("ltv", deposit_input.ltv.to_string()),
            attr("max_borrow_ltv", deposit_input.max_borrow_ltv.to_string()),
            attr("deposit_id", deposit_id.to_string()),
            attr("amount", deposit_amount.to_string()),
            attr("vault_tokens", vault_tokens.to_string()),
            attr("action", "created_new"),
        ]))
}

/// Withdraw a backing deposit
pub fn withdraw_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    deposit_id: Uint128,
    asset: String,
    amount: Option<Uint128>, //Amount of base tokens to withdraw
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut queue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    
    let deposit = read_deposit(deps.storage, deposit_id, queue.clone())?;

    // Only owner can withdraw
    if deposit.user != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    //Get Slot
    let slot_index = find_deposit_slot_index(&queue.slots, deposit_id)?;
    let mut slot = queue.slots[slot_index].clone();
    //Get Group
    let group_index = find_or_create_borrow_group(&mut slot, deposit.max_borrow_ltv, false)?;
    let mut group = slot.deposit_groups[group_index].clone();

    //Calculate base tokens to withdraw
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

    // Update the deposit in the appropriate Group
    if let Some(deposit_index) = group.backing_deposits.iter().position(|d| d.id == deposit_id) {

        // Update group totals
        group.total_deposit_tokens -= base_tokens_to_withdraw;
        group.total_vault_tokens -= withdraw_vault_tokens;

        // Remove or update deposit
        if withdraw_vault_tokens == deposit.vault_tokens {
            // Remove deposit
            group.backing_deposits.remove(deposit_index);
        } else {
            // Update deposit
            group.backing_deposits[deposit_index].vault_tokens -= withdraw_vault_tokens;

            // Calculate remaining base tokens
            let remaining_base_tokens = calculate_base_tokens(
                group.backing_deposits[deposit_index].vault_tokens,
                group.total_deposit_tokens,
                group.total_vault_tokens,
            )?;

            // Validate withdrawal amount
            if remaining_base_tokens < config.minimum_deposit {
                return Err(ContractError::InvalidWithdrawal {
                    minimum: config.minimum_deposit,
                });
            }
        }
    } else {
        return Err(ContractError::DepositNotFound {});
    }
    

    // Update slot totals
    slot.total_deposit_tokens -= base_tokens_to_withdraw;
    // Update queue
    slot.deposit_groups[group_index] = group.clone();
    queue.slots[slot_index] = slot.clone();
    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    // Track base token amounts
    add_base_token_tracking_entry(deps.storage, env.clone(), asset.clone(), slot.ltv.clone(), deposit.max_borrow_ltv)?;

    // Send tokens back to user
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

    //Add rate assurance callback msg if remaining tokens are non-zero
    if !group.total_deposit_tokens.is_zero() && !group.total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance {
                asset: asset.clone(),
                max_ltv: slot.ltv,
                max_borrow_ltv: deposit.max_borrow_ltv,
            })?,
            funds: vec![],
        }));
    }

    Ok(Response::new()
        .add_messages(msgs)
        .add_attributes(vec![
            attr("method", "withdraw_deposit"),
            attr("asset", asset),
            attr("deposit_id", deposit_id.to_string()),
            attr("vault_tokens", withdraw_vault_tokens.to_string()),
            attr("base_tokens", base_tokens_to_withdraw.to_string()),
        ]))
}

/// Add bad debt to an LTV queue (CDP contract only)
pub fn add_bad_debt(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
    mut amount: Uint128,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut msgs: Vec<SubMsg> = vec![];

    // Only CDP contract can add bad debt
    if info.sender != config.cdp_contract {
        return Err(ContractError::Unauthorized {});
    }
    /////if the deposit denom is the Transmuter's vault token///////
    if let Some(vault_info) = config.deposit_denom.vault_info.clone(){
        //1) Query how many vault tokens is the bad debt amount worth in the vault contract
        let bad_debt_as_vault_tokens = deps.querier.query_wasm_smart::<Uint128>(
            vault_info.vault_contract.clone(),
            &Transmuter_QueryMsg::DepositTokenConversion { deposit_token_amount: amount },
        )?;
        //2) Update amount to denominate as vault tokens
        amount = bad_debt_as_vault_tokens;
        //3) Withdraw the vault tokens from the vault contract
        msgs.push(SubMsg::reply_on_error(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: vault_info.vault_contract.clone(),
            msg: to_json_binary(&Transmuter_ExecuteMsg::ExitVault { 
                recipient: None, //set to none so we don't have to conditionally add the CDP_ExecuteMsg::FulfillBadDebt Msg at the end of this fn
                withdraw_as: Some(vault_info.underlying_token.clone()),
             })?,
            funds: vec![
                Coin {
                    denom: config.deposit_denom.denom.clone(),
                    amount: bad_debt_as_vault_tokens,
                }
            ],
        }), TRANSMUTER_REPLY_ID));
        //We reply on error, and add the errored amount to PENDING_BAD_DEBT using BAD_DEBT_PROPAGATION data 
        BAD_DEBT_PROPAGATION.save(deps.storage, &BadDebtPropagation {
            asset: asset.clone(),
            amount: amount,
        })?;
    }

    //Bad debt is sent per asset
    let mut queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;

    // Apply bad debt waterfall from highest LTV to lowest
    let mut remaining_bad_debt = amount;

    // Sort slots by LTV in descending order
    let mut sorted_slots: Vec<(usize, MaxLTVSlot)> = queue.slots
        .iter()
        .enumerate()
        .map(|(i, slot)| (i, slot.clone()))
        .collect();
    sorted_slots.sort_by(|a, b| b.1.ltv.cmp(&a.1.ltv));

    for (slot_index, mut slot) in sorted_slots {
        if remaining_bad_debt.is_zero() {
            break;
        }

        // Sort groups by max_borrow_ltv in descending order
        slot.deposit_groups.sort_by(|a, b| b.max_borrow_ltv.cmp(&a.max_borrow_ltv));

        for group in &mut slot.deposit_groups {
            if remaining_bad_debt.is_zero() {
                break;
            }

            // Calculate bad debt for this group
            let group_bad_debt = std::cmp::min(remaining_bad_debt, group.total_deposit_tokens);

            // Update Group
            group.total_deposit_tokens -= group_bad_debt;
            slot.bad_debt += group_bad_debt;
            remaining_bad_debt -= group_bad_debt;
        }

        
        queue.slots[slot_index] = slot;
    }

    //Calc bad debt fulfilled amount 
    let bad_debt_fulfilled_amount = match amount.checked_sub(remaining_bad_debt){
        Ok(amount) => amount,
        Err(_) => return Err(ContractError::CustomError { val: "Bad debt subtraction underflow (should be impossible here)".to_string() }),
    };

    // Send bad debt to CDP contract
    if !bad_debt_fulfilled_amount.is_zero() {
        //If its a vault token
        if let Some(vault_info) = config.deposit_denom.vault_info.clone(){
            //Convert the fulfilled amount to base tokens through the query
            let bad_debt_fulfilled_amount_as_base_tokens = deps.querier.query_wasm_smart::<Uint128>(
                vault_info.vault_contract.clone(),
                &Transmuter_QueryMsg::VaultTokenUnderlying { vault_token_amount: bad_debt_fulfilled_amount.clone() },
            )?;

            //Add msg
            msgs.push(SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.cdp_contract.to_string(),
                msg: to_json_binary(&CDP_ExecuteMsg::FulfillBadDebt {})?,
                funds: vec![
                    Coin {
                        denom: vault_info.underlying_token.clone(),
                        amount: bad_debt_fulfilled_amount_as_base_tokens,
                    }
                ],
            })));
        } else {

            //Add msg
            msgs.push(SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.cdp_contract.to_string(),
                msg: to_json_binary(&CDP_ExecuteMsg::FulfillBadDebt {})?,
                funds: vec![
                    Coin {
                        denom: config.deposit_denom.denom.clone(),
                        amount: bad_debt_fulfilled_amount,
                    }
                ],
            })));
        }
    }

    // Save queue
    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    Ok(Response::new()
        .add_submessages(msgs)
        .add_attributes(vec![
            attr("method", "add_bad_debt"),
            attr("asset", asset),
            attr("bad_debt_fulfilled_amount", bad_debt_fulfilled_amount.to_string()),
        ]))
}

/// Add revenue to an asset's LTV queue.
/// 
pub fn add_revenue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset: String,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Validate that only the configured deposit denom is sent
    if info.funds.len() != 1 || info.funds[0].denom != config.deposit_denom.denom {
        return Err(ContractError::CustomError {
            val: "Invalid deposit denomination".to_string(),
        });
    }

    let mut revenue_amount = info.funds[0].amount;
    let mut queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;

    let response = Response::new();

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

    //Calculate portion to add to dispersal
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
    // Distribute revenue based on weighted pro-rata system
    distribute_revenue(&mut queue, revenue_amount);

    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;
    Ok(response
        .add_attributes(vec![
            attr("method", "add_revenue"),
            attr("asset", asset),
            attr("amount", revenue_amount.to_string()),
        ]))
}


/// Update contract configuration
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    cdp_contract: Option<String>,
    deposit_denom: Option<DepositDenom>,
    minimum_deposit: Option<Uint128>,
    waiting_period: Option<u64>,
    percent_to_disperse: Option<Decimal>,
    dispersal_window: Option<u64>,
    activation_window: Option<u64>
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

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "update_config"),
            attr("config", format!("{:?}", config)),
        ]))
}

/// Activate dispersal for an asset (CDP contract only)
/// REMOVE: Since we're adding liquidation history state, we will query and check the history in the DisperseRevenue call to determine if dispersal should be active if it isn't already
pub fn activate_dispersal(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    dispersal_window: u64,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Only CDP contract can activate dispersal
    if info.sender != config.cdp_contract {
        return Err(ContractError::Unauthorized {});
    }

    // Validate dispersal window (must be > 0)
    if dispersal_window == 0 {
        return Err(ContractError::InvalidDispersalWindow {});
    }

    // Check if dispersal already exists
    if DISPERSAL.has(deps.storage, asset.clone()) {
        return Err(ContractError::CustomError {
            val: "Dispersal already active for this asset".to_string(),
        });
    }

    // Create new dispersal
    let dispersal = Dispersal {
        total_to_disperse: Uint128::zero(),
        dispersal_window,
        active_dispersal: ActiveDispersal {
            dispersal_start: env.block.time.seconds(),
            amount_dispersed: Uint128::zero(),
        },
        pending_dispersal: Uint128::zero()
    };

    DISPERSAL.save(deps.storage, asset.clone(), &dispersal)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "activate_dispersal"),
            attr("asset", asset),
            attr("dispersal_window", dispersal_window.to_string()),
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

    // Load queue and distribute the dispersed amount
    let mut queue: LTVQueue = LTV_QUEUES.load(deps.storage, asset.clone())?;
    distribute_revenue(&mut queue, final_disperse_amount);
    LTV_QUEUES.save(deps.storage, asset.clone(), &queue)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "disperse_revenue"),
            attr("asset", asset),
            attr("elapsed_hours", elapsed_hours.to_string()),
            attr("disperse_amount", final_disperse_amount.to_string()),
        ]))
}

/// Retry failed bad debt, only necessary for the Transmuter's exit failures.
/// Pull from the pending bad debt map & attempt to withdraw it from the Transmuter & send to the CDP contract as a FulfillBadDebt Msg.
/// If the withdrawal errors, we don't update the pending bad debt map.
/// If it succeeds, we update the pending bad debt map.
pub fn retry_failed_bad_debt(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    asset: String,
) -> Result<Response, ContractError> {
    let mut msgs: Vec<SubMsg> = vec![];
    let config = CONFIG.load(deps.storage)?;
    //Load the pending bad debt
    let pending_bad_debt = PENDING_BAD_DEBT.load(deps.storage, asset.clone())?;
    //Save the pending asset to the BAD_DEBT_PROPAGATION
    BAD_DEBT_PROPAGATION.save(deps.storage, &BadDebtPropagation {
        asset: asset.clone(),
        amount: pending_bad_debt,
    })?;

    //Set the denom
    let withdraw_as_denom = config.deposit_denom.vault_info.clone().unwrap().underlying_token.clone();

    //Attempt to withdraw the pending bad debt from the Transmuter
    msgs.push(SubMsg::reply_on_success(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.deposit_denom.vault_info.clone().unwrap().vault_contract.clone(),
        msg: to_json_binary(&Transmuter_ExecuteMsg::ExitVault { 
            recipient: None, //set to none so we don't have to conditionally add the CDP_ExecuteMsg::FulfillBadDebt Msg at the end of this fn
            withdraw_as: Some(withdraw_as_denom.clone()),
         })?,
        funds: vec![
            Coin {
                denom: config.deposit_denom.denom.clone(),
                amount: pending_bad_debt,
            }
        ],
    }), TRANSMUTER_REPLY_ID));

    //convert the pending bad debt to the underlying token
    let pending_bad_debt_as_underlying = deps.querier.query_wasm_smart::<Uint128>(
        config.deposit_denom.vault_info.clone().unwrap().vault_contract.clone(),
        &Transmuter_QueryMsg::VaultTokenUnderlying { vault_token_amount: pending_bad_debt },
    )?;

    //Add the CDP_ExecuteMsg::FulfillBadDebt Msg 
    msgs.push(SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.cdp_contract.to_string(),
        msg: to_json_binary(&CDP_ExecuteMsg::FulfillBadDebt {})?,
        funds: vec![
            Coin {
                denom: withdraw_as_denom,
                amount: pending_bad_debt_as_underlying,
            }
        ],
    })));

    Ok(Response::new()
        .add_submessages(msgs)
        .add_attributes(vec![
            attr("method", "retry_failed_bad_debt"),
            attr("asset", asset),
        ]))
}


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
            backing_deposits: Vec::new(),
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

/// Find the slot index containing a specific deposit
fn find_deposit_slot_index(slots: &[MaxLTVSlot], deposit_id: Uint128) -> Result<usize, ContractError> {
    slots.iter()
        .position(|slot| slot.deposit_groups.iter().any(|group| group.backing_deposits.iter().any(|d| d.id == deposit_id)))
        .ok_or_else(|| ContractError::DepositNotFound {})
}

/// Read a deposit by ID
fn read_deposit(_deps: &dyn Storage, deposit_id: Uint128, queue: LTVQueue) -> Result<BackingDeposit, ContractError> {
    for slot in queue.slots {
        for group in slot.deposit_groups {
            if let Some(deposit) = group.backing_deposits.into_iter().find(|d| d.id == deposit_id) {
                return Ok(deposit);
            }
        }
    }
    Err(ContractError::DepositNotFound {})
}

/// Read deposits by user
fn read_deposits_by_user(
    _deps: &dyn Storage,
    queue: LTVQueue,
    user: Addr,
    limit: Option<u32>,
    start_after: Option<Uint128>,
) -> Result<Vec<BackingDeposit>, ContractError> {
    let mut deposits = Vec::new();
    let limit = limit.unwrap_or(MAX_LIMIT) as usize;
    let start = start_after.unwrap_or_else(Uint128::zero);

    for slot in queue.slots {
        for group in slot.deposit_groups {
            deposits.extend(
                group.backing_deposits
                    .into_iter()
                    .filter(|deposit| deposit.id > start)
                    .filter(|deposit| deposit.user == user)
                    .collect::<Vec<_>>(),
            );
        }
    }

    deposits.truncate(limit);
    Ok(deposits)
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

/// Distribute revenue based on weighted pro-rata system
fn distribute_revenue(queue: &mut LTVQueue, revenue_amount: Uint128) {
    // Calculate total weight (sum of LTV * total_deposit_tokens for each slot)
    let total_weight: Uint128 = queue.slots
        .iter()
        .map(|slot| {
            slot.ltv * slot.total_deposit_tokens
        })
        .sum();

    if total_weight.is_zero() {
        return;
    }

    // Distribute revenue to each slot
    for slot in &mut queue.slots {
        if slot.total_deposit_tokens.is_zero() {
            continue;
        }

        let slot_weight = slot.ltv * slot.total_deposit_tokens;
        let slot_revenue = if total_weight.is_zero() {
            Uint128::zero()
        } else {
            // Use proper decimal arithmetic to avoid integer division issues
            let weight_ratio = Decimal::from_ratio(slot_weight.u128(), total_weight.u128());
            revenue_amount * weight_ratio
        };

        // Split slot revenue pro-rata among deposits based on maxBorrowLTV
        distribute_slot_revenue(slot, slot_revenue);
    }
}

/// Distribute revenue within a slot based on maxBorrowLTV weights
fn distribute_slot_revenue(slot: &mut MaxLTVSlot, slot_revenue: Uint128) {
    if slot.deposit_groups.is_empty() {
        return;
    }

    // Calculate total maxBorrowLTV weight for this slot
    let total_borrow_weight: Uint128 = slot.deposit_groups
        .iter()
        .map(|group| {
            group.max_borrow_ltv * group.total_vault_tokens
        })
        .sum();

    if total_borrow_weight.is_zero() {
        return;
    }

    // Distribute revenue to each group based on their maxBorrowLTV weight
    for group in &mut slot.deposit_groups {
        let group_weight = group.max_borrow_ltv * group.total_vault_tokens;
        let group_revenue = if total_borrow_weight.is_zero() {
            Uint128::zero()
        } else {
            // Use proper decimal arithmetic to avoid integer division issues
            let weight_ratio = Decimal::from_ratio(group_weight.u128(), total_borrow_weight.u128());
            slot_revenue * weight_ratio
        };

        // Add revenue to group's total deposit tokens (this increases the value of all vault tokens in the group)
        group.total_deposit_tokens += group_revenue;
        slot.total_deposit_tokens += group_revenue;
    }
}

/// Track base token amounts for vault tokens for a specific slot and group
/// This function calculates and stores underlying base token amounts for 1,000,000 vault tokens
/// Stores data per (asset, max LTV, max borrow LTV) combination
fn add_base_token_tracking_entry(
    storage: &mut dyn Storage,
    env: Env,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
) -> Result<(), ContractError> {
    let timestamp = env.block.time.seconds();
    
    // Load the queue to find the specific slot and group
    let queue: LTVQueue = LTV_QUEUES.load(storage, asset.clone())?;
    
    // Find the specific slot and group
    if let Some(slot) = queue.slots.iter().find(|s| s.ltv == max_ltv) {
        if let Some(group) = slot.deposit_groups.iter().find(|g| g.max_borrow_ltv == max_borrow_ltv) {
            if !group.total_vault_tokens.is_zero() {
                // Calculate base tokens for 1,000,000,000,000 vault tokens
                let base_tokens_for_million = calculate_base_tokens(
                    Uint128::new(1_000_000_000_000),
                    group.total_deposit_tokens,
                    group.total_vault_tokens,
                )?;
                
                // Create tracking entry
                let tracking_entry = BaseTokenTrackingEntry {
                    timestamp,
                    base_token_amount: base_tokens_for_million,
                };
                
                // Load existing entries for this (asset, max LTV, max borrow LTV) combination
                let mut existing_entries = BASE_TOKEN_TRACKING
                    .may_load(storage, (asset.clone(), max_ltv.to_string(), max_borrow_ltv.to_string()))?
                    .unwrap_or_else(Vec::new);
                

                //If the new entry is the same base token amount as the last entry, don't add it
                if existing_entries.len() > 0 && existing_entries.last().unwrap().base_token_amount == tracking_entry.base_token_amount {
                    return Ok(());
                }
                
                // Add new entry
                existing_entries.push(tracking_entry);
                
                // Apply size limit
                if existing_entries.len() > BASE_TOKEN_TRACKING_LIMIT {
                    existing_entries.drain(0..existing_entries.len() - BASE_TOKEN_TRACKING_LIMIT);
                }
                
                // Save updated entries
                BASE_TOKEN_TRACKING.save(
                    storage, 
                    (asset, max_ltv.to_string(), max_borrow_ltv.to_string()), 
                    &existing_entries
                )?;
            }
        }
    }
    
    Ok(())
}

/// Post a deposit tracker entry for base token tracking
pub fn post_deposit_tracker_entry(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset: String,
    max_ltv: Decimal,
    max_borrow_ltv: Decimal,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    // Only owner or CDP contract can post tracker entries
    // if info.sender != config.owner && info.sender != config.cdp_contract {
    //     return Err(ContractError::Unauthorized {});
    // }

    // Call the base token tracking function
    add_base_token_tracking_entry(deps.storage, env, asset.clone(), max_ltv, max_borrow_ltv)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "post_deposit_tracker_entry"),
            attr("asset", asset),
            attr("max_ltv", max_ltv.to_string()),
            attr("max_borrow_ltv", max_borrow_ltv.to_string()),
        ]))
}

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

    //Get last entry from BASE_TOKEN_TRACKING for this (asset, max_ltv, max_borrow_ltv)
    let tracking_entries = BASE_TOKEN_TRACKING
        .may_load(deps.storage, (asset.clone(), max_ltv.to_string(), max_borrow_ltv.to_string()))?
        .unwrap_or_else(Vec::new);

    if tracking_entries.is_empty() {
        // If no previous entries, this is the first deposit - allow it
        return Ok(Response::new());
    }

    let last_entry = tracking_entries.last().unwrap();

    //Calculate current rate: btokens_per_one = calculate_base_tokens(1_000_000_000_000, group.total_deposit_tokens, group.total_vault_tokens)
    let current_btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000),
        group.total_deposit_tokens,
        group.total_vault_tokens,
    )?;

    //Compare with previous entry's base_token_amount, allowing +/- tolerance
    if !(current_btokens_per_one + Uint128::one() >= last_entry.base_token_amount) {
        return Err(ContractError::CustomError { 
            val: format!("Rate assurance failed for asset {} (max_ltv: {}, max_borrow_ltv: {}). Previous rate: {:?}, current rate: {:?}", 
                asset, max_ltv, max_borrow_ltv, last_entry.base_token_amount, current_btokens_per_one) 
        });
    }

    Ok(Response::new())
}
