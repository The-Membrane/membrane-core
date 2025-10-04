use cosmwasm_std::{
    attr, coin, entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Order, QuerierWrapper, Response, StdError, StdResult, Storage, Uint128
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;

use membrane::helpers::get_contract_balances;
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};
use membrane::stability_pool_vault::{calculate_base_tokens, calculate_vault_tokens};
use membrane::tokenfactory::{burn_msg, create_denom_msg, mint_msg};
use membrane::transmuter::{
    AssetPair, Config, ExecuteMsg, InstantiateMsg, QueryMsg, TransmuteHistoryResponse, SwapRecord,
    VaultInfoResponse, VolumeHistoryResponse, VolumeWindow, VolumeWindowResponse,
};
use membrane::types::AssetInfo;

use crate::error::ContractError;
use crate::state::{
    append_transmute_snapshot, append_volume_window, apply_volume_update, history_slice,
    history_total, init_history, new_volume_window, CONFIG, TRANSMUTE_HISTORY, VOLUME_HISTORY,
    VOLUME_WINDOW, VAULT_TOKEN_SUPPLY, TransmuteSnapshot,
};

const CONTRACT_NAME: &str = "membrane-transmuter";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = msg.clone()
        .owner
        .map(|o| deps.api.addr_validate(&o))
        .transpose()?;
    let owner = owner.unwrap_or_else(|| info.sender.clone());

    validate_asset_pair(&msg.deposit_pair)?;

    if msg.swap_history_cap == 0 {
        return Err(ContractError::Validation(
            "swap_history_cap must be greater than zero".into(),
        ));
    }
    if msg.volume_history_cap == 0 {
        return Err(ContractError::Validation(
            "volume_history_cap must be greater than zero".into(),
        ));
    }
    if msg.asset_a_to_b_rate.is_zero() {
        return Err(ContractError::Validation(
            "asset_a_to_b_rate must be greater than zero".into(),
        ));
    }
    if msg.target_ratio > Decimal::one() {
        return Err(ContractError::Validation(
            "target_ratio must be less than or equal to 1".into(),
        ));
    }

    let vault_token = format!(
        "factory/{}/{}",
        env.contract.address,
        msg.vault_subdenom
    );

    let config = Config {
        owner: owner.clone(),
        tokenfactory_contract: msg.clone().tokenfactory_contract,
        vault_token: vault_token.clone(),
        deposit_pair: msg.clone().deposit_pair,
        composition_leeway: msg.clone().composition_leeway,
        asset_a_to_b_rate: msg.clone().asset_a_to_b_rate,
        target_ratio: msg.clone().target_ratio,
        swap_history_cap: msg.clone().swap_history_cap,
        volume_history_cap: msg.clone().volume_history_cap,
    };

    CONFIG.save(deps.storage, &config)?;
    VAULT_TOKEN_SUPPLY.save(deps.storage, &Uint128::zero())?;
    init_history(deps.storage)?;
    VOLUME_WINDOW.save(deps.storage, &new_volume_window(env.block.time))?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let create_msg = create_denom_msg(
        msg.tokenfactory_contract,
        env.contract.address.as_str(),
        &msg.vault_subdenom,
    )?;

    Ok(Response::new()
        .add_message(create_msg)
        .add_attributes(vec![
            attr("action", "instantiate"),
            attr("owner", owner.as_str()),
            attr("vault_token", vault_token),
        ]))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig {
            owner,
            deposit_pair,
            composition_leeway,
            asset_a_to_b_rate,
            target_ratio,
            tokenfactory_contract,
            swap_history_cap,
            volume_history_cap,
        } => execute_update_config(
            deps,
            env,
            info,
            owner,
            deposit_pair,
            composition_leeway,
            asset_a_to_b_rate,
            target_ratio,
            tokenfactory_contract,
            swap_history_cap,
            volume_history_cap,
        ),
        ExecuteMsg::EnterVault { recipient } => execute_enter_vault(deps, env, info, recipient),
        ExecuteMsg::DepositFee {} => execute_deposit_fee(deps, env, info),
        ExecuteMsg::ExitVault {
            recipient,
            withdraw_as,
        } => execute_exit_vault(deps, env, info, recipient, withdraw_as),
        ExecuteMsg::Transmute { recipient } => execute_transmute(deps, env, info, recipient),
        ExecuteMsg::UpdateVolumeWindow {} => execute_update_volume_window(deps, env),
    }
}

