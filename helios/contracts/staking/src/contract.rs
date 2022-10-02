use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coin, from_binary, to_binary, Addr, Api, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Uint128, WasmMsg,
};
use cw2::set_contract_version;
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};

use membrane::apollo_router::{Cw20HookMsg as RouterCw20HookMsg, ExecuteMsg as RouterExecuteMsg};
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::staking::{
    ConfigResponse, Cw20HookMsg, ExecuteMsg, FeeEventsResponse, InstantiateMsg, QueryMsg,
    RewardsResponse, StakedResponse, StakerResponse, TotalStakedResponse,
};
use membrane::types::{Asset, AssetInfo, FeeEvent, LiqAsset, StakeDeposit};
use membrane::math::decimal_division;

use crate::error::ContractError;
use crate::state::{Config, Totals, CONFIG, FEE_EVENTS, STAKED, TOTALS};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:staking";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const SECONDS_PER_YEAR: u64 = 31_536_000u64;
const SECONDS_PER_DAY: u64 = 86_400u64;
const DEFAULT_LIMIT: u32 = 32u32;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config: Config;

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            positions_contract: None,
            builders_contract: None,
            osmosis_proxy: None,
            staking_rate: msg.staking_rate.unwrap_or_else(|| Decimal::percent(10)),
            fee_wait_period: msg.fee_wait_period.unwrap_or(3u64),
            unstaking_period: msg.unstaking_period.unwrap_or(3u64),
            mbrn_denom: msg.mbrn_denom,
            dex_router: None,
            max_spread: msg.max_spread,
        };
    } else {
        config = Config {
            owner: info.sender,
            positions_contract: None,
            builders_contract: None,
            osmosis_proxy: None,
            staking_rate: msg.staking_rate.unwrap_or_else(|| Decimal::percent(10)),
            fee_wait_period: msg.fee_wait_period.unwrap_or(3u64),
            unstaking_period: msg.unstaking_period.unwrap_or(3u64),
            mbrn_denom: msg.mbrn_denom,
            dex_router: None,
            max_spread: msg.max_spread,
        };
    }

    let mut attrs = vec![];
    // //Set optional config parameters
    match msg.dex_router {
        Some(dex_router) => {
            config.dex_router = Some(deps.api.addr_validate(&dex_router)?);
            attrs.push(attr("dex_router", dex_router));
        }
        None => {}
    }
    match msg.builders_contract {
        Some(builders_contract) => {
            config.builders_contract = Some(deps.api.addr_validate(&builders_contract)?);
            attrs.push(attr("builders_contract", builders_contract));
        }
        None => {}
    }
    match msg.positions_contract {
        Some(positions_contract) => {
            config.positions_contract = Some(deps.api.addr_validate(&positions_contract)?);
            attrs.push(attr("positions_contract", positions_contract));
        }
        None => {}
    }
    match msg.osmosis_proxy {
        Some(osmosis_proxy) => {
            config.osmosis_proxy = Some(deps.api.addr_validate(&osmosis_proxy)?);
            attrs.push(attr("osmosis_proxy", osmosis_proxy));
        }
        None => {}
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    //Initialize StakeDeposit List
    let vec: Vec<StakeDeposit> = vec![];
    STAKED.save(deps.storage, &vec)?;
    //Initialize stake Totals
    TOTALS.save(
        deps.storage,
        &Totals {
            stakers: Uint128::zero(),
            builders_contract: Uint128::zero(),
        },
    )?;
    //Initialize fee events
    let vec: Vec<FeeEvent> = vec![];
    FEE_EVENTS.save(deps.storage, &vec)?;

    let res = Response::new();

    attrs.push(attr("method", "instantiate"));

    let c = &config.owner.to_string();
    attrs.push(attr("owner", c));

    Ok(res.add_attributes(attrs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Receive(msg) => receive_cw20(deps, env, info, msg),
        ExecuteMsg::UpdateConfig {
            owner,
            mbrn_denom,
            dex_router,
            max_spread,
            builders_contract,
            osmosis_proxy,
            positions_contract,
            staking_rate,
            fee_wait_period,
            unstaking_period,
        } => update_config(
            deps,
            info,
            owner,
            positions_contract,
            builders_contract,
            osmosis_proxy,
            mbrn_denom,
            staking_rate,
            fee_wait_period,
            unstaking_period,
            dex_router,
            max_spread,
        ),
        ExecuteMsg::Stake { user } => stake(deps, env, info, user),
        ExecuteMsg::Unstake { mbrn_amount } => unstake(deps, env, info, mbrn_amount),
        ExecuteMsg::Restake { mbrn_amount } => restake(deps, env, info, mbrn_amount),
        ExecuteMsg::ClaimRewards {
            claim_as_native,
            claim_as_cw20,
            send_to,
            restake,
        } => claim_rewards(
            deps,
            env,
            info,
            claim_as_native,
            claim_as_cw20,
            send_to,
            restake,
        ),
        ExecuteMsg::DepositFee {} => {
            let config = CONFIG.load(deps.storage)?;

            if info.sender != config.positions_contract.unwrap() {
                return Err(ContractError::Unauthorized {});
            }

            //Take fee_assets from sent_assets
            let fee_assets = {
                info.clone()
                    .funds
                    .into_iter()
                    .map(|coin| Asset {
                        info: AssetInfo::NativeToken { denom: coin.denom },
                        amount: coin.amount,
                    })
                    .collect::<Vec<Asset>>()
            };

            deposit_fee(deps, env, info, fee_assets, false)
        }
    }
}

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    positions_contract: Option<String>,
    builders_contract: Option<String>,
    osmosis_proxy: Option<String>,
    mbrn_denom: Option<String>,
    staking_rate: Option<Decimal>,
    fee_wait_period: Option<u64>,
    unstaking_period: Option<u64>,
    dex_router: Option<String>,
    max_spread: Option<Decimal>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut attrs = vec![attr("method", "update_config")];

    //Match Optionals
    match owner {
        Some(owner) => {
            let valid_addr = deps.api.addr_validate(&owner)?;
            config.owner = valid_addr.clone();
            attrs.push(attr("new_owner", valid_addr.to_string()));
        }
        None => {}
    }
    match dex_router {
        Some(dex_router) => {
            let valid_addr = deps.api.addr_validate(&dex_router)?;
            config.dex_router = Some(valid_addr.clone());
            attrs.push(attr("new_dex_router", valid_addr.to_string()));
        }
        None => {}
    }
    match max_spread {
        Some(max_spread) => {
            config.max_spread = Some(max_spread);
            attrs.push(attr("new_max_spread", max_spread.to_string()));
        }
        None => {}
    }
    match staking_rate {
        Some(mut staking_rate) => {
            //Hard code a 20% maximum
            if staking_rate > Decimal::percent(20) {
                staking_rate = Decimal::percent(20);
            }
            config.staking_rate = staking_rate;
            attrs.push(attr("new_staking_rate", staking_rate.to_string()));
        }
        None => {}
    }
    match unstaking_period {
        Some(unstaking_period) => {
            config.unstaking_period = unstaking_period;
            attrs.push(attr("new_unstaking_period", unstaking_period.to_string()));
        }
        None => {}
    }
    match fee_wait_period {
        Some(fee_wait_period) => {
            config.fee_wait_period = fee_wait_period;
            attrs.push(attr("new_fee_wait_period", fee_wait_period.to_string()));
        }
        None => {}
    }
    match mbrn_denom {
        Some(mbrn_denom) => {
            config.mbrn_denom = mbrn_denom.clone();
            attrs.push(attr("new_mbrn_denom", mbrn_denom));
        }
        None => {}
    }
    match builders_contract {
        Some(builders_contract) => {
            config.builders_contract = Some(deps.api.addr_validate(&builders_contract)?);
            attrs.push(attr("new_builders_contract", builders_contract));
        }
        None => {}
    }
    match positions_contract {
        Some(positions_contract) => {
            config.positions_contract = Some(deps.api.addr_validate(&positions_contract)?);
            attrs.push(attr("new_positions_contract", positions_contract));
        }
        None => {}
    }
    match osmosis_proxy {
        Some(osmosis_proxy) => {
            config.osmosis_proxy = Some(deps.api.addr_validate(&osmosis_proxy)?);
            attrs.push(attr("new_osmosis_proxy", osmosis_proxy));
        }
        None => {}
    }

    //Save new Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attributes(attrs))
}

