#[cfg(not(feature = "library"))]
use std::env;


use cosmwasm_std::{entry_point, Coin};
use cosmwasm_std::{
    attr, coin, to_binary, Addr, Api, BankMsg, Binary, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Uint128, WasmMsg, QueryRequest, WasmQuery, QuerierWrapper,
};
use cw2::set_contract_version;

use membrane::apollo_router::{ExecuteMsg as RouterExecuteMsg, SwapToAssetsInput};
use membrane::governance::{QueryMsg as Gov_QueryMsg, ProposalListResponse, ProposalStatus};
use membrane::helpers::{assert_sent_native_token_balance, validate_position_owner, asset_to_coin, accrue_user_positions};
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::cdp::QueryMsg as CDP_QueryMsg;
use membrane::auction::ExecuteMsg as AuctionExecuteMsg;
use membrane::staking::{ Config, ExecuteMsg, InstantiateMsg, QueryMsg };
use membrane::vesting::{QueryMsg as Vesting_QueryMsg, RecipientsResponse};
use membrane::types::{Asset, AssetInfo, FeeEvent, LiqAsset, StakeDeposit, StakeDistributionLog, StakeDistribution, Basket};
use membrane::math::decimal_division;

use crate::error::ContractError;
use crate::query::{query_user_stake, query_staker_rewards, query_staked, query_fee_events, query_totals};
use crate::state::{Totals, CONFIG, FEE_EVENTS, STAKED, TOTALS, INCENTIVE_SCHEDULING, OWNERSHIP_TRANSFER};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:staking";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_PER_YEAR: u64 = 31_536_000u64;
const SECONDS_PER_DAY: u64 = 86_400u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config: Config;

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            positions_contract: None,
            auction_contract: None,
            vesting_contract: None,
            governance_contract: None,
            osmosis_proxy: None,
            incentive_schedule: msg.incentive_schedule.unwrap_or_else(|| StakeDistribution {
                rate: Decimal::percent(123),
                duration: 240,
            }),
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
            auction_contract: None,
            vesting_contract: None,
            governance_contract: None,
            osmosis_proxy: None,
            incentive_schedule: msg.incentive_schedule.unwrap_or_else(|| StakeDistribution {
                rate: Decimal::percent(123),
                duration: 240,
            }),
            fee_wait_period: msg.fee_wait_period.unwrap_or(3u64),
            unstaking_period: msg.unstaking_period.unwrap_or(3u64),
            mbrn_denom: msg.mbrn_denom,
            dex_router: None,
            max_spread: msg.max_spread,
        };
    }

    //Set optional config parameters
    if let Some(dex_router) = msg.dex_router {
        config.dex_router = Some(deps.api.addr_validate(&dex_router)?);
    };
    if let Some(vesting_contract) = msg.vesting_contract {        
        config.vesting_contract = Some(deps.api.addr_validate(&vesting_contract)?);
    };
    if let Some(positions_contract) = msg.positions_contract {
        config.positions_contract = Some(deps.api.addr_validate(&positions_contract)?);
    };
    if let Some(auction_contract) = msg.auction_contract {
        config.auction_contract = Some(deps.api.addr_validate(&auction_contract)?);
    };
    if let Some(governance_contract) = msg.governance_contract {
        config.governance_contract = Some(deps.api.addr_validate(&governance_contract)?);
    };
    if let Some(osmosis_proxy) = msg.osmosis_proxy {
        config.osmosis_proxy = Some(deps.api.addr_validate(&osmosis_proxy)?);
    };

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    //Initialize StakeDeposit List
    STAKED.save(deps.storage, &vec![])?;

    //Initialize stake Totals
    TOTALS.save(
        deps.storage,
        &Totals {
            stakers: Uint128::zero(),
            vesting_contract: Uint128::zero(),
        },
    )?;
    //Initialize fee events
    FEE_EVENTS.save(deps.storage, &vec![])?;

    //Initialize INCENTIVE_SCHEDULING
    INCENTIVE_SCHEDULING.save(deps.storage, &StakeDistributionLog {
        ownership_distribution: config.clone().incentive_schedule,
        start_time: env.block.time.seconds(),
    })?;

    
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address))
}