fn execute_update_config(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: Option<String>,
    deposit_pair: Option<AssetPair>,
    composition_leeway: Option<Decimal>,
    asset_a_to_b_rate: Option<Decimal>,
    target_ratio: Option<Decimal>,
    tokenfactory_contract: Option<Addr>,
    swap_history_cap: Option<u32>,
    volume_history_cap: Option<u32>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    ensure_owner(&config, &info.sender)?;

    if let Some(owner_str) = owner {
        config.owner = deps.api.addr_validate(&owner_str)?;
    }

    if let Some(pair) = deposit_pair {
        validate_asset_pair(&pair)?;
        config.deposit_pair = pair;
    }

    if let Some(leeway) = composition_leeway {
        if leeway > Decimal::one() {
            return Err(ContractError::Validation(
                "composition_leeway must be less than or equal to 1".into(),
            ))
        }
        config.composition_leeway = leeway;
    }

    if let Some(rate) = asset_a_to_b_rate {
        if rate.is_zero() {
            return Err(ContractError::Validation(
                "asset_a_to_b_rate must be greater than zero".into(),
            ));
        }
        config.asset_a_to_b_rate = rate;
    }

    if let Some(target) = target_ratio {
        if target > Decimal::one() {
            return Err(ContractError::Validation(
                "target ratio must be less than or equal to 1".into(),
            ));
        }
        config.target_ratio = target;
    }

    if let Some(tf_addr) = tokenfactory_contract {
        config.tokenfactory_contract = Some(tf_addr);
    }

    if let Some(cap) = swap_history_cap {
        if cap == 0 {
            return Err(ContractError::Validation(
                "swap_history_cap must be greater than zero".into(),
            ));
        }
        config.swap_history_cap = cap;
    }

    if let Some(cap) = volume_history_cap {
        if cap == 0 {
            return Err(ContractError::Validation(
                "volume_history_cap must be greater than zero".into(),
            ));
        }
        config.volume_history_cap = cap;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

fn execute_enter_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut total_deposits = get_total_deposit_value(deps.querier, &env, &config)?;
    let mut vault_supply = VAULT_TOKEN_SUPPLY.load(deps.storage)?;

    //Split the funds into the two assets
    let (deposit_a, deposit_b) = segregate_funds(&config.deposit_pair, &info)?;
    if deposit_a.is_zero() && deposit_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no valid funds provided".into(),
        });
    }

    println!("deposit_a: {:?}", deposit_a);
    println!("deposit_b: {:?}", deposit_b);
    //Ensure the deposit is aligned with the deposit pair
    ensure_deposit_alignment(
        deps.querier,
        &env,
        &config,
        deposit_a,
        deposit_b,
    )?;

    //Calc user deposit value, denominated in asset A
    let user_deposit_value = sum_base_value(deposit_a, deposit_b, config.asset_a_to_b_rate)?;
    if user_deposit_value.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "deposit value is zero".into(),
        });
    }
    println!("user_deposit_value: {:?}", user_deposit_value);
    println!("total_deposits: {:?}", total_deposits);
    println!("vault_supply: {:?}", vault_supply);

    //Calc the amount of vault tokens to mint
    let vault_tokens_to_mint = calculate_vault_tokens(
        user_deposit_value,
         total_deposits - user_deposit_value,
          vault_supply
    )?;

    //Get the recipient address
    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?;
    let recipient_addr = recipient_addr.unwrap_or_else(|| info.sender.clone());

    let mut messages: Vec<CosmosMsg> = vec![];
    //Mint vault tokens to the recipient 
    if !vault_tokens_to_mint.is_zero() {
        let mint = mint_msg(
            config.tokenfactory_contract.clone(),
            env.contract.address.as_str(),
            &config.vault_token,
            vault_tokens_to_mint,
            recipient_addr.as_str(),
        )?
        .into();
        messages.push(mint);
        //Update the total vault supply
        vault_supply = increment_vault_supply(deps.storage, vault_supply, vault_tokens_to_mint)?;
    }