//From a receive cw20 hook. Comes from the contract address so easy to validate sent funds.
//Check if sent funds are equal to amount in msg so we don't have to recheck in the function
pub fn receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    let passed_asset: Asset = Asset {
        info: AssetInfo::Token {
            address: info.sender.clone(),
        },
        amount: cw20_msg.amount,
    };

    match from_binary(&cw20_msg.msg) {
        Ok(Cw20HookMsg::DepositFee {}) => {
            let config = CONFIG.load(deps.storage)?;

            if cw20_msg.sender != config.positions_contract.unwrap() {
                return Err(ContractError::Unauthorized {});
            }
            deposit_fee(deps, env, info, vec![passed_asset], true)
        }
        Err(_) => Err(ContractError::Cw20MsgError {}),
    }
}

pub fn stake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let valid_asset: Asset;
    //Assert only MBRN was sent
    if info.funds.len() == 1 && info.funds[0].denom == config.mbrn_denom {
        valid_asset = assert_sent_native_token_balance(
            AssetInfo::NativeToken {
                denom: config.mbrn_denom,
            },
            &info,
        )?;
    } else {
        return Err(ContractError::CustomError {
            val: "No valid assets".to_string(),
        });
    }

    let valid_owner_addr = validate_position_owner(deps.api, info.clone(), user)?;

    //Add new deposit to List of StakeDeposit
    let mut current_stake = STAKED.load(deps.storage)?;
    current_stake.push(StakeDeposit {
        staker: valid_owner_addr.clone(),
        amount: valid_asset.amount,
        stake_time: env.block.time.seconds(),
        unstake_start_time: None,
    });
    STAKED.save(deps.storage, &current_stake)?;

    //Add to Totals
    let mut totals = TOTALS.load(deps.storage)?;
    if config.builders_contract.is_some() {
        if info.sender == config.builders_contract.unwrap() {
            totals.builders_contract += valid_asset.amount;
        } else {
            totals.stakers += valid_asset.amount;
        }
        TOTALS.save(deps.storage, &totals)?;
    } else {
        return Err(ContractError::CustomError {
            val: String::from("Builders contract needs to be added to the config"),
        });
    }

    //Response build
    let response = Response::new();
    let attrs = vec![
        attr("method", "stake"),
        attr("staker", valid_owner_addr.to_string()),
        attr("amount", valid_asset.amount.to_string()),
    ];

    Ok(response.add_attributes(attrs))
}