/// Return total MBRN vesting
fn get_total_vesting(
    querier: QuerierWrapper,    
    vesting_contract: String,
) -> StdResult<Uint128>{

    let recipients = querier.query::<RecipientsResponse>(&QueryRequest::Wasm(WasmQuery::Smart { 
        contract_addr: vesting_contract, 
        msg: to_binary(&Vesting_QueryMsg::Recipients { })?
    }))?;    

    Ok(recipients.get_total_vesting())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig {
            owner,
            mbrn_denom,
            dex_router,
            max_spread,
            vesting_contract,
            governance_contract,
            osmosis_proxy,
            positions_contract,
            auction_contract,
            incentive_schedule,
            fee_wait_period,
            unstaking_period,
        } => update_config(
            deps,
            info,
            env,
            owner,
            positions_contract,
            auction_contract,
            vesting_contract,
            governance_contract,
            osmosis_proxy,
            mbrn_denom,
            incentive_schedule,
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
            send_to,
            restake,
        } => claim_rewards(
            deps,
            env,
            info,
            claim_as_native,
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

            deposit_fee(deps, env, fee_assets)
        },
        ExecuteMsg::TrimFeeEvents {  } => trim_fee_events(deps.storage, info),
    }
}

/// Update contract configuration
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    owner: Option<String>,
    positions_contract: Option<String>,
    auction_contract: Option<String>,
    vesting_contract: Option<String>,
    governance_contract: Option<String>,
    osmosis_proxy: Option<String>,
    mbrn_denom: Option<String>,
    incentive_schedule: Option<StakeDistribution>,
    fee_wait_period: Option<u64>,
    unstaking_period: Option<u64>,
    dex_router: Option<String>,
    max_spread: Option<Decimal>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    let mut attrs = vec![attr("method", "update_config")];

    //Match Optionals
    if let Some(owner) = owner {
        let valid_addr = deps.api.addr_validate(&owner)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));     
    };
    if let Some(max_spread) = max_spread {
        config.max_spread = Some(max_spread);
    };
    if let Some(incentive_schedule) = incentive_schedule {
        //Update incentive schedule
        config.incentive_schedule = incentive_schedule.clone();

        //Set Scheduling
        INCENTIVE_SCHEDULING.save(deps.storage, 
            &StakeDistributionLog { 
                ownership_distribution: incentive_schedule, 
                start_time: env.block.time.seconds(),
        })?;
    };
    if let Some(unstaking_period) = unstaking_period {
        config.unstaking_period = unstaking_period;
    };
    if let Some(fee_wait_period) = fee_wait_period {
        config.fee_wait_period = fee_wait_period;
    };
    if let Some(mbrn_denom) = mbrn_denom {
        config.mbrn_denom = mbrn_denom.clone();
    };
    if let Some(dex_router) = dex_router {
        config.dex_router = Some(deps.api.addr_validate(&dex_router)?);
    };
    if let Some(vesting_contract) = vesting_contract {
        config.vesting_contract = Some(deps.api.addr_validate(&vesting_contract)?);
    };
    if let Some(positions_contract) = positions_contract {
        config.positions_contract = Some(deps.api.addr_validate(&positions_contract)?);
    };
    if let Some(auction_contract) = auction_contract {
        config.auction_contract = Some(deps.api.addr_validate(&auction_contract)?);
    };
    if let Some(governance_contract) = governance_contract {
        config.governance_contract = Some(deps.api.addr_validate(&governance_contract)?);
    };
    if let Some(osmosis_proxy) = osmosis_proxy {
            config.osmosis_proxy = Some(deps.api.addr_validate(&osmosis_proxy)?);
    };

    //Save new Config
    CONFIG.save(deps.storage, &config)?;
    attrs.push(attr("updated_config", format!("{:?}", config)));
    
    Ok(Response::new().add_attributes(attrs))
}