println!("vault_tokens_to_mint: {:?}", vault_tokens_to_mint);
    VAULT_TOKEN_SUPPLY.save(deps.storage, &vault_supply)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "enter_vault"),
            attr("base_amount", user_deposit_value.to_string()),
            attr("vault_tokens", vault_tokens_to_mint.to_string()),
            attr("recipient", recipient_addr.as_str()),
        ]))
}

fn execute_deposit_fee(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    // let mut total_deposits = get_total_deposit_value(deps.querier, &env, &config)?;

    let (deposit_a, deposit_b) = segregate_funds(&config.deposit_pair, &info)?;
    if deposit_a.is_zero() && deposit_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no valid funds provided".into(),
        });
    }

    // let deposit_base = sum_base_value(deposit_a, deposit_b, config.asset_a_to_b_rate)?;
    // total_deposits = total_deposits
    //     .checked_add(deposit_base)
    //     .map_err(|err| ContractError::Std(err.into()))?;

    // VAULT_TOKEN_SUPPLY.save(deps.storage, &total_deposits)?;

    Ok(Response::new().add_attributes(vec![
        attr("action", "deposit_fee"),
        attr("asset_a", deposit_a.to_string()),
        attr("asset_b", deposit_b.to_string()),
    ]))
}

fn execute_exit_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
    withdraw_as: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut total_deposits = get_total_deposit_value(deps.querier, &env, &config)?;
    let mut vault_supply = VAULT_TOKEN_SUPPLY.load(deps.storage)?;

    if vault_supply.is_zero() {
        return Err(ContractError::Validation("no vault tokens in circulation".into()));
    }

    //Get the amount of vault tokens sent
    let vault_tokens = extract_coin_amount(&info, &config.vault_token)?;
    if vault_tokens.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no vault tokens provided".into(),
        });
    }
    if vault_tokens > vault_supply {
        return Err(ContractError::InvalidFunds {
            reason: "vault token amount exceeds supply".into(),
        });
    }

    //Calc withdraw value, denominated in asset A
    let base_amount = calculate_base_tokens(
        vault_tokens,
         total_deposits,
          vault_supply
        )?;

    //Get the balances of the contract
    let balances = current_balances(deps.querier, &env, &config.deposit_pair)?;
    //Calc user share of assets
    let user_share_of_assets = Decimal::from_ratio(vault_tokens, vault_supply);
    //Calc user share of asset A
    let asset_a_share = decimal_multiplication(
        Decimal::from_ratio(balances.0, Uint128::one()),
        user_share_of_assets
    )?.to_uint_floor();
    //Calc user share of asset B
    let asset_b_share = decimal_multiplication(
        Decimal::from_ratio(balances.1, Uint128::one()),
        user_share_of_assets
    )?.to_uint_floor();

    //Get the recipient address
    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?;
    let recipient_addr = recipient_addr.unwrap_or_else(|| info.sender.clone());

    let mut send_coins: Vec<Coin> = vec![];

    //Send the assets to the recipient & convert assets if needed
    match withdraw_as {
        Some(ref denom) if denom == &config.deposit_pair.asset_a => {
            let converted = convert_asset_b_to_a(asset_b_share, config.asset_a_to_b_rate)?;
            let total_a_needed = asset_a_share
                .checked_add(converted)
                .map_err(|err| ContractError::Std(err.into()))?;
            if balances.0 < total_a_needed {
                return Err(ContractError::InsufficientLiquidity(config.deposit_pair.asset_a.clone()));
            }
            if !total_a_needed.is_zero() {
                send_coins.push(coin(total_a_needed.u128(), denom));
            }
        }
        Some(ref denom) if denom == &config.deposit_pair.asset_b => {
            let converted = convert_asset_a_to_b(asset_a_share, config.asset_a_to_b_rate)?;
            let total_b_needed = asset_b_share
                .checked_add(converted)
                .map_err(|err| ContractError::Std(err.into()))?;
            if balances.1 < total_b_needed {
                return Err(ContractError::InsufficientLiquidity(config.deposit_pair.asset_b.clone()));
            }
            if !total_b_needed.is_zero() {
                send_coins.push(coin(total_b_needed.u128(), denom));
            }
        }
        Some(denom) => {
            return Err(ContractError::InvalidAsset { denom });
        }
        None => {
            if !asset_a_share.is_zero() {
                send_coins.push(coin(asset_a_share.u128(), config.deposit_pair.asset_a.clone()));
            }
            if !asset_b_share.is_zero() {
                send_coins.push(coin(asset_b_share.u128(), config.deposit_pair.asset_b.clone()));
            }
        }
    }

    let burn = burn_msg(
        config.tokenfactory_contract.clone(),
        env.contract.address.as_str(),
        &config.vault_token,
        vault_tokens,
        env.contract.address.as_str(),
    )?;

    //Update the total vault supply
    vault_supply = decrement_vault_supply(deps.storage, vault_supply, vault_tokens)?;
    VAULT_TOKEN_SUPPLY.save(deps.storage, &vault_supply)?;

    let mut response = Response::new()
        .add_message(burn)
        .add_attribute("action", "exit_vault")
        .add_attribute("base_amount", base_amount.to_string())
        .add_attribute("vault_tokens", vault_tokens.to_string())
        .add_attribute("recipient", recipient_addr.as_str());

    if !send_coins.is_empty() {
        response = response.add_message(BankMsg::Send {
            to_address: recipient_addr.to_string(),
            amount: send_coins,
        });
    }

    Ok(response)
}