//First call is an unstake
//2nd call is a withdrawal
pub fn unstake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mbrn_withdraw_amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let mut msgs = vec![];

    let fee_events = FEE_EVENTS.load(deps.storage)?;

    //Get total Stake
    let total_stake = {
        let staker_deposits: Vec<StakeDeposit> = STAKED
            .load(deps.storage)?
            .into_iter()
            .filter(|deposit| deposit.staker == info.clone().sender)
            .collect::<Vec<StakeDeposit>>();

        if staker_deposits == vec![] {
            return Err(ContractError::CustomError {
                val: String::from("User has no stake"),
            });
        }

        let total_staker_deposits: Uint128 = staker_deposits
            .into_iter()
            .map(|deposit| deposit.amount)
            .collect::<Vec<Uint128>>()
            .into_iter()
            .sum();

        total_staker_deposits
    };

    //Assert valid stake
    let withdraw_amount = mbrn_withdraw_amount.unwrap_or(total_stake);
    if withdraw_amount > total_stake {
        return Err(ContractError::CustomError {
            val: String::from("Invalid withdrawal amount"),
        });
    }

    //info.sender is user
    let (claimables, accrued_interest, withdrawable_amount) = withdraw_from_state(
        deps.storage,
        env,
        info.clone().sender,
        withdraw_amount,
        fee_events,
    )?;

    //List of coins to send
    let mut native_claims = vec![];

    //If user can unstake, add to native claims list
    if !withdrawable_amount.is_zero() {
        //Push to native claims list
        native_claims.push(asset_to_coin(Asset {
            info: AssetInfo::NativeToken {
                denom: config.clone().mbrn_denom,
            },
            amount: withdrawable_amount,
        })?);
    }

    //Create msg for claimable fees
    if claimables != vec![] {
        //Aggregate native tokens
        for asset in claimables {
            match asset.clone().info {
                AssetInfo::Token { address: _ } => {
                    msgs.push(withdrawal_msg(asset, info.clone().sender)?);
                }
                AssetInfo::NativeToken { denom: _ } => {
                    native_claims.push(asset_to_coin(asset)?);
                }
            }
        }
    }

    if native_claims != vec![] {
        let msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: native_claims,
        });
        msgs.push(msg);
    }

    //Create msg for accrued interest
    if !accrued_interest.is_zero() {
        let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
            msg: to_binary(&OsmoExecuteMsg::MintTokens {
                denom: config.clone().mbrn_denom,
                amount: accrued_interest,
                mint_to_address: info.sender.to_string(),
            })?,
            funds: vec![],
        });
        msgs.push(message);
    }

    //Correct Totals
    let mut totals = TOTALS.load(deps.storage)?;
    if info.sender == config.builders_contract.unwrap() {
        totals.builders_contract -= withdrawable_amount;
    } else {
        totals.stakers -= withdrawable_amount;
    }
    TOTALS.save(deps.storage, &totals)?;

    //Response builder
    let response = Response::new();
    let attrs = vec![
        attr("method", "unstake"),
        attr("staker", info.sender.to_string()),
        attr("unstake_amount", withdrawable_amount.to_string()),
    ];

    Ok(response.add_attributes(attrs).add_messages(msgs))
}

//Restake unstaking deposits for a user
fn restake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mut restake_amount: Uint128,
) -> Result<Response, ContractError> {
    let initial_restake = restake_amount;

    let restaked_deposits: Vec<StakeDeposit> = STAKED
        .load(deps.storage)?
        .into_iter()
        .map(|mut deposit| {
            if deposit.staker == info.clone().sender && !restake_amount.is_zero() {
                if deposit.amount >= restake_amount {
                    //Zero restake_amount
                    restake_amount = Uint128::zero();

                    //Restake
                    deposit.unstake_start_time = None;
                    deposit.stake_time = env.block.time.seconds();
                } else if deposit.amount < restake_amount {
                    //Sub from restake_amount
                    restake_amount -= deposit.amount;

                    //Restake
                    deposit.unstake_start_time = None;
                    deposit.stake_time = env.block.time.seconds();
                }
            }
            deposit
        })
        .collect::<Vec<StakeDeposit>>();

    //Save new Deposits
    STAKED.save(deps.storage, &restaked_deposits)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "restake"),
        attr("restake_amount", initial_restake),
    ]))
}