/// Stake MBRN
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
                denom: config.clone().mbrn_denom,
            },
            &info,
        )?;
    } else {
        return Err(ContractError::CustomError {
            val: "No valid assets".to_string(),
        });
    }

    //Set valid address
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
    if let Some(vesting_contract) = config.clone().vesting_contract{
        if info.clone().sender == vesting_contract {
            totals.vesting_contract += valid_asset.amount;
        } else {
            totals.stakers += valid_asset.amount;
        }
    } else {
        totals.stakers += valid_asset.amount;
    }
    TOTALS.save(deps.storage, &totals)?;    

    //Response build
    let response = Response::new();
    let attrs = vec![
        attr("method", "stake"),
        attr("staker", valid_owner_addr.to_string()),
        attr("amount", valid_asset.amount.to_string()),
    ];

    Ok(response.add_attributes(attrs))
}

/// First call is an unstake
/// 2nd call after unstake period is a withdrawal
pub fn unstake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mbrn_withdraw_amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let fee_events = FEE_EVENTS.load(deps.storage)?;

    //Restrict unstaking
    can_this_addr_unstake(deps.querier, info.clone().sender, config.clone())?;

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

    //Initialize variables
    let mut native_claims = vec![];
    let mut msgs = vec![];

    //If user can withdraw, accrue their positions and add to native_claims
    if !withdrawable_amount.is_zero() {
        //Create Position accrual msgs to lock in user discounts before withdrawing
        let accrual_msg = accrue_user_positions(
            deps.querier, 
            config.clone().positions_contract.unwrap_or_else(|| Addr::unchecked("")).to_string(),
            info.sender.clone().to_string(), 
            32,
        )?;
        msgs.push(accrual_msg);

        //Push to native claims list
        native_claims.push(asset_to_coin(Asset {
            info: AssetInfo::NativeToken {
                denom: config.clone().mbrn_denom,
            },
            amount: withdrawable_amount,
        })?);
    }

    //Create claimable msgs
    let claims_msgs = create_rewards_msgs(
        config.clone(), 
        claimables.clone(), 
        accrued_interest.clone(),
        info.clone().sender.to_string(),
        native_claims,
    )?;
    msgs.extend(claims_msgs);

    //Update Totals
    let mut totals = TOTALS.load(deps.storage)?;
    if let Some(vesting_contract) = config.clone().vesting_contract{
        if info.clone().sender == vesting_contract{
            totals.vesting_contract -= withdrawable_amount;
        } else {
            totals.stakers -= withdrawable_amount;
        }
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

/// Restake unstaking deposits for a user
fn restake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mut restake_amount: Uint128,
) -> Result<Response, ContractError> {
    //Load state
    let config = CONFIG.load(deps.storage)?;
    let incentive_schedule = INCENTIVE_SCHEDULING.load(deps.storage)?;
    let fee_events = FEE_EVENTS.load(deps.storage)?;

    //Initialize variables
    let mut claimables: Vec<Asset> = vec![];
    let mut accrued_interest = Uint128::zero();
    let initial_restake = restake_amount;
    let mut error: Option<StdError> = None;

    //Iterate through deposits
    let restaked_deposits: Vec<StakeDeposit> = STAKED
        .load(deps.storage)?
        .into_iter()
        .map(|mut deposit| {
            if deposit.staker == info.clone().sender && !restake_amount.is_zero() {
                if deposit.amount >= restake_amount {
                    //Zero restake_amount
                    restake_amount = Uint128::zero();

                    //Add claimables from this deposit
                    match add_deposit_claimables(
                        config.clone(),
                        incentive_schedule.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                        &mut claimables,
                        &mut accrued_interest,
                    ) {
                        Ok(res) => res,
                        Err(err) => 
                            error = Some(err)                        
                    };

                    //Restake
                    deposit.unstake_start_time = None;
                    deposit.stake_time = env.block.time.seconds();
                } else if deposit.amount < restake_amount {
                    //Sub from restake_amount
                    restake_amount -= deposit.amount;

                    //Add claimables from this deposit
                    match add_deposit_claimables(
                        config.clone(),
                        incentive_schedule.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                        &mut claimables,
                        &mut accrued_interest,
                    ) {
                        Ok(res) => res,
                        Err(err) => 
                            error = Some(err)                        
                    };

                    //Restake
                    deposit.unstake_start_time = None;
                    deposit.stake_time = env.block.time.seconds();
                }
            }
            deposit
        })
        .collect::<Vec<StakeDeposit>>();

    //Return error if any
    if let Some(err) = error {
        return Err(ContractError::Std(err));
    }

    //Create rewards msgs
    let rewards_msgs = create_rewards_msgs(
        config.clone(),
        claimables,
        accrued_interest,
        info.clone().sender.to_string(),
        vec![],
    )?;

    //Save new Deposits
    STAKED.save(deps.storage, &restaked_deposits)?;

    Ok(Response::new().add_messages(rewards_msgs).add_attributes(vec![
        attr("method", "restake"),
        attr("restake_amount", initial_restake),
    ]))
}

