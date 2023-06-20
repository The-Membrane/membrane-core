#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_binary, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, QueryRequest, Response, StdResult, Uint128, WasmMsg, WasmQuery, QuerierWrapper, Storage, Coin, BankMsg,
};
use cw2::set_contract_version;

use membrane::vesting::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::governance::{ExecuteMsg as GovExecuteMsg, ProposalMessage, ProposalVoteOption};
use membrane::math::decimal_division;
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::staking::{
    ExecuteMsg as StakingExecuteMsg, QueryMsg as StakingQueryMsg, RewardsResponse, StakerResponse,
};
use membrane::types::{Allocation, Asset, VestingPeriod, Recipient, AssetInfo};
use membrane::helpers::asset_to_coin;

use crate::error::ContractError;
use crate::query::{query_allocation, query_unlocked, query_recipients, query_recipient};
use crate::state::{CONFIG, RECIPIENTS, OWNERSHIP_TRANSFER};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:vesting";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_IN_A_DAY: u64 = 86400u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config = Config {
        owner: info.sender,
        total_allocation: msg.initial_allocation,
        mbrn_denom: msg.mbrn_denom,
        osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
        staking_contract: deps.api.addr_validate(&msg.staking_contract)?,
    };

    //Set Optionals
    if let Some(address) = msg.owner{
        config.owner = deps.api.addr_validate(&address)?;
    };

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    //Save Recipients w/ the pre_launch_contributors as the first Recipient
    RECIPIENTS.save(deps.storage, &vec![
        Recipient { 
            recipient: deps.api.addr_validate(&msg.pre_launch_contributors)?, 
            allocation: Some(Allocation { 
                amount: msg.initial_allocation, 
                amount_withdrawn: Uint128::zero(), 
                start_time_of_allocation: env.block.time.seconds(), 
                vesting_period: VestingPeriod { cliff: 730, linear: 365 },
            }), 
            claimables: vec![], 
        }
    ])?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
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
        ExecuteMsg::AddRecipient { recipient } => add_recipient(deps, info, recipient),
        ExecuteMsg::RemoveRecipient { recipient } => remove_recipient(deps, info, recipient),
        ExecuteMsg::AddAllocation {
            recipient,
            allocation,
            vesting_period,
        } => add_allocation(deps, env, info, recipient, allocation, vesting_period),
        ExecuteMsg::WithdrawUnlocked {} => withdraw_unlocked(deps, env, info),
        ExecuteMsg::ClaimFeesforContract {} => claim_fees_for_contract(deps.storage, deps.querier, env),
        ExecuteMsg::ClaimFeesforRecipient {} => claim_fees_for_recipient(deps, info),
        ExecuteMsg::SubmitProposal {
            title,
            description,
            link,
            messages,
            expedited
        } => submit_proposal(deps, info, title, description, link, messages, expedited),
        ExecuteMsg::CastVote { proposal_id, vote } => cast_vote(deps, info, proposal_id, vote),
        ExecuteMsg::UpdateConfig {
            owner,
            mbrn_denom,
            osmosis_proxy,
            staking_contract,
            additional_allocation,
        } => update_config(
            deps,
            info,
            owner,
            mbrn_denom,
            osmosis_proxy,
            staking_contract,
            additional_allocation
        ),
    }
}

/// Calls the Governance contract SubmitProposalMsg
fn submit_proposal(
    deps: DepsMut,
    info: MessageInfo,
    title: String,
    description: String,
    link: Option<String>,
    messages: Option<Vec<ProposalMessage>>,
    expedited: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let recipients = RECIPIENTS.load(deps.storage)?;

    match recipients
        .into_iter()
        .find(|recipient| recipient.recipient == info.sender)
    {
        Some(recipient) => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.owner.to_string(),
                msg: to_binary(&GovExecuteMsg::SubmitProposal {
                    title,
                    description,
                    link,
                    messages,
                    recipient: Some(recipient.recipient.to_string()),
                    expedited,
                })?,
                funds: vec![],
            });

            Ok(Response::new()
                .add_attributes(vec![
                    attr("method", "submit_proposal"),
                    attr("proposer", recipient.recipient.to_string()),
                ])
                .add_message(message))
        }
        None => Err(ContractError::InvalidRecipient {}),
    }
}