//Returns claimable assets, accrued interest, withdrawable amount
fn withdraw_from_state(
    storage: &mut dyn Storage,
    env: Env,
    staker: Addr,
    mut withdrawal_amount: Uint128,
    fee_events: Vec<FeeEvent>,
) -> StdResult<(Vec<Asset>, Uint128, Uint128)> {
    let config = CONFIG.load(storage)?;

    let deposits = STAKED.load(storage)?;

    let mut new_deposit_total = Uint128::zero();
    let mut claimables: Vec<Asset> = vec![];
    let mut accrued_interest = Uint128::zero();
    let mut error: Option<StdError> = None;
    let mut withdrawable_amount = Uint128::zero();
    let mut withdrawable = false;

    let mut returning_deposit: Option<StakeDeposit> = None;

    let mut new_deposits: Vec<StakeDeposit> = deposits
        .into_iter()
        .map(|mut deposit| {
            //Only edit user deposits
            if deposit.staker == staker {
                //If the deposit has started unstaking
                if deposit.unstake_start_time.is_some() {
                    //If the unstake period has been fulfilled
                    if env.block.time.seconds() - deposit.unstake_start_time.unwrap()
                        >= config.unstaking_period
                    {
                        withdrawable = true;
                    }
                }

                //Subtract from each deposit until there is none left to withdraw
                if withdrawal_amount != Uint128::zero() && deposit.amount > withdrawal_amount {
                    //Calc claimables from this deposit
                    let (deposit_claimables, deposit_interest) = match get_deposit_claimables(
                        config.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                    ) {
                        Ok(res) => res,
                        Err(err) => {
                            error = Some(err);
                            (vec![], Uint128::zero())
                        }
                    };
                    accrued_interest += deposit_interest;

                    //Condense like Assets
                    for claim_asset in deposit_claimables {
                        //Check if asset is already in the list of claimables and add according
                        match claimables
                            .clone()
                            .into_iter()
                            .enumerate()
                            .find(|(_i, asset)| asset.info == claim_asset.info)
                        {
                            Some((index, _asset)) => claimables[index].amount += claim_asset.amount,
                            None => claimables.push(claim_asset),
                        }
                    }

                    //If withdrawable...
                    //Set partial deposit total
                    //Set current deposit to 0
                    //Add withdrawal_amount to withdrawable_amount
                    if withdrawable {
                        new_deposit_total = deposit.amount - withdrawal_amount;
                        deposit.amount = Uint128::zero();
                        withdrawable_amount += withdrawal_amount;
                    } else {
                        //Set unstaking time for the amount getting withdrawn
                        //Create a StakeDeposit object for the amount not getting unstaked
                        if deposit.amount > withdrawal_amount
                            && withdrawal_amount != Uint128::zero()
                        {
                            //Set new deposit
                            returning_deposit = Some(StakeDeposit {
                                amount: deposit.amount - withdrawal_amount,
                                unstake_start_time: None,
                                ..deposit.clone()
                            });

                            //Set new deposit amount
                            deposit.amount = withdrawal_amount;
                        }

                        //Set the unstaking_start_time and stake_time to now
                        deposit.unstake_start_time = Some(env.block.time.seconds());
                        //Since we claimed rewards
                        deposit.stake_time = env.block.time.seconds();
                    }
                    //Zero withdrawal_amount
                    withdrawal_amount = Uint128::zero();
                } else if withdrawal_amount != Uint128::zero()
                    && deposit.amount <= withdrawal_amount
                {
                    //Calc claimables from this deposit
                    let (deposit_claimables, deposit_interest) = match get_deposit_claimables(
                        config.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                    ) {
                        Ok(res) => res,
                        Err(err) => {
                            error = Some(err);
                            (vec![], Uint128::zero())
                        }
                    };
                    accrued_interest += deposit_interest;

                    //Condense like Assets
                    for claim_asset in deposit_claimables {
                        //Check if asset is already in the list of claimables and add according
                        match claimables
                            .clone()
                            .into_iter()
                            .enumerate()
                            .find(|(_i, asset)| asset.info == claim_asset.info)
                        {
                            Some((index, _asset)) => claimables[index].amount += claim_asset.amount,
                            None => claimables.push(claim_asset),
                        }
                    }

                    //If it's less than amount, substract it from the withdrawal amount
                    withdrawal_amount -= deposit.amount;

                    //If withdrawable...
                    //Add deposit amount to withdrawable_amount
                    //Set current deposit to 0
                    if withdrawable {
                        withdrawable_amount += deposit.amount;
                        deposit.amount = Uint128::zero();
                    } else {
                        //Ee, Set the unstaking_start_time and stake_time to now
                        deposit.unstake_start_time = Some(env.block.time.seconds());
                        //Since we claimed rewards
                        deposit.stake_time = env.block.time.seconds();
                    }
                }
            }
            deposit
        })
        .collect::<Vec<StakeDeposit>>()
        .into_iter()
        .filter(|deposit| deposit.amount != Uint128::zero())
        .collect::<Vec<StakeDeposit>>();

    if withdrawal_amount != Uint128::zero() {
        return Err(StdError::GenericErr {
            msg: format!(
                "Attempting to withdraw {} MBRN over ( {} )'s total deposit",
                withdrawal_amount, staker
            ),
        });
    }

    if error.is_some() {
        return Err(error.unwrap());
    }

    //Push returning_deposit if some
    if let Some(deposit) = returning_deposit {
        new_deposits.push(deposit);
    }

    //We set any edited deposit to zero and push any partial withdrawals back to the list here
    if !new_deposit_total.is_zero() {
        new_deposits.push(StakeDeposit {
            staker,
            amount: new_deposit_total,
            stake_time: env.block.time.seconds(),
            unstake_start_time: None,
        });
    }
    //Save new deposit stack
    STAKED.save(storage, &new_deposits)?;

    Ok((claimables, accrued_interest, withdrawable_amount))
}