/// Sends available claims to info.sender or as specified in send_to.
/// If claim_as is passed, the claims will be sent as said asset.
/// If restake is true, the accrued ownership will be restaked.
pub fn claim_rewards(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    claim_as_native: Option<String>,
    send_to: Option<String>,
    restake: bool,
) -> Result<Response, ContractError> {

    let config: Config = CONFIG.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg>;
    let accrued_interest: Uint128;
    let user_claimables: Vec<Asset>;

    //Get user claim msgs and accrued interest
    (messages, user_claimables, accrued_interest) = user_claims(
        deps.storage,
        deps.api,
        env.clone(),
        config.clone(),
        info.clone(),
        config.clone().dex_router,
        claim_as_native.clone(),
        send_to.clone(),
    )?;    

    //Create MBRN Mint Msg
    if config.osmosis_proxy.is_some() {
        if info.sender != config.clone().vesting_contract.unwrap_or_else(|| Addr::unchecked("")) && !accrued_interest.is_zero() {
            //Who to send to?
            if send_to.is_some() {
                let valid_recipient = deps.api.addr_validate(&send_to.clone().unwrap())?;

                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom: config.mbrn_denom,
                        amount: accrued_interest,
                        mint_to_address: valid_recipient.to_string(),
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

    //Error if there is nothing to claim
    if messages.is_empty() {
        return Err(ContractError::CustomError {
            val: String::from("Nothing to claim"),
        });
    }

    let user_claimables_string: Vec<String> = user_claimables
        .into_iter()
        .map(|claims| claims.to_string())
        .collect::<Vec<String>>();

    let res = Response::new()
        .add_attribute("method", "claim")
        .add_attribute("user", info.sender)
        .add_attribute("claim_as_native", claim_as_native.unwrap_or_else(|| String::from("None")))
        .add_attribute("send_to", send_to.unwrap_or_else(|| String::from("None")))
        .add_attribute("restake", restake.to_string())
        .add_attribute("mbrn_rewards", accrued_interest.to_string())
        .add_attribute("fee_rewards", format!("{:?}", user_claimables_string));

    Ok(res.add_messages(messages))
}

/// Deposit assets for staking rewards
fn deposit_fee(
    deps: DepsMut,
    env: Env,
    fee_assets: Vec<Asset>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut messages: Vec<CosmosMsg> = vec![];

    //Create response attribute
    let string_fee_assets = fee_assets.clone()
        .into_iter()
        .map(|asset| asset.to_string())
        .collect::<Vec<String>>();

    //Get CDT denom
    let basket: Basket = deps.querier.query_wasm_smart(
        config.positions_contract.unwrap_or_else(|| Addr::unchecked("")), 
        &CDP_QueryMsg::GetBasket{ })?;
    let cdt_denom = basket.credit_asset.info;

    //If fee asset isn't CDT, send to Fee Auction if the contract is set
    let non_CDT_assets = fee_assets.clone()
        .into_iter()
        .filter(|fee_asset| fee_asset.info != cdt_denom)
        .collect::<Vec<Asset>>();
    
    //Act if there are non-CDT assets
    if non_CDT_assets.len() != 0 {
        if let Some(auction_contract) = config.auction_contract {
            //Create auction msgs
            for asset in non_CDT_assets.clone() {
                let message: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: auction_contract.to_string(),
                    msg: to_binary(&AuctionExecuteMsg::StartAuction { 
                        repayment_position_info: None, 
                        send_to: None, 
                        auction_asset: asset.clone(),
                    })?,
                    funds: vec![asset_to_coin(asset)?],
                });

                messages.push(message);
            }
        }
    }

    //Remove non-CDT assets from fee assets
    let CDT_assets = fee_assets.clone()
        .into_iter()
        .filter(|fee_asset| fee_asset.info == cdt_denom)
        .collect::<Vec<Asset>>();

    //Load Fee Events
    let mut fee_events = FEE_EVENTS.load(deps.storage)?;

    //Load Total staked
    let mut totals = TOTALS.load(deps.storage)?;

    //Update vesting total
    if let Some(vesting_contract) = config.vesting_contract {        
        let vesting_total = get_total_vesting(deps.querier, vesting_contract.to_string())?;

        totals.vesting_contract = vesting_total;
        TOTALS.save(deps.storage, &totals)?;
    }

    //Set total
    let mut total = totals.vesting_contract + totals.stakers;
    if total.is_zero() {
        total = Uint128::new(1u128)
    }
    let decimal_total = Decimal::from_ratio(total, Uint128::new(1u128));
    
    //Add new Fee Event
    for asset in CDT_assets.clone() {        
        let amount = Decimal::from_ratio(asset.amount, Uint128::new(1u128));
        
        fee_events.push(FeeEvent {
            time_of_event: env.block.time.seconds(),
            fee: LiqAsset {
                //Amount = Amount per Staked MBRN
                info: asset.info,
                amount: decimal_division(amount, decimal_total)?,
            },
        });
    }

    FEE_EVENTS.save(deps.storage, &fee_events)?;
    
    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("method", "deposit_fee"),
        attr("fee_assets", format!("{:?}", string_fee_assets)),
    ]))
}