fn execute_transmute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let pair = &config.deposit_pair;
    //Get the recipient address
    let recipient_addr = recipient
        .map(|r| deps.api.addr_validate(&r))
        .transpose()?;
    let recipient_addr = recipient_addr.unwrap_or_else(|| info.sender.clone());

    let (funds_a, funds_b) = segregate_funds(pair, &info)?;
    if funds_a.is_zero() && funds_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no valid funds provided".into(),
        });
    }
    if !funds_a.is_zero() && !funds_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "only one asset may be sent for transmute".into(),
        });
    }

    let (offered_asset, offered_amount, received_asset, received_amount) = if !funds_a.is_zero() {
        let receive_amount = convert_asset_a_to_b(funds_a, config.asset_a_to_b_rate)?;
        ensure_contract_balance(deps.querier, &env, &pair.asset_b, receive_amount)?;
        (
            pair.asset_a.clone(),
            funds_a,
            pair.asset_b.clone(),
            receive_amount,
        )
    } else {
        let receive_amount = convert_asset_b_to_a(funds_b, config.asset_a_to_b_rate)?;
        ensure_contract_balance(deps.querier, &env, &pair.asset_a, receive_amount)?;
        (
            pair.asset_b.clone(),
            funds_b,
            pair.asset_a.clone(),
            receive_amount,
        )
    };

    let mut send_coins: Vec<Coin> = vec![];
    if !received_amount.is_zero() {
        send_coins.push(coin(received_amount.u128(), received_asset.clone()));
    }

    append_transmute_snapshot(
        deps.storage,
        config.swap_history_cap,
        TransmuteSnapshot {
            offered_asset: offered_asset.clone(),
            offered_amount,
            received_asset: received_asset.clone(),
            received_amount,
            block_time: env.block.time,
        },
    )?;

    VOLUME_WINDOW.update(deps.storage, |mut window| -> StdResult<_> {
        apply_volume_update(
            &mut window,
            if offered_asset == pair.asset_a {
                offered_amount
            } else {
                Uint128::zero()
            },
            if received_asset == pair.asset_a {
                received_amount
            } else {
                Uint128::zero()
            },
            if offered_asset == pair.asset_b {
                offered_amount
            } else {
                Uint128::zero()
            },
            if received_asset == pair.asset_b {
                received_amount
            } else {
                Uint128::zero()
            },
        );
        Ok(window)
    })?;

    // println!("window: {:?}", VOLUME_WINDOW.load(deps.storage)?);

    let mut response = Response::new()
        .add_attribute("action", "transmute")
        .add_attribute("offered_asset", offered_asset)
        .add_attribute("offered_amount", offered_amount.to_string())
        .add_attribute("received_asset", received_asset)
        .add_attribute("received_amount", received_amount.to_string())
        .add_attribute("recipient", recipient_addr.as_str());

    if !send_coins.is_empty() {
        response = response.add_message(BankMsg::Send {
            to_address: recipient_addr.to_string(),
            amount: send_coins,
        });
    }

    Ok(response)
}