//Sends available claims to info.sender
//If asset is passed, the claims will be sent as said asset
pub fn claim_rewards(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    claim_as_native: Option<String>,
    claim_as_cw20: Option<String>,
    send_to: Option<String>,
    restake: bool,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg>;
    let accrued_interest: Uint128;
    let user_claimables: Vec<Asset>;

    //Only 1 toggle at a time
    if claim_as_native.is_some() && claim_as_cw20.is_some() {
        return Err(ContractError::CustomError {
            val: "Can't claim as multiple assets, if not all claimable assets".to_string(),
        });
    }

    (messages, user_claimables, accrued_interest) = user_claims(
        deps.storage,
        deps.api,
        env.clone(),
        config.clone(),
        info.clone(),
        config.clone().dex_router,
        claim_as_native.clone(),
        claim_as_cw20.clone(),
        send_to.clone(),
    )?;

    //Because get_user_claimables() in user_claim_msgs() saves a condensed user deposit at the front of the List...
    // //we can fetch the user's total deposits from the end of the list
    // let staked = STAKED.load( deps.storage )?;
    // let user_stake = &staked[ staked.len()-1 ];

    //Create MBRN Mint Msg
    if config.osmosis_proxy.is_some() {
        if info.sender != config.clone().builders_contract.unwrap() && !accrued_interest.is_zero() {
            //Who to send to?
            if send_to.is_some() {
                let valid_receipient = deps.api.addr_validate(&send_to.clone().unwrap())?;

                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom: config.mbrn_denom,
                        amount: accrued_interest,
                        mint_to_address: valid_receipient.to_string(),
                    })?,
                    funds: vec![],
                });
                messages.push(message);
            } else if restake {
                //Mint to contract
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom: config.clone().mbrn_denom,
                        amount: accrued_interest,
                        mint_to_address: env.contract.address.to_string(),
                    })?,
                    funds: vec![],
                });
                messages.push(message);
                //Stake for user
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_binary(&ExecuteMsg::Stake {
                        user: Some(info.sender.to_string()),
                    })?,
                    funds: vec![coin(accrued_interest.u128(), config.mbrn_denom)],
                });
                messages.push(message);
            } else {
                //Send stake to sender
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom: config.mbrn_denom,
                        amount: accrued_interest,
                        mint_to_address: info.sender.to_string(),
                    })?,
                    funds: vec![],
                });
                messages.push(message);
            }
        }
    } else {
        return Err(ContractError::CustomError {
            val: String::from("No proxy contract setup"),
        });
    }

    let user_claimables_string: Vec<String> = user_claimables
        .into_iter()
        .map(|claims| claims.to_string())
        .collect::<Vec<String>>();

    let res = Response::new()
        .add_attribute("method", "claim")
        .add_attribute("user", info.sender)
        .add_attribute("claim_as_native", claim_as_native.unwrap_or_default())
        .add_attribute("claim_as_cw20", claim_as_cw20.unwrap_or_default())
        .add_attribute("send_to", send_to.unwrap_or_default())
        .add_attribute("restake", restake.to_string())
        .add_attribute("mbrn_rewards", accrued_interest.to_string())
        .add_attribute("fee_rewards", format!("{:?}", user_claimables_string));

    Ok(res.add_messages(messages))
}

fn accumulate_interest(stake: Uint128, rate: Decimal, time_elapsed: u64) -> StdResult<Uint128> {
    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    let accrued_interest = stake * applied_rate;

    Ok(accrued_interest)
}

fn deposit_fee(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    fee_assets: Vec<Asset>,
    cw20_contract: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.positions_contract.unwrap() && !cw20_contract {
        return Err(ContractError::Unauthorized {});
    }

    //Load Fee Events
    let mut fee_events = FEE_EVENTS.load(deps.storage)?;

    //Load Total staked
    let totals = TOTALS.load(deps.storage)?;
    let mut total = totals.builders_contract + totals.stakers;

    if total.is_zero() {
        total = Uint128::new(1u128)
    }

    let decimal_total = Decimal::from_ratio(total, Uint128::new(1u128));

    for asset in fee_assets.clone() {
        let amount = Decimal::from_ratio(asset.amount, Uint128::new(1u128));
        //Add new Fee Event
        fee_events.push(FeeEvent {
            time_of_event: env.block.time.seconds(),
            fee: LiqAsset {
                //Amount = Amount per Staked MBRN
                info: asset.info,
                amount: decimal_division(amount, decimal_total),
            },
        });
    }

    //Save new List of Events
    FEE_EVENTS.save(deps.storage, &fee_events)?;

    let string_fee_assets = fee_assets
        .into_iter()
        .map(|asset| asset.to_string())
        .collect::<Vec<String>>();

    Ok(Response::new().add_attributes(vec![
        attr("method", "deposit_fee"),
        attr("fee_assets", format!("{:?}", string_fee_assets)),
    ]))
}