/// Calls the Governance contract CastVoteMsg
fn cast_vote(
    deps: DepsMut,
    info: MessageInfo,
    proposal_id: u64,
    vote: ProposalVoteOption,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let recipients = RECIPIENTS.load(deps.storage)?;

    match recipients
        
        .into_iter()
        .find(|recipient| recipient.recipient == info.sender)
    {
        Some(recipient) => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.owner.to_string(),
                msg: to_binary(&GovExecuteMsg::CastVote {
                    proposal_id,
                    vote,
                    recipient: Some(recipient.recipient.to_string()),
                })?,
                funds: vec![],
            });

            Ok(Response::new()
                .add_attributes(vec![
                    attr("method", "cast_vote"),
                    attr("voter", recipient.recipient.to_string()),
                ])
                .add_message(message))
        }
        None => Err(ContractError::InvalidRecipient {}),
    }
}


/// Claim a Recipient's proportion of staking rewards that were previously claimed using ClaimFeesForContract
fn claim_fees_for_recipient(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {

    //Load recipients
    let mut recipients = RECIPIENTS.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut claimables: Vec<Coin> = vec![];

    //Find Recipient claimables
    match recipients
        .clone()
        .into_iter()
        .enumerate()
        .find(|(_i, recipient)| recipient.recipient == info.sender)
    {
        Some((i, recipient)) => {
            if recipient.claimables == vec![] {
                return Err(ContractError::CustomError {
                    val: String::from("Nothing to claim"),
                });
            }

            //Aggregate native claims
            for claimable in recipient.clone().claimables {
                if let AssetInfo::NativeToken { denom: _ } = claimable.info {
                    //Add Asset as Coin
                    claimables.push(asset_to_coin(claimable.clone())?);
                }     
            }
            //Remove claimables from recipient
            recipients[i].claimables = vec![];            

            //Create withdraw msg for all native tokens
            messages.push(
                CosmosMsg::Bank(BankMsg::Send {
                    to_address: recipient.clone().recipient.to_string(),
                    amount: claimables.clone(),
                })
            );
        }
        None => return Err(ContractError::InvalidRecipient {}),
    }
    //Save Edited claims
    RECIPIENTS.save(deps.storage, &recipients)?;

    //Claimables into String List
    let claimables_string: Vec<String> = claimables
        .into_iter()
        .map(|claim| claim.to_string())
        .collect::<Vec<String>>();

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("method", "claim_fees_for_recipient"),
        attr("claimables", format!("{:?}", claimables_string)),
    ]))
}

/// Claim staking rewards for allocated MBRN
fn claim_fees_for_contract(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
) -> Result<Response, ContractError> {
    //Load Config
    let config = CONFIG.load(storage)?;

    //Query Rewards
    let res: RewardsResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&StakingQueryMsg::UserRewards {
            user: env.contract.address.to_string(),
        })?,
    }))?;

    //Split rewards w/ recipients based on allocation amounts
    if res.claimables != vec![] {
        let recipients = RECIPIENTS.load(storage)?;

        let mut allocated_recipients: Vec<Recipient> = recipients
            .clone()
            .into_iter()
            .filter(|recipient| recipient.allocation.is_some())
            .collect::<Vec<Recipient>>();

        //Calculate allocation ratios
        let allocation_ratios = get_allocation_ratios(querier, env.clone(), config.clone(), &mut allocated_recipients)?;
        
        //Add Recipient's ratio of each claim asset to position
        for claim_asset in res.clone().claimables {
            for (i, recipient) in allocated_recipients.clone().into_iter().enumerate() {
                match recipient
                    .clone()
                    .claimables
                    .into_iter()
                    .enumerate()
                    .find(|(_index, claim)| claim.info == claim_asset.info)
                {
                    //If found in claimables, add amount to position
                    Some((index, _claim)) => {
                        allocated_recipients[i].claimables[index].amount +=
                            claim_asset.amount * allocation_ratios[i]
                    }
                    //If None, add asset as if new
                    None => allocated_recipients[i].claimables.push(Asset {
                        amount: claim_asset.amount * allocation_ratios[i],
                        ..claim_asset.clone()
                    }),
                }
            }
        }

        //Filter out, Extend, Save
        let mut new_recipients: Vec<Recipient> = recipients
            
            .into_iter()
            .filter(|recipient| recipient.allocation.is_none())
            .collect::<Vec<Recipient>>();
        new_recipients.extend(allocated_recipients);
        RECIPIENTS.save(storage, &new_recipients)?;
    }

    //Construct ClaimRewards Msg to Staking Contract
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&StakingExecuteMsg::ClaimRewards {
            restake: false,
            send_to: None,
        })?,
        funds: vec![],
    });

    //Claimables into String List
    let claimables_string: Vec<String> = res
        .claimables
        .into_iter()
        .map(|claim| claim.to_string())
        .collect::<Vec<String>>();

    Ok(Response::new().add_message(msg).add_attributes(vec![
        attr("method", "claim_fees_for_contract"),
        attr("claimables", format!("{:?}", claimables_string)),
    ]))
}