fn execute_update_volume_window(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let window = VOLUME_WINDOW.load(deps.storage)?;

    append_volume_window(
        deps.storage,
        config.volume_history_cap,
        window,
    )
    .map_err(|err| ContractError::Std(err.into()))?;

    VOLUME_WINDOW.save(deps.storage, &new_volume_window(env.block.time))?;

    Ok(Response::new().add_attribute("action", "update_volume_window"))
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::VaultInfo {} => to_json_binary(&query_vault_info(deps, env)?),
        QueryMsg::TransmuteHistory { start_after, limit } => {
            to_json_binary(&query_swap_history(deps, start_after, limit)?)
        }
        QueryMsg::VolumeHistory { start_after, limit } => {
            to_json_binary(&query_volume_history(deps, start_after, limit)?)
        }
        QueryMsg::VolumeWindow {} => {
            let window = VOLUME_WINDOW.load(deps.storage)?;
            to_json_binary(&VolumeWindowResponse { window })
        }
    }
}

fn query_vault_info(deps: Deps, env: Env) -> StdResult<VaultInfoResponse> {
    let config = CONFIG.load(deps.storage)?;
    let total_deposit_value = get_total_deposit_value(deps.querier, &env, &config).map_err(|err| StdError::GenericErr { msg: format!("Failed to query the contract for the total deposit value") })?;
    let vault_token_supply = VAULT_TOKEN_SUPPLY.load(deps.storage)?;
    let balances = current_balances(deps.querier, &env, &config.deposit_pair)?;

    Ok(VaultInfoResponse {
        total_deposit_value,
        vault_token_supply,
        asset_a_balance: balances.0,
        asset_b_balance: balances.1,
    })
}