fn user_claims(
    storage: &mut dyn Storage,
    api: &dyn Api,
    env: Env,
    config: Config,
    info: MessageInfo,
    dex_router: Option<Addr>,
    claim_as_native: Option<String>,
    claim_as_cw20: Option<String>,
    send_to: Option<String>,
) -> StdResult<(Vec<CosmosMsg>, Vec<Asset>, Uint128)> {
    let mut messages: Vec<CosmosMsg> = vec![];

    //Can only claim for oneself (info.sender)
    let (user_claimables, accrued_interest) =
        get_user_claimables(storage, env, info.clone().sender)?;

    //If we are claiming the available assets without swaps
    if claim_as_cw20.is_none() && claim_as_native.is_none() {
        for asset in user_claimables.clone() {
            if send_to.clone().is_none() {
                messages.push(withdrawal_msg(asset, info.clone().sender)?);
            } else {
                let valid_receipient = api.addr_validate(&send_to.clone().unwrap())?;
                messages.push(withdrawal_msg(asset, valid_receipient)?);
            }
        }
    } else if dex_router.is_some() {
        //Router usage
        for asset in user_claimables.clone() {
            match asset.info {
                AssetInfo::Token { address } => {
                    //Swap to Cw20 before sending or depositing
                    if claim_as_cw20.is_some() {
                        let valid_claim_addr =
                            api.addr_validate(&claim_as_cw20.clone().unwrap())?;

                        if send_to.clone().is_some() {
                            //Send to Optional receipient
                            let valid_receipient = api.addr_validate(&send_to.clone().unwrap())?;
                            //Create Cw20 Router SwapMsgs
                            let swap_hook = RouterCw20HookMsg::Swap {
                                to: AssetInfo::Token {
                                    address: valid_claim_addr,
                                },
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(valid_receipient.to_string()),
                                hook_msg: None,
                                split: None,
                            };

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: address.to_string(),
                                msg: to_binary(&Cw20ExecuteMsg::Send {
                                    contract: config.clone().dex_router.unwrap().to_string(),
                                    amount: asset.amount,
                                    msg: to_binary(&swap_hook)?,
                                })?,
                                funds: vec![],
                            });

                            messages.push(message);
                        } else {
                            //Send to Staker
                            //Create Cw20 Router SwapMsgs
                            let swap_hook = RouterCw20HookMsg::Swap {
                                to: AssetInfo::Token {
                                    address: valid_claim_addr,
                                },
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(info.clone().sender.to_string()),
                                hook_msg: None,
                                split: None,
                            };

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: address.to_string(),
                                msg: to_binary(&Cw20ExecuteMsg::Send {
                                    contract: config.clone().dex_router.unwrap().to_string(),
                                    amount: asset.amount,
                                    msg: to_binary(&swap_hook)?,
                                })?,
                                funds: vec![],
                            });

                            messages.push(message);
                        }
                    }
                    //Swap to native before sending or depositing
                    else if claim_as_native.is_some() {
                        if send_to.clone().is_some() {
                            //Send to Optional receipient
                            let valid_receipient = api.addr_validate(&send_to.clone().unwrap())?;
                            //Create Cw20 Router SwapMsgs
                            let swap_hook = RouterCw20HookMsg::Swap {
                                to: AssetInfo::NativeToken {
                                    denom: claim_as_native.clone().unwrap(),
                                },
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(valid_receipient.to_string()),
                                hook_msg: None,
                                split: None,
                            };

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: address.to_string(),
                                msg: to_binary(&Cw20ExecuteMsg::Send {
                                    contract: config.clone().dex_router.unwrap().to_string(),
                                    amount: asset.amount,
                                    msg: to_binary(&swap_hook)?,
                                })?,
                                funds: vec![],
                            });

                            messages.push(message);
                        } else {
                            //Send to Staker
                            //Create Cw20 Router SwapMsgs
                            let swap_hook = RouterCw20HookMsg::Swap {
                                to: AssetInfo::NativeToken {
                                    denom: claim_as_native.clone().unwrap(),
                                },
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(info.clone().sender.to_string()),
                                hook_msg: None,
                                split: None,
                            };

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: address.to_string(),
                                msg: to_binary(&Cw20ExecuteMsg::Send {
                                    contract: config.clone().dex_router.unwrap().to_string(),
                                    amount: asset.amount,
                                    msg: to_binary(&swap_hook)?,
                                })?,
                                funds: vec![],
                            });

                            messages.push(message);
                        }
                    }
                }
                /////Starting token is native so msgs go straight to the router contract
                AssetInfo::NativeToken { denom: _ } => {
                    //Swap to Cw20 before sending or depositing
                    if claim_as_cw20.is_some() {
                        let valid_claim_addr =
                            api.addr_validate(claim_as_cw20.clone().unwrap().as_ref())?;

                        if send_to.clone().is_some() {
                            //Send to Optional receipient
                            let valid_receipient = api.addr_validate(&send_to.clone().unwrap())?;
                            //Create Cw20 Router SwapMsgs
                            let swap_hook = RouterExecuteMsg::SwapFromNative {
                                to: AssetInfo::Token {
                                    address: valid_claim_addr,
                                },
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(valid_receipient.to_string()),
                                hook_msg: None,
                                split: None,
                            };

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: config.clone().dex_router.unwrap().to_string(),
                                msg: to_binary(&swap_hook)?,
                                funds: vec![asset_to_coin(asset)?],
                            });

                            messages.push(message);
                        } else {
                            //Create Cw20 Router SwapMsgs
                            let swap_hook = RouterExecuteMsg::SwapFromNative {
                                to: AssetInfo::Token {
                                    address: valid_claim_addr,
                                },
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(info.clone().sender.to_string()),
                                hook_msg: None,
                                split: None,
                            };

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: config.clone().dex_router.unwrap().to_string(),
                                msg: to_binary(&swap_hook)?,
                                funds: vec![asset_to_coin(asset)?],
                            });

                            messages.push(message);
                        }
                    }
                    //Swap to native before sending or depositing
                    else if claim_as_native.is_some() {
                        if send_to.clone().is_some() {
                            //Send to Optional receipient
                            let valid_receipient = api.addr_validate(&send_to.clone().unwrap())?;
                            //Create Cw20 Router SwapMsgs
                            let swap_hook = RouterExecuteMsg::SwapFromNative {
                                to: AssetInfo::NativeToken {
                                    denom: claim_as_native.clone().unwrap(),
                                },
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(valid_receipient.to_string()),
                                hook_msg: None,
                                split: None,
                            };

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: config.clone().dex_router.unwrap().to_string(),
                                msg: to_binary(&swap_hook)?,
                                funds: vec![asset_to_coin(asset)?],
                            });

                            messages.push(message);
                        } else {
                            //Send to Staker
                            //Create Cw20 Router SwapMsgs
                            let swap_hook = RouterExecuteMsg::SwapFromNative {
                                to: AssetInfo::NativeToken {
                                    denom: claim_as_native.clone().unwrap(),
                                },
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(info.clone().sender.to_string()),
                                hook_msg: None,
                                split: None,
                            };

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: config.clone().dex_router.unwrap().to_string(),
                                msg: to_binary(&swap_hook)?,
                                funds: vec![asset_to_coin(asset)?],
                            });

                            messages.push(message);
                        }
                    }
                }
            }
        }
    } else {
        return Err(StdError::GenericErr {
            msg: String::from("Can't claim as without a DEX router"),
        });
    }

    //Once messages are created, we set user_claims to vec![]

    Ok((messages, user_claimables, accrued_interest))
}