/// Get allocation ratios for list of recipients
/// Only used for allocated recipients
fn get_allocation_ratios(querier: QuerierWrapper, env: Env, config: Config, recipients: &mut Vec<Recipient>) -> StdResult<Vec<Decimal>> {
    let mut allocation_ratios: Vec<Decimal> = vec![];

    //Get Contract's MBRN staked amount
    let res: StakerResponse = querier.query_wasm_smart(
        config.staking_contract, 
        &StakingQueryMsg::UserStake { staker: env.contract.address.to_string() }
    )?;
    let staked_mbrn = res.total_staked;

    for recipient in recipients.clone() {
        //Initialize allocation 
        let allocation = recipient.clone().allocation.unwrap();
        
        //Ratio of base Recipient's allocation.amount to total_staked
        allocation_ratios.push(decimal_division(
            Decimal::from_ratio(
                allocation.amount,
                Uint128::new(1u128),
            ),
            Decimal::from_ratio(staked_mbrn, Uint128::new(1u128)),
        )?);
    }
    

    Ok(allocation_ratios)
}

/// Update contract configuration
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    mbrn_denom: Option<String>,
    osmosis_proxy: Option<String>,
    staking_contract: Option<String>,    
    additional_allocation: Option<Uint128>,
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

    if let Some(owner) = owner {
        let valid_addr = deps.api.addr_validate(&owner)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));  
    };
    if let Some(osmosis_proxy) = osmosis_proxy {
        config.osmosis_proxy = deps.api.addr_validate(&osmosis_proxy)?;
    };
    if let Some(mbrn_denom) = mbrn_denom {
        config.mbrn_denom = mbrn_denom.clone();
    };
    if let Some(staking_contract) = staking_contract {
        config.staking_contract = deps.api.addr_validate(&staking_contract)?;
    };
    if let Some(additional_allocation) = additional_allocation {
        config.total_allocation += additional_allocation;
    };

    CONFIG.save(deps.storage, &config)?;
    attrs.push(attr("updated_config", format!("{:?}", config)));

    Ok(Response::new().add_attributes(attrs))
}


/// Withdraw unvested MBRN by minting the unlocked quantity
fn withdraw_unlocked(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let recipients = RECIPIENTS.load(deps.storage)?;

    let message: CosmosMsg;
    let unlocked_amount: Uint128;
    let new_allocation: Allocation;

    //Find Recipient
    match recipients
        .clone()
        .into_iter()
        .find(|recipient| recipient.recipient == info.sender)
    {
        Some(mut recipient) => {
            if recipient.allocation.is_some() {
                (unlocked_amount, new_allocation) =
                    get_unlocked_amount(recipient.allocation, env.block.time.seconds())?;

                //Save new allocation
                recipient.allocation = Some(new_allocation);

                let mut new_recipients = recipients
                    .into_iter()
                    .filter(|recipient| recipient.recipient != info.sender)
                    .collect::<Vec<Recipient>>();
                new_recipients.push(recipient.clone());

                RECIPIENTS.save(deps.storage, &new_recipients)?;

                //Mint the unlocked amount
                //Mint will error if 0
                message = CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: config.osmosis_proxy.to_string(), 
                    msg: to_binary(&OsmoExecuteMsg::MintTokens { 
                        denom: config.mbrn_denom, 
                        amount: unlocked_amount, 
                        mint_to_address: info.sender.to_string(), 
                    })?, 
                    funds: vec![], 
                });
                
            } else {
                return Err(ContractError::InvalidAllocation {});
            }
        }
        None => return Err(ContractError::InvalidRecipient {}),
    };
    
    Ok(Response::new()
        .add_message(message)
        .add_attributes(vec![
            attr("method", "withdraw_unlocked"),
            attr("recipient", info.sender),
            attr("withdrawn_amount", String::from(unlocked_amount)),
        ])
    )
    
}