/// Create rewards msgs from claimables and accrued interest
fn create_rewards_msgs(
    config: Config,
    claimables: Vec<Asset>,
    accrued_interest: Uint128,
    user: String,
    mut native_claims: Vec<Coin>,
) -> StdResult<Vec<CosmosMsg>>{

    let mut msgs: Vec<CosmosMsg> = vec![];

    //Create msg for claimable fees
    if claimables != vec![] {
        //Aggregate native tokens
        for asset in claimables {
            match asset.clone().info {
                AssetInfo::Token { address: _ } => {
                    return Err(StdError::GenericErr { msg: String::from("Non-native token unclaimable") })
                }
                AssetInfo::NativeToken { denom: _ } => {
                    native_claims.push(asset_to_coin(asset)?);
                }
            }
        }
    }

    if native_claims != vec![] {
        let msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: user.clone(),
            amount: native_claims,
        });
        msgs.push(msg);
    }

    //Create msg to mint accrued interest
    if !accrued_interest.is_zero() {
        let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
            msg: to_binary(&OsmoExecuteMsg::MintTokens {
                denom: config.clone().mbrn_denom,
                amount: accrued_interest,
                mint_to_address: user,
            })?,
            funds: vec![],
        });
        msgs.push(message);
    }

    Ok(msgs)
}