fn get_user_claimables(
    storage: &mut dyn Storage,
    env: Env,
    staker: Addr,
) -> StdResult<(Vec<Asset>, Uint128)> {
    let config = CONFIG.load(storage)?;

    let deposits: Vec<StakeDeposit> = STAKED
        .load(storage)?
        .into_iter()
        .filter(|deposit| deposit.staker == staker)
        .collect::<Vec<StakeDeposit>>();

    if deposits == vec![] {
        return Err(StdError::GenericErr {
            msg: String::from("User has no stake"),
        });
    }

    //Load Fee events
    let fee_events = FEE_EVENTS.load(storage)?;

    let mut claimables: Vec<Asset> = vec![];
    let mut total_deposits = Uint128::zero();
    let mut accrued_interest = Uint128::zero();

    for deposit in deposits {
        let (deposit_claimables, deposit_interest) = get_deposit_claimables(
            config.clone(),
            env.clone(),
            fee_events.clone(),
            deposit.clone(),
        )?;
        accrued_interest += deposit_interest;

        //Condense like Assets
        for claim_asset in deposit_claimables {
            //Check if asset is already in the list of claimables and add according
            match claimables
                .clone()
                .into_iter()
                .enumerate()
                .find(|(_i, asset)| asset.info == claim_asset.info)
            {
                Some((index, _asset)) => claimables[index].amount += claim_asset.amount,
                None => claimables.push(claim_asset),
            }
        }

        //Total deposits
        total_deposits += deposit.amount;
    }

    //Filter out user deposits
    let mut new_deposits: Vec<StakeDeposit> = STAKED
        .load(storage)?
        .into_iter()
        .filter(|deposit| deposit.staker != staker)
        .collect::<Vec<StakeDeposit>>();

    //Add new condensed deposit for user
    new_deposits.push(StakeDeposit {
        staker,
        amount: total_deposits,
        stake_time: env.block.time.seconds(),
        unstake_start_time: None,
    });
    //Save new StakeDeposit list
    STAKED.save(storage, &new_deposits)?;

    Ok((claimables, accrued_interest))
}

fn get_deposit_claimables(
    config: Config,
    env: Env,
    fee_events: Vec<FeeEvent>,
    deposit: StakeDeposit,
) -> StdResult<(Vec<Asset>, Uint128)> {
    let mut claimables: Vec<Asset> = vec![];

    //Filter for events that the deposit was staked for
    //ie event times after the deposit
    let wait_period_in_seconds = config.fee_wait_period * SECONDS_PER_DAY;
    let events_experienced = fee_events
        .into_iter()
        .filter(|event| event.time_of_event >= deposit.stake_time + wait_period_in_seconds)
        .collect::<Vec<FeeEvent>>();

    //Condense like Assets
    for event in events_experienced {
        //Check if asset is already in the list of claimables and add according
        match claimables
            .clone()
            .into_iter()
            .enumerate()
            .find(|(_i, asset)| asset.info == event.fee.info)
        {
            Some((index, _asset)) => claimables[index].amount += event.fee.amount * deposit.amount,
            None => claimables.push(Asset {
                info: event.fee.info,
                amount: event.fee.amount * deposit.amount,
            }),
        }
    }

    //Calc MBRN denominated rewards
    let time_elapsed = env.block.time.seconds() - deposit.stake_time;
    let deposit_interest = accumulate_interest(deposit.amount, config.staking_rate, time_elapsed)?;

    Ok((claimables, deposit_interest))
}

pub fn withdrawal_msg(asset: Asset, recipient: Addr) -> StdResult<CosmosMsg> {
    match asset.clone().info {
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
        AssetInfo::NativeToken { denom: _ } => {
            let coin: Coin = asset_to_coin(asset)?;
            let message = CosmosMsg::Bank(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![coin],
            });
            Ok(message)
        }
    }
}

pub fn asset_to_coin(asset: Asset) -> StdResult<Coin> {
    match asset.info {
        //
        AssetInfo::Token { address: _ } => {
            Err(StdError::GenericErr {
                msg: String::from("CW20 Assets can't be converted into Coin"),
            })
        }
        AssetInfo::NativeToken { denom } => Ok(Coin {
            denom,
            amount: asset.amount,
        }),
    }
}

//Refactored Terraswap function
pub fn assert_sent_native_token_balance(
    asset_info: AssetInfo,
    message_info: &MessageInfo,
) -> StdResult<Asset> {
    let asset: Asset;

    if let AssetInfo::NativeToken { denom } = &asset_info {
        match message_info.funds.iter().find(|x| x.denom == *denom) {
            Some(coin) => {
                if coin.amount > Uint128::zero() {
                    asset = Asset {
                        info: asset_info,
                        amount: coin.amount,
                    };
                } else {
                    return Err(StdError::generic_err("You gave me nothing to deposit"));
                }
            }
            None => {
                return Err(StdError::generic_err(
                    "Incorrect denomination, sent asset denom and asset.info.denom differ",
                ))
            }
        }
    } else {
        return Err(StdError::generic_err(
            "Asset type not native, check Msg schema and use AssetInfo::Token{ address: Addr }",
        ));
    }

    Ok(asset)
}