fn query_swap_history(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<TransmuteHistoryResponse> {
    let history = TRANSMUTE_HISTORY.load(deps.storage)?;
    let total = history_total(&history);
    let (slice, start_index) = history_slice(&history, start_after, limit);

    let records: Vec<SwapRecord> = slice
        .into_iter()
        .map(|snapshot| SwapRecord {
            offered_asset: snapshot.offered_asset,
            offered_amount: snapshot.offered_amount,
            received_asset: snapshot.received_asset,
            received_amount: snapshot.received_amount,
            block_time: snapshot.block_time,
        })
        .collect();

    let next_start_after = if records.is_empty() {
        None
    } else {
        let last_index = start_index + records.len() - 1;
        if (last_index as u64) + 1 < total {
            Some(last_index as u64)
        } else {
            None
        }
    };

    Ok(TransmuteHistoryResponse {
        records,
        total,
        next_start_after,
    })
}

fn query_volume_history(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<VolumeHistoryResponse> {
    let mut history = VOLUME_HISTORY.load(deps.storage)?;
    let window = VOLUME_WINDOW.load(deps.storage)?;
    //Add current window to the history
    history.push(window);

    let total = history_total(&history);
    let (records, start_index) = history_slice(&history, start_after, limit);

    let next_start_after = if records.is_empty() {
        None
    } else {
        let last_index = start_index + records.len() - 1;
        if (last_index as u64) + 1 < total {
            Some(last_index as u64)
        } else {
            None
        }
    };

    Ok(VolumeHistoryResponse {
        records,
        total,
        next_start_after,
    })
}

fn validate_asset_pair(pair: &AssetPair) -> Result<(), ContractError> {
    if pair.asset_a.is_empty() || pair.asset_b.is_empty() {
        return Err(ContractError::Validation(
            "asset denoms must be non-empty".into(),
        ));
    }
    if pair.asset_a == pair.asset_b {
        return Err(ContractError::Validation(
            "asset A and asset B must differ".into(),
        ));
    }
    Ok(())
}

fn ensure_owner(config: &Config, sender: &Addr) -> Result<(), ContractError> {
    if &config.owner != sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

fn segregate_funds(
    pair: &AssetPair,
    info: &MessageInfo,
) -> Result<(Uint128, Uint128), ContractError> {
    let mut asset_a_total = Uint128::zero();
    let mut asset_b_total = Uint128::zero();

    for fund in &info.funds {
        if fund.denom == pair.asset_a {
            asset_a_total = asset_a_total
                .checked_add(fund.amount)
                .map_err(|err| ContractError::Std(err.into()))?;
        } else if fund.denom == pair.asset_b {
            asset_b_total = asset_b_total
                .checked_add(fund.amount)
                .map_err(|err| ContractError::Std(err.into()))?;
        } else {
            return Err(ContractError::InvalidFunds {
                reason: format!("unsupported denom: {}", fund.denom),
            });
        }
    }

    Ok((asset_a_total, asset_b_total))
}

fn extract_coin_amount(info: &MessageInfo, denom: &str) -> Result<Uint128, ContractError> {
    let mut amount = Uint128::zero();
    for fund in &info.funds {
        if fund.denom == denom {
            amount = amount
                .checked_add(fund.amount)
                .map_err(|err| ContractError::Std(err.into()))?;
        }
    }
    Ok(amount)
}

fn current_balances(
    querier: QuerierWrapper,
    env: &Env,
    pair: &AssetPair,
) -> StdResult<(Uint128, Uint128)> {
    let assets = vec![
        AssetInfo::NativeToken {
            denom: pair.asset_a.clone(),
        },
        AssetInfo::NativeToken {
            denom: pair.asset_b.clone(),
        },
    ];
    let balances = get_contract_balances(querier, env.clone(), assets)?;
    Ok((balances[0], balances[1]))
}

fn sum_base_value(
    asset_a_amount: Uint128,
    asset_b_amount: Uint128,
    rate: Decimal,
) -> Result<Uint128, ContractError> {
    let converted_b_to_a = convert_asset_b_to_a(asset_b_amount, rate)?;
    asset_a_amount
        .checked_add(converted_b_to_a)
        .map_err(|err| ContractError::Std(err.into()))
}

fn convert_asset_a_to_b(amount: Uint128, rate: Decimal) -> Result<Uint128, ContractError> {
    let decimal_amount = Decimal::from_ratio(amount, Uint128::one());
    Ok(decimal_multiplication(rate, decimal_amount)?.to_uint_floor())
}

fn convert_asset_b_to_a(amount: Uint128, rate: Decimal) -> Result<Uint128, ContractError> {
    if rate.is_zero() {
        return Err(ContractError::Validation(
            "asset_a_to_b_rate must be greater than zero".into(),
        ));
    }
    let inv_rate = decimal_division(Decimal::one(), rate)?;
    let decimal_amount = Decimal::from_ratio(amount, Uint128::one());
    Ok(decimal_multiplication(inv_rate, decimal_amount)?.to_uint_floor())
}

fn ensure_deposit_alignment(
    querier: QuerierWrapper,
    env: &Env,
    config: &Config,
    deposit_a: Uint128,
    deposit_b: Uint128,
) -> Result<(), ContractError> {
    if deposit_a.is_zero() && deposit_b.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "no deposit value provided".into(),
        });
    }

    //Get the balances after the deposit
    let balances_after = current_balances(querier, env, &config.deposit_pair)?;
    let pre_a = balances_after
        .0
        .checked_sub(deposit_a)
        .map_err(|err| ContractError::Std(err.into()))?;
    let pre_b = balances_after
        .1
        .checked_sub(deposit_b)
        .map_err(|err| ContractError::Std(err.into()))?;

    //Get the total deposit value before the deposit
    let total_before = sum_base_value(pre_a, pre_b, config.asset_a_to_b_rate)?;
    //Get the user deposit value
    let deposit_value = sum_base_value(deposit_a, deposit_b, config.asset_a_to_b_rate)?;
    if deposit_value.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "deposit value is zero".into(),
        });
    }

    let target = config.target_ratio;
    let leeway = config.composition_leeway;

    //If the total contract value before the deposit is zero,
    //...we need to ensure the user deposit value is within the leeway
    if total_before.is_zero() {
        let deposit_ratio = Decimal::from_ratio(deposit_a, deposit_value);
        ensure_within_leeway(deposit_ratio, target, leeway)?;
        return Ok(());
    }

    //Get the current ratio (pre-deposit)
    let current_ratio = Decimal::from_ratio(pre_a, total_before);

    let new_a = balances_after.0;
    let new_b = balances_after.1;
    //Get the total contract value after the deposit
    let total_after = sum_base_value(new_a, new_b, config.asset_a_to_b_rate)?;
    if total_after.is_zero() {
        return Err(ContractError::InvalidFunds {
            reason: "resulting deposit value is zero".into(),
        });
    }

    //Get the new ratio of Asset A (post-deposit)
    let new_ratio = Decimal::from_ratio(new_a, total_after);

    //Get the difference between the current ratio and the target ratio
    let current_diff = decimal_abs_diff(current_ratio, target)?;
    //Get the difference between the new ratio and the target ratio
    let new_diff = decimal_abs_diff(new_ratio, target)?;


    //Ensure the new ratio is within the leeway
    //or it at least puts the contract composition closer to the target ratio
    match ensure_within_leeway(new_ratio, target, leeway){
        Ok(()) => Ok(()),
        Err(err) => {
            //OR, if the new ratio is outside the leeway,
            //..we error ONLY IF the deposit puts the balance in a worse position
            if new_diff > current_diff {
                return Err(ContractError::Validation(
                    "deposit would move composition away from target".into(),
                ));
            }
            //Otherwise, the deposit is valid bc it moves the composition closer to the target ratio or keeps it static
            Ok(())
        }
    }


}