/// Get deposit claims and add to list of claims/total interest
fn add_deposit_claimables(
    config: Config,
    incentive_schedule: StakeDistributionLog,
    env: Env,
    fee_events: Vec<FeeEvent>,
    deposit: StakeDeposit,
    claimables: &mut Vec<Asset>,
    accrued_interest: &mut Uint128,
) -> StdResult<()>{
    //Calc claimables from this deposit
    let (deposit_claimables, deposit_interest) = get_deposit_claimables(
        config.clone(),
        incentive_schedule.clone(),
        env.clone(),
        fee_events.clone(),
        deposit.clone(),
    )?;
    *accrued_interest += deposit_interest;

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

    Ok(())
}
/// Can't Unstake if...
/// 1. There is an active proposal by the address
/// 2. The address has voted for a proposal that has passed but not yet executed
pub fn can_this_addr_unstake(
    querier: QuerierWrapper,
    user: Addr,
    config: Config,
) -> Result<(), ContractError> {
    
    //Can't unstake if there is an active proposal by user
    let proposal_list: ProposalListResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart { 
        contract_addr: config.clone().governance_contract.unwrap().to_string(), 
        msg: to_binary(&Gov_QueryMsg::Proposals { start: None, limit: None })?
    }))?;

    for proposal in proposal_list.clone().proposal_list {
        if proposal.submitter == user && proposal.status == ProposalStatus::Active {
            return Err(ContractError::CustomError { val: String::from("Can't unstake while your proposal is active") })
        }
    }

    //Can't unstake if the user has voted for a proposal that has passed but not yet executed    
    //Get list of proposals that have passed & have executables
    for proposal in proposal_list.proposal_list {
        if proposal.status == ProposalStatus::Passed && proposal.messages.is_some() {
            //Get list of voters for this proposal
            let _voters: Vec<Addr> = querier.query_wasm_smart(
                config.clone().governance_contract.unwrap().to_string(), 
                &Gov_QueryMsg::ProposalVoters { 
                    proposal_id: proposal.proposal_id.into(), 
                    vote_option: membrane::governance::ProposalVoteOption::For, 
                    start: None, 
                    limit: None,
                    specific_user: Some(user.to_string())
                }
            )?;
            // if the query doesn't error then the user has voted For this proposal
            return Err(ContractError::CustomError { val: String::from("Can't unstake if the proposal you helped pass hasn't executed its messages yet") })
        }
    }

    Ok(())
}