//Validate Recipient
pub fn validate_position_owner(
    deps: &dyn Api,
    info: MessageInfo,
    recipient: Option<String>,
) -> StdResult<Addr> {
    let valid_recipient: Addr = if let Some(recipient) = recipient {
        deps.addr_validate(&recipient)?
    } else {
        info.sender
    };
    Ok(valid_recipient)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
        QueryMsg::UserStake { staker } => to_binary(&query_user_stake(deps, staker)?),
        QueryMsg::StakerRewards { staker } => to_binary(&query_staker_rewards(deps, env, staker)?),
        QueryMsg::Staked {
            limit,
            start_after,
            end_before,
            unstaking,
        } => to_binary(&query_staked(
            deps,
            env,
            limit,
            start_after,
            end_before,
            unstaking,
        )?),
        QueryMsg::FeeEvents { limit, start_after } => {
            to_binary(&query_fee_events(deps, limit, start_after)?)
        }
        QueryMsg::TotalStaked {} => to_binary(&query_totals(deps)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;

    Ok(ConfigResponse {
        owner: config.owner.to_string(),
        dex_router: config
            .dex_router
            .unwrap_or(Addr::unchecked("None"))
            .to_string(),
        max_spread: config.max_spread.unwrap().to_string(),
        positions_contract: config
            .positions_contract
            .unwrap_or(Addr::unchecked("None"))
            .to_string(),
        builders_contract: config
            .builders_contract
            .unwrap_or(Addr::unchecked("None"))
            .to_string(),
        osmosis_proxy: config
            .osmosis_proxy
            .unwrap_or(Addr::unchecked("None"))
            .to_string(),
        staking_rate: config.staking_rate.to_string(),
        fee_wait_period: config.fee_wait_period.to_string(),
        mbrn_denom: config.mbrn_denom.to_string(),
        unstaking_period: config.unstaking_period.to_string(),
    })
}

fn query_user_stake(deps: Deps, staker: String) -> StdResult<StakerResponse> {
    let valid_addr = deps.api.addr_validate(&staker)?;

    let staker_deposits: Vec<StakeDeposit> = STAKED
        .load(deps.storage)?
        .into_iter()
        .filter(|deposit| deposit.staker == valid_addr)
        .collect::<Vec<StakeDeposit>>();

    let deposit_list = staker_deposits
        .clone()
        .into_iter()
        .map(|deposit| (deposit.amount.to_string(), deposit.stake_time.to_string()))
        .collect::<Vec<(String, String)>>();

    let total_staker_deposits: Uint128 = staker_deposits
        .into_iter()
        .map(|deposit| deposit.amount)
        .collect::<Vec<Uint128>>()
        .into_iter()
        .sum();

    Ok(StakerResponse {
        staker: valid_addr.to_string(),
        total_staked: total_staker_deposits,
        deposit_list,
    })
}

fn query_staker_rewards(deps: Deps, env: Env, staker: String) -> StdResult<RewardsResponse> {
    let config = CONFIG.load(deps.storage)?;

    let valid_addr = deps.api.addr_validate(&staker)?;

    let staker_deposits: Vec<StakeDeposit> = STAKED
        .load(deps.storage)?
        .into_iter()
        .filter(|deposit| deposit.staker == valid_addr)
        .collect::<Vec<StakeDeposit>>();

    let fee_events = FEE_EVENTS.load(deps.storage)?;

    let mut claimables = vec![];
    let mut accrued_interest = Uint128::zero();
    for deposit in staker_deposits {
        let res = get_deposit_claimables(config.clone(), env.clone(), fee_events.clone(), deposit)?;
        claimables.extend(res.0);
        accrued_interest += res.1;
    }

    Ok(RewardsResponse {
        claimables,
        accrued_interest,
    })
}

fn query_staked(
    deps: Deps,
    env: Env,
    limit: Option<u32>,
    start_after: Option<u64>,
    end_before: Option<u64>,
    unstaking: bool,
) -> StdResult<StakedResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let start_after = start_after.unwrap_or(0u64);
    let end_before = end_before.unwrap_or_else(|| env.block.time.seconds() + 1u64);

    let mut stakers = STAKED
        .load(deps.storage)?
        .into_iter()
        .filter(|deposit| deposit.stake_time >= start_after && deposit.stake_time < end_before)
        .take(limit as usize)
        .collect::<Vec<StakeDeposit>>();

    //Filter out unstakers
    if !unstaking {
        stakers = stakers
            .clone()
            .into_iter()
            .filter(|deposit| deposit.unstake_start_time.is_none())
            .collect::<Vec<StakeDeposit>>();
    }

    Ok(StakedResponse { stakers })
}

fn query_fee_events(
    deps: Deps,
    limit: Option<u32>,
    start_after: Option<u64>,
) -> StdResult<FeeEventsResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let start_after = start_after.unwrap_or(0u64);

    let fee_events = FEE_EVENTS
        .load(deps.storage)?
        .into_iter()
        .filter(|event| event.time_of_event >= start_after)
        .take(limit as usize)
        .collect::<Vec<FeeEvent>>();

    Ok(FeeEventsResponse { fee_events })
}

fn query_totals(deps: Deps) -> StdResult<TotalStakedResponse> {
    let totals = TOTALS.load(deps.storage)?;

    Ok(TotalStakedResponse {
        total_not_including_builders: totals.stakers.to_string(),
        builders_total: totals.builders_contract.to_string(),
    })
}