fn ensure_within_leeway(
    actual: Decimal,
    target: Decimal,
    leeway: Decimal,
) -> Result<(), ContractError> {
    let diff = decimal_abs_diff(actual, target)?;

    if diff > leeway {
        return Err(ContractError::SlippageExceeded {});
    }

    Ok(())
}

fn ensure_contract_balance(
    querier: QuerierWrapper,
    env: &Env,
    denom: &str,
    amount: Uint128,
) -> Result<(), ContractError> {
    if amount.is_zero() {
        return Ok(());
    }
    let balance = querier.query_balance(env.contract.address.clone(), denom.to_string())?.amount;
    if balance < amount {
        return Err(ContractError::InsufficientLiquidity(denom.to_string()));
    }
    Ok(())
}

fn decimal_abs_diff(a: Decimal, b: Decimal) -> Result<Decimal, ContractError> {
    if a >= b {
        decimal_subtraction(a, b).map_err(|err| ContractError::Std(err.into()))
    } else {
        decimal_subtraction(b, a).map_err(|err| ContractError::Std(err.into()))
    }
}

fn get_total_deposit_value(
    querier: QuerierWrapper,
    env: &Env,
    config: &Config,
) -> Result<Uint128, ContractError> {
    let balances = current_balances(querier, env, &config.deposit_pair)?;
    sum_base_value(balances.0, balances.1, config.asset_a_to_b_rate)
}

fn increment_vault_supply(
    storage: &mut dyn Storage,
    current_supply: Uint128,
    amount: Uint128,
) -> Result<Uint128, ContractError> {
    let new_supply = current_supply
        .checked_add(amount)
        .map_err(|err| ContractError::Std(err.into()))?;
    VAULT_TOKEN_SUPPLY.save(storage, &new_supply)?;
    Ok(new_supply)
}

fn decrement_vault_supply(
    storage: &mut dyn Storage,
    current_supply: Uint128,
    amount: Uint128,
) -> Result<Uint128, ContractError> {
    let new_supply = current_supply
        .checked_sub(amount)
        .map_err(|err| ContractError::Std(err.into()))?;
    VAULT_TOKEN_SUPPLY.save(storage, &new_supply)?;
    Ok(new_supply)
}