/// Get unvested amount 
pub fn get_unlocked_amount(
    //This is an option bc the Recipient's allocation is. Its existence is confirmed beforehand.
    allocation: Option<Allocation>, 
    current_block_time: u64, //in seconds
) -> StdResult<(Uint128, Allocation)> {
    let mut allocation = allocation.unwrap();
    let mut unlocked_amount = Uint128::zero();

    //Skip if allocation amount is 0
    if allocation.amount == Uint128::zero() {
        return Ok((unlocked_amount, allocation));
    }

    //Calculate unlocked amount
    let time_passed = current_block_time - allocation.clone().start_time_of_allocation;
    let cliff_in_seconds = allocation.clone().vesting_period.cliff * SECONDS_IN_A_DAY;

    //If cliff has been passed then calculate linear unlock
    if time_passed >= cliff_in_seconds {
        let time_passed_cliff = time_passed - cliff_in_seconds;
        let linear_in_seconds = allocation.clone().vesting_period.linear * SECONDS_IN_A_DAY;

        if time_passed_cliff < linear_in_seconds {
            //Unlock amount based off time into linear vesting period
            let ratio_unlocked = decimal_division(
                Decimal::from_ratio(Uint128::new(time_passed_cliff as u128), Uint128::new(1u128)),
                Decimal::from_ratio(Uint128::new(linear_in_seconds as u128), Uint128::new(1u128)),
            )?;

            let newly_unlocked: Uint128;
            //Partial unlock
            if !ratio_unlocked.is_zero() {
                newly_unlocked = (ratio_unlocked * allocation.clone().amount)
                    - allocation.clone().amount_withdrawn;
            }//Unlock nothing
            else {
                newly_unlocked = Uint128::zero();
            }

            unlocked_amount = newly_unlocked;

            //Edit Allocation object
            allocation.amount_withdrawn += newly_unlocked;
        } else {
            //Unlock full amount
            unlocked_amount = allocation.clone().amount - allocation.clone().amount_withdrawn;
            allocation.amount_withdrawn += allocation.clone().amount;
        }
    }

    Ok((unlocked_amount, allocation))
}