/// Update deposits being withdrawn from.
/// Returns claimable assets, accrued interest, withdrawable amount.
fn withdraw_from_state(
    storage: &mut dyn Storage,
    env: Env,
    staker: Addr,
    mut withdrawal_amount: Uint128,
    fee_events: Vec<FeeEvent>,
) -> StdResult<(Vec<Asset>, Uint128, Uint128)> {
    let config = CONFIG.load(storage)?;
    let incentive_schedule = INCENTIVE_SCHEDULING.load(storage)?;
    let deposits = STAKED.load(storage)?;

    let mut new_deposit_total = Uint128::zero();
    let mut accrued_interest = Uint128::zero();
    let mut withdrawable_amount = Uint128::zero();
    
    let mut claimables: Vec<Asset> = vec![];
    let mut error: Option<StdError> = None;
    let mut this_deposit_is_withdrawable = false;

    let mut returning_deposit: Option<StakeDeposit> = None;

    //Iterate through deposits
    let mut new_deposits: Vec<StakeDeposit> = deposits
        .into_iter()
        .map(|mut deposit| {
            //Only edit user deposits
            if deposit.staker == staker {
                //If the deposit has started unstaking
                if let Some(deposit_unstake_start) = deposit.unstake_start_time {
                    //If the unstake period has been fulfilled
                    if env.block.time.seconds() - deposit_unstake_start
                        >= config.unstaking_period
                    {
                        this_deposit_is_withdrawable = true;
                    }
                }

                //Subtract from each deposit until there is none left to withdraw
                if withdrawal_amount != Uint128::zero() && deposit.amount > withdrawal_amount {
                    
                    //Add claimables from this deposit
                    match add_deposit_claimables(
                        config.clone(),
                        incentive_schedule.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                        &mut claimables,
                        &mut accrued_interest,
                    ) {
                        Ok(res) => res,
                        Err(err) => 
                            error = Some(err)                        
                    };

                    //If withdrawable...
                    //Set partial deposit total
                    //Set current deposit to 0
                    //Add withdrawal_amount to withdrawable_amount
                    if this_deposit_is_withdrawable {
                        new_deposit_total = deposit.amount - withdrawal_amount;
                        withdrawable_amount += deposit.amount;
                        deposit.amount = Uint128::zero();

                        this_deposit_is_withdrawable = false;
                    } else {
                        
                        //Since we claimed rewards
                        deposit.stake_time = env.block.time.seconds();                        
                        
                        //Set unstaking time for the amount getting withdrawn
                        //Create a StakeDeposit object for the amount not getting unstaked
                        //Set new deposit
                        returning_deposit = Some(StakeDeposit {
                            amount: deposit.amount - withdrawal_amount,
                            unstake_start_time: None,
                            ..deposit.clone()
                        });
                        
                        //Set new deposit amount
                        deposit.amount = withdrawal_amount;                       

                        //Set the unstaking_start_time and stake_time to now
                        deposit.unstake_start_time = Some(env.block.time.seconds());
                    }

                    //Zero withdrawal_amount since the deposit total fulfills the withdrawal
                    withdrawal_amount = Uint128::zero();

                } else if withdrawal_amount != Uint128::zero() && deposit.amount <= withdrawal_amount {

                    //Add claimables from this deposit
                    match add_deposit_claimables(
                        config.clone(),
                        incentive_schedule.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                        &mut claimables,
                        &mut accrued_interest,
                    ) {
                        Ok(res) => res,
                        Err(err) => 
                            error = Some(err)                        
                    };

                    //Since it's less than the Deposit amount, substract it from the withdrawal amount
                    withdrawal_amount -= deposit.amount;

                    //If withdrawable...
                    //Add deposit amount to withdrawable_amount
                    //Set current deposit to 0
                    if this_deposit_is_withdrawable {
                        withdrawable_amount += deposit.amount;
                        deposit.amount = Uint128::zero();

                        this_deposit_is_withdrawable = false;
                    } else {
                        //Else, Set the unstaking_start_time and stake_time to now
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

    //Push returning_deposit if Some
    //This can be done outside the loop bc it can only happen once
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


/// Calculates the accrued interest for a given stake
fn accumulate_interest(stake: Uint128, rate: Decimal, time_elapsed: u64) -> StdResult<Uint128> {
    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    let accrued_interest = stake * applied_rate;

    Ok(accrued_interest)
}

/// Return claim messages for a given user 
fn user_claims(
    storage: &mut dyn Storage,
    api: &dyn Api,
    env: Env,
    config: Config,
    info: MessageInfo,
    dex_router: Option<Addr>,
    claim_as_native: Option<String>,
    send_to: Option<String>,
) -> StdResult<(Vec<CosmosMsg>, Vec<Asset>, Uint128)> {

    let mut messages: Vec<CosmosMsg> = vec![];

    //Can only claim for oneself (info.sender)
    let (user_claimables, accrued_interest) =
        get_user_claimables(storage, env, info.clone().sender)?;

    //If we are claiming the available assets without swaps
    if claim_as_native.is_none() {
        //If we are sending to the sender
        if send_to.clone().is_none() {                
            //Send to sender
            let rewards_msgs = create_rewards_msgs(
                config.clone(), 
                user_claimables.clone(), 
                Uint128::zero(), //Dont send interest here
                info.clone().sender.to_string(), 
                vec![],
            )?;
            
            return Ok((rewards_msgs, user_claimables, accrued_interest))
        } else {
            //Validate recipient
            let valid_recipient = api.addr_validate(&send_to.clone().unwrap())?;

            //Send to recipient
            let rewards_msgs = create_rewards_msgs(
                config.clone(), 
                user_claimables.clone(), 
                Uint128::zero(), //Dont send interest here
                valid_recipient.to_string(), 
                vec![],
            )?;

            return Ok((rewards_msgs, user_claimables, accrued_interest))
        }
    } else if dex_router.is_some() {
        //Router usage
        for asset in user_claimables.clone() {
            match asset.info {
                AssetInfo::Token { address:_ } => { },
                /////Starting token is native so msgs go straight to the router contract
                AssetInfo::NativeToken { denom: _ } => {
                    //Swap to native before sending or depositing
                    if claim_as_native.is_some() {
                        if send_to.clone().is_some() {
                            //Send to Optional recipient
                            let valid_recipient = api.addr_validate(&send_to.clone().unwrap())?;
                            //Create Native Router SwapMsgs
                            let swap_hook = RouterExecuteMsg::Swap {
                                to: SwapToAssetsInput::Single(AssetInfo::NativeToken {
                                    denom: claim_as_native.clone().unwrap(),
                                }),
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(valid_recipient.to_string()),
                                hook_msg: None,
                            };

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: config.clone().dex_router.unwrap().to_string(),
                                msg: to_binary(&swap_hook)?,
                                funds: vec![asset_to_coin(asset)?],
                            });

                            messages.push(message);
                        } else {
                            //Send to Staker
                            //Create Native Router SwapMsgs
                            let swap_hook = RouterExecuteMsg::Swap {
                                to: SwapToAssetsInput::Single(AssetInfo::NativeToken {
                                    denom: claim_as_native.clone().unwrap(),
                                }),
                                max_spread: Some(
                                    config
                                        .clone()
                                        .max_spread
                                        .unwrap_or_else(|| Decimal::percent(10)),
                                ),
                                recipient: Some(info.clone().sender.to_string()),
                                hook_msg: None,
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
            msg: String::from("Can't 'claim as' without a DEX router"),
        });
    }
    
    Ok((messages, user_claimables, accrued_interest))
}

/// Return user claimables for a given user
fn get_user_claimables(
    storage: &mut dyn Storage,
    env: Env,
    staker: Addr,
) -> StdResult<(Vec<Asset>, Uint128)> {

    //Load state
    let config = CONFIG.load(storage)?;
    let incentive_schedule = INCENTIVE_SCHEDULING.load(storage)?;

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

    //Get claimables per deposit
    for deposit in deposits {
        add_deposit_claimables(
            config.clone(), 
            incentive_schedule.clone(), 
            env.clone(), 
            fee_events.clone(), 
            deposit.clone(), 
            &mut claimables, 
            &mut accrued_interest
        )?;

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

/// Trim fee events to only include events after the earliest deposit
fn trim_fee_events(
    storage: &mut dyn Storage,
    info: MessageInfo,
) -> Result<Response, ContractError>{

    let config = CONFIG.load(storage)?;

    if info.sender != config.owner { return Err( ContractError::Unauthorized {  } )}

    let mut fee_events = FEE_EVENTS.load(storage)?;
    let stakers = STAKED.load(storage)?;

    //Filter for fee events that are after the earliest deposit to trim state
    if stakers != vec![] {
        fee_events = fee_events.clone()
            .into_iter()
            .filter(|event| event.time_of_event > stakers[0].stake_time)
            .collect::<Vec<FeeEvent>>();
    }
    //In a situation where no one is staked the contract will need to be upgraded to handle its assets
    //This won't happen as long as the builder's allocation is vesting so the functionality isn't necessary rn
    
    //Save Fee events
    FEE_EVENTS.save(storage, &fee_events)?;

    Ok(Response::new().add_attribute("trimmed", "true"))
}

/// Get deposit's claimable fee assets based on which FeeEvents it experienced
pub fn get_deposit_claimables(
    mut config: Config,
    incentive_schedule: StakeDistributionLog,
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
        //Check if asset is already in the list of claimables and add accordingly
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

    //Assert staking rate is still active, if not set to 0
    let rate_duration = incentive_schedule.ownership_distribution.duration * SECONDS_PER_DAY;
    if env.block.time.seconds() - incentive_schedule.start_time > rate_duration {
        config.incentive_schedule.rate = Decimal::zero();
    }

    //Calc MBRN denominated rewards
    let deposit_interest = if !config.incentive_schedule.rate.is_zero() {
        let time_elapsed = env.block.time.seconds() - deposit.stake_time;
        accumulate_interest(deposit.amount, config.incentive_schedule.rate, time_elapsed)?
    } else {
        Uint128::zero()
    };

    Ok((claimables, deposit_interest))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
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
        QueryMsg::IncentiveSchedule {  } => to_binary(&INCENTIVE_SCHEDULING.load(deps.storage)?),
    }
}