/// Add allocation to a Recipient or
/// an existing Recipient can divvy their allocation to add a new Recipient
fn add_allocation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: String,
    allocation: Uint128,
    vesting_period: Option<VestingPeriod>,
) -> Result<Response, ContractError> {
    //Run claim_fees_for_contract beforehand to accurately allot claims before new allocations
    let res = claim_fees_for_contract(deps.storage, deps.querier, env.clone())?;

    //Assert allocation is not 0
    if allocation.is_zero() {
        return Err(ContractError::InvalidAllocation {});
    }
    
    //Validate recipient
    let valid_recipient = deps.api.addr_validate(&recipient)?;

    let config = CONFIG.load(deps.storage)?;    

    match vesting_period {
        //If Some && from the contract owner, adds new allocation amount to a valid Recipient
        Some(vesting_period) => {
            //Valid contract caller
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            //Add allocation to a Recipient
            RECIPIENTS.update(
                deps.storage,
                |mut recipients| -> Result<Vec<Recipient>, ContractError> {
                    //Add allocation
                    recipients = recipients
                        .into_iter()
                        .map(|mut stored_recipient| {
                            if stored_recipient.recipient == valid_recipient.to_string() {
                                stored_recipient.allocation = Some(Allocation {
                                    amount: allocation,
                                    amount_withdrawn: Uint128::zero(),
                                    start_time_of_allocation: env.block.time.seconds(),
                                    vesting_period: vesting_period.clone(),
                                });
                            }
                            stored_recipient
                        })
                        .collect::<Vec<Recipient>>();

                    Ok(recipients)
                },
            )?;
        },
        //If None && called by an existing Recipient, subtract & delegate part of the allocation to the allotted recipient
        //Add new Recipient object for the new recipient 
        None => {
            //Initialize new_allocation
            let mut new_allocation: Option<Allocation> = None;

            //Add Recipient
            RECIPIENTS.update(
                deps.storage,
                |mut recipients| -> Result<Vec<Recipient>, ContractError> {                    
                    //Divvy info.sender's allocation
                    recipients = recipients
                        .into_iter()
                        .map(|mut stored_recipient| {
                            //Checking equality to info.sender
                            if stored_recipient.recipient == info.clone().sender && stored_recipient.allocation.is_some(){

                                //Initialize stored_allocation 
                                let mut stored_allocation = stored_recipient.allocation.unwrap();                               

                                //Decrease stored_allocation.amount & set new_allocation
                                stored_allocation.amount = match stored_allocation.amount.checked_sub(allocation){
                                    Ok(diff) => {
                                    
                                        //Set new_allocation
                                        new_allocation = Some(
                                            Allocation { 
                                                amount: allocation, 
                                                amount_withdrawn: Uint128::zero(),
                                                ..stored_allocation.clone()
                                            }
                                        );

                                        diff
                                    },
                                    Err(_err) => {
                                        //Set new_allocation
                                        new_allocation = Some(
                                            Allocation { 
                                                amount: stored_allocation.amount, 
                                                amount_withdrawn: Uint128::zero(),
                                                ..stored_allocation.clone()
                                            }
                                        );
                                    
                                        Uint128::zero()
                                    }
                                };                                                                
                                
                                stored_recipient.allocation = Some(stored_allocation);
                            }

                            stored_recipient
                        })
                        .collect::<Vec<Recipient>>();
                    
                    if recipients
                        .iter()
                        .any(|recipient| recipient.recipient == valid_recipient)
                    {
                        return Err(ContractError::CustomError {
                            val: String::from("Duplicate Recipient"),
                        });
                    }

                    recipients.push(Recipient {
                        recipient: valid_recipient,
                        allocation: new_allocation,
                        claimables: vec![],
                    });

                    Ok(recipients)
                },
            )?;            
        },
    };

    //Get allocation total
    let mut allocation_total: Uint128 = Uint128::zero();
    for recipient in RECIPIENTS.load(deps.storage)?.into_iter() {
         if recipient.allocation.is_some() {
             allocation_total += recipient.allocation.unwrap().amount;
         }
    }

    //Error if over allocating
    if allocation_total > config.total_allocation {
        return Err(ContractError::OverAllocated {});
    }

    Ok(res.add_attributes(vec![
        attr("method", "increase_allocation"),
        attr("recipient", recipient),
        attr("allocation_increase", String::from(allocation)),
    ]))
}


/// Add new Recipient
fn add_recipient(
    deps: DepsMut,
    info: MessageInfo,
    recipient: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let valid_recipient = deps.api.addr_validate(&recipient)?;
    //Add new Recipient
    RECIPIENTS.update(
        deps.storage,
        |mut recipients| -> Result<Vec<Recipient>, ContractError> {
            if recipients
                .iter()
                .any(|recipient| recipient.recipient == valid_recipient)
            {
                return Err(ContractError::CustomError {
                    val: String::from("Duplicate Recipient"),
                });
            }

            recipients.push(Recipient {
                recipient: valid_recipient,
                allocation: None,
                claimables: vec![],
            });

            Ok(recipients)
        },
    )?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "add_recipient"),
        attr("Recipient", recipient),
    ]))
}

/// Remove existing Recipient
fn remove_recipient(
    deps: DepsMut,
    info: MessageInfo,
    recipient: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    //Remove Recipient
    RECIPIENTS.update(
        deps.storage,
        |recipients| -> Result<Vec<Recipient>, ContractError> {
            //Filter out Recipient and save
            Ok(recipients
                .into_iter()
                .filter(|stored_recipient| stored_recipient.recipient != recipient)
                .collect::<Vec<Recipient>>())
        },
    )?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "remove_recipient"),
        attr("Recipient", recipient),
    ]))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Allocation { recipient } => to_binary(&query_allocation(deps, recipient)?),
        QueryMsg::UnlockedTokens { recipient } => to_binary(&query_unlocked(deps, env, recipient)?),
        QueryMsg::Recipients {} => to_binary(&query_recipients(deps)?),
        QueryMsg::Recipient { recipient } => to_binary(&query_recipient(deps, recipient)?),
    }
}
