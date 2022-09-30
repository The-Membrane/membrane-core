//Fork of: https://github.com/astroport-fi/astroport-governance/tree/main/contracts/assembly

use cosmwasm_std::{
    attr, entry_point, to_binary, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, Order, QueryRequest, Response, StdResult, Uint128, Uint64, WasmMsg, WasmQuery,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;

use membrane::builder_vesting::{AllocationResponse, QueryMsg as BuildersQueryMsg};
use membrane::governance::helpers::validate_links;
use membrane::governance::{
    Config, ExecuteMsg, InstantiateMsg, Proposal, ProposalListResponse, ProposalMessage,
    ProposalResponse, ProposalStatus, ProposalVoteOption, ProposalVotesResponse, QueryMsg,
    UpdateConfig,
};
use membrane::staking::{
    ConfigResponse as StakingConfigResponse, QueryMsg as StakingQueryMsg, StakedResponse,
};

use std::str::FromStr;

use crate::error::ContractError;
use crate::state::{CONFIG, PROPOSALS, PROPOSAL_COUNT};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "mbrn-governance";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Default pagination constants
const DEFAULT_LIMIT: u32 = 10;
const MAX_LIMIT: u32 = 30;
const DEFAULT_VOTERS_LIMIT: u32 = 100;
const MAX_VOTERS_LIMIT: u32 = 250;

pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let mbrn_denom = deps
        .querier
        .query::<StakingConfigResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: deps
                .api
                .addr_validate(&msg.mbrn_staking_contract_addr)?
                .to_string(),
            msg: to_binary(&StakingQueryMsg::Config {})?,
        }))?
        .mbrn_denom;

    let config = Config {
        mbrn_denom,
        staking_contract_addr: deps.api.addr_validate(&msg.mbrn_staking_contract_addr)?,
        builders_contract_addr: deps.api.addr_validate(&msg.builders_contract_addr)?,
        builders_voting_power_multiplier: msg.builders_voting_power_multiplier,
        proposal_voting_period: msg.proposal_voting_period,
        expedited_proposal_voting_period: msg.expedited_proposal_voting_period,
        proposal_effective_delay: msg.proposal_effective_delay,
        proposal_expiration_period: msg.proposal_expiration_period,
        proposal_required_stake: msg.proposal_required_stake,
        proposal_required_quorum: Decimal::from_str(&msg.proposal_required_quorum)?,
        proposal_required_threshold: Decimal::from_str(&msg.proposal_required_threshold)?,
        whitelisted_links: msg.whitelisted_links,
    };

    config.validate()?;

    CONFIG.save(deps.storage, &config)?;

    PROPOSAL_COUNT.save(deps.storage, &Uint64::zero())?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::SubmitProposal {
            title,
            description,
            link,
            messages,
            receiver,
            expedited,
        } => submit_proposal(
            deps,
            env,
            info,
            title,
            description,
            link,
            messages,
            receiver,
            expedited,
        ),
        ExecuteMsg::CastVote {
            proposal_id,
            vote,
            receiver,
        } => cast_vote(deps, env, info, proposal_id, vote, receiver),
        ExecuteMsg::EndProposal { proposal_id } => end_proposal(deps, env, proposal_id),
        ExecuteMsg::ExecuteProposal { proposal_id } => execute_proposal(deps, env, proposal_id),
        ExecuteMsg::CheckMessages { messages } => check_messages(env, messages),
        ExecuteMsg::CheckMessagesPassed {} => Err(ContractError::MessagesCheckPassed {}),
        ExecuteMsg::RemoveCompletedProposal { proposal_id } => {
            remove_completed_proposal(deps, env, proposal_id)
        }
        ExecuteMsg::UpdateConfig(config) => update_config(deps, env, info, config),
    }
}

pub fn submit_proposal(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    title: String,
    description: String,
    link: Option<String>,
    messages: Option<Vec<ProposalMessage>>,
    receiver: Option<String>,
    expedited: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //If sender is Builder's Contract, toggle
    let mut builders: bool = false;
    if info.sender == config.builders_contract_addr {
        builders = true;
    }

    //Validate voting power
    let voting_power = calc_voting_power(
        deps.as_ref(),
        info.sender.to_string(),
        env.block.time.seconds(),
        builders,
        receiver.clone(),
    )?;

    if voting_power < config.proposal_required_stake {
        return Err(ContractError::InsufficientStake {});
    }

    // Update the proposal count
    let count = PROPOSAL_COUNT.update(deps.storage, |c| -> StdResult<_> {
        Ok(c.checked_add(Uint64::new(1))?)
    })?;

    let mut submitter: Option<Addr> = None;
    if let Some(receiver) = receiver.clone() {
        submitter = Some(deps.api.addr_validate(&receiver)?);
    }

    //Set end_block 
    let end_block: u64 = {
        if expedited {
            env.block.height + config.expedited_proposal_voting_period
        } else {
            env.block.height + config.proposal_voting_period
        }
    };    

    let proposal = Proposal {
        proposal_id: count,
        submitter: submitter.unwrap_or_else(|| info.sender.clone()),
        status: ProposalStatus::Active,
        for_power: Uint128::zero(),
        against_power: Uint128::zero(),
        for_voters: Vec::new(),
        against_voters: Vec::new(),
        start_block: env.block.height,
        start_time: env.block.time.seconds(),
        end_block,
        delayed_end_block: env.block.height
            + config.proposal_voting_period
            + config.proposal_effective_delay,
        expiration_block: env.block.height
            + config.proposal_voting_period
            + config.proposal_effective_delay
            + config.proposal_expiration_period,
        title,
        description,
        link,
        messages,
    };

    proposal.validate(config.whitelisted_links)?;

    PROPOSALS.save(deps.storage, count.to_string(), &proposal)?;

    Ok(Response::new().add_attributes(vec![
        attr("action", "submit_proposal"),
        attr(
            "submitter",
            receiver.unwrap_or_else(|| info.sender.to_string()),
        ),
        attr("proposal_id", count),
        attr(
            "proposal_end_height",
            (proposal.end_block).to_string(),
        ),
    ]))
}

pub fn cast_vote(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
    vote_option: ProposalVoteOption,
    receiver: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //If sender is Builder's Contract, toggle
    let mut builders: bool = false;
    if info.sender == config.builders_contract_addr {
        builders = true;
    }

    let mut proposal = PROPOSALS.load(deps.storage, proposal_id.to_string())?;

    if proposal.status != ProposalStatus::Active {
        return Err(ContractError::ProposalNotActive {});
    }

    //Can't vote on your own proposal
    if proposal.submitter == info.sender {
        return Err(ContractError::Unauthorized {});
    } else if let Some(receiver) = receiver.clone() {
        let receiver = deps.api.addr_validate(&receiver)?;

        if proposal.submitter == receiver {
            return Err(ContractError::Unauthorized {});
        }
    }

    if env.block.height > proposal.end_block {
        return Err(ContractError::VotingPeriodEnded {});
    }

    if proposal.for_voters.contains(&info.sender) || proposal.against_voters.contains(&info.sender)
    {
        return Err(ContractError::UserAlreadyVoted {});
    }

    let voting_power = calc_voting_power(
        deps.as_ref(),
        info.sender.to_string(),
        proposal.clone().start_time,
        builders,
        receiver.clone(),
    )?;

    if voting_power.is_zero() {
        return Err(ContractError::NoVotingPower {});
    }

    match vote_option {
        ProposalVoteOption::For => {
            proposal.for_power = proposal.for_power.checked_add(voting_power)?;
            proposal.for_voters.push(info.sender.clone());
        }
        ProposalVoteOption::Against => {
            proposal.against_power = proposal.against_power.checked_add(voting_power)?;
            proposal.against_voters.push(info.sender.clone());
        }
    };

    PROPOSALS.save(deps.storage, proposal_id.to_string(), &proposal)?;

    Ok(Response::new().add_attributes(vec![
        attr("action", "cast_vote"),
        attr("proposal_id", proposal_id.to_string()),
        attr("voter", receiver.unwrap_or_else(|| info.sender.to_string())),
        attr("vote", vote_option.to_string()),
        attr("voting_power", voting_power),
    ]))
}

pub fn end_proposal(deps: DepsMut, env: Env, proposal_id: u64) -> Result<Response, ContractError> {
    let mut proposal = PROPOSALS.load(deps.storage, proposal_id.to_string())?;

    if proposal.status != ProposalStatus::Active {
        return Err(ContractError::ProposalNotActive {});
    }

    if env.block.height <= proposal.end_block {
        return Err(ContractError::VotingPeriodNotEnded {});
    }

    let config = CONFIG.load(deps.storage)?;

    let for_votes = proposal.for_power;
    let against_votes = proposal.against_power;
    let total_votes = for_votes + against_votes;

    let total_voting_power =
        calc_total_voting_power_at(deps.as_ref(), proposal.clone().start_time)?;

    let mut proposal_quorum: Decimal = Decimal::zero();
    let mut proposal_threshold: Decimal = Decimal::zero();

    if !total_voting_power.is_zero() {
        proposal_quorum = Decimal::from_ratio(total_votes, total_voting_power);
    }

    if !total_votes.is_zero() {
        proposal_threshold = Decimal::from_ratio(for_votes, total_votes);
    }

    // Determine the proposal result
    proposal.status = if proposal_quorum >= config.proposal_required_quorum
        && proposal_threshold > config.proposal_required_threshold
    {
        ProposalStatus::Passed
    } else {
        ProposalStatus::Rejected
    };

    PROPOSALS.save(deps.storage, proposal_id.to_string(), &proposal)?;

    let response = Response::new().add_attributes(vec![
        attr("action", "end_proposal"),
        attr("proposal_id", proposal_id.to_string()),
        attr("proposal_result", proposal.status.to_string()),
    ]);

    Ok(response)
}

//Execute Proposal Msgs
pub fn execute_proposal(
    deps: DepsMut,
    env: Env,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    let mut proposal = PROPOSALS.load(deps.storage, proposal_id.to_string())?;

    if proposal.status != ProposalStatus::Passed {
        return Err(ContractError::ProposalNotPassed {});
    }

    if env.block.height < proposal.delayed_end_block {
        return Err(ContractError::ProposalDelayNotEnded {});
    }

    if env.block.height > proposal.expiration_block {
        return Err(ContractError::ExecuteProposalExpired {});
    }

    proposal.status = ProposalStatus::Executed;

    PROPOSALS.save(deps.storage, proposal_id.to_string(), &proposal)?;

    let messages = match proposal.messages {
        Some(mut messages) => {
            messages.sort_by(|a, b| a.order.cmp(&b.order));
            messages.into_iter().map(|message| message.msg).collect()
        }
        None => vec![],
    };

    Ok(Response::new()
        .add_attribute("action", "execute_proposal")
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_messages(messages))
}

/// Checks that proposal messages are correct.
/// Returns [`ContractError`] on failure, otherwise returns a [`Response`] with the specified
/// attributes. The last message will always fail to prevent committing into blockchain.
pub fn check_messages(
    env: Env,
    mut messages: Vec<ProposalMessage>,
) -> Result<Response, ContractError> {

    messages.sort_by(|a, b| a.order.cmp(&b.order));

    let mut messages: Vec<_> = messages.into_iter().map(|message| message.msg).collect();

    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_binary(&ExecuteMsg::CheckMessagesPassed {})?,
        funds: vec![],
    }));

    Ok(Response::new()
        .add_attribute("action", "check_messages")
        .add_messages(messages))
}

//Remove completed Proposals
pub fn remove_completed_proposal(
    deps: DepsMut,
    env: Env,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let mut proposal = PROPOSALS.load(deps.storage, proposal_id.to_string())?;

    if env.block.height
        > (proposal.end_block + config.proposal_effective_delay + config.proposal_expiration_period)
    {
        proposal.status = ProposalStatus::Expired;
    }

    if proposal.status != ProposalStatus::Expired && proposal.status != ProposalStatus::Rejected {
        return Err(ContractError::ProposalNotCompleted {});
    }

    PROPOSALS.remove(deps.storage, proposal_id.to_string());

    Ok(Response::new()
        .add_attribute("action", "remove_completed_proposal")
        .add_attribute("proposal_id", proposal_id.to_string()))
}

pub fn update_config(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    updated_config: UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    // Only the Governance contract is allowed to update its own parameters (through a successful proposal)
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(mbrn_denom) = updated_config.mbrn_denom {
        config.mbrn_denom = mbrn_denom;
    }

    if let Some(staking_contract) = updated_config.staking_contract {
        config.staking_contract_addr = deps.api.addr_validate(&staking_contract)?;
    }

    if let Some(builders_contract_addr) = updated_config.builders_contract_addr {
        config.builders_contract_addr = deps.api.addr_validate(&builders_contract_addr)?;
    }

    if let Some(builders_voting_power_multiplier) = updated_config.builders_voting_power_multiplier
    {
        config.builders_voting_power_multiplier = builders_voting_power_multiplier;
    }

    if let Some(proposal_voting_period) = updated_config.proposal_voting_period {
        config.proposal_voting_period = proposal_voting_period;
    }

    if let Some(expedited_proposal_voting_period) = updated_config.expedited_proposal_voting_period {
        config.expedited_proposal_voting_period = expedited_proposal_voting_period;
    }

    if let Some(proposal_effective_delay) = updated_config.proposal_effective_delay {
        config.proposal_effective_delay = proposal_effective_delay;
    }

    if let Some(proposal_expiration_period) = updated_config.proposal_expiration_period {
        config.proposal_expiration_period = proposal_expiration_period;
    }

    if let Some(proposal_required_stake) = updated_config.proposal_required_stake {
        config.proposal_required_stake = Uint128::from(proposal_required_stake);
    }

    if let Some(proposal_required_quorum) = updated_config.proposal_required_quorum {
        config.proposal_required_quorum = Decimal::from_str(&proposal_required_quorum)?;
    }

    if let Some(proposal_required_threshold) = updated_config.proposal_required_threshold {
        config.proposal_required_threshold = Decimal::from_str(&proposal_required_threshold)?;
    }

    if let Some(whitelist_add) = updated_config.whitelist_add {
        validate_links(&whitelist_add)?;

        config.whitelisted_links.append(
            &mut whitelist_add
                .into_iter()
                .filter(|link| !config.whitelisted_links.contains(link))
                .collect(),
        );
    }

    if let Some(whitelist_remove) = updated_config.whitelist_remove {
        config.whitelisted_links = config
            .whitelisted_links
            .into_iter()
            .filter(|link| !whitelist_remove.contains(link))
            .collect();

        if config.whitelisted_links.is_empty() {
            return Err(ContractError::WhitelistEmpty {});
        }
    }

    config.validate()?;

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

//Calc total voting power at a specific time
pub fn calc_total_voting_power_at(deps: Deps, start_time: u64) -> StdResult<Uint128> {
    let config = CONFIG.load(deps.storage)?;

    //Pulls stake from before Proposal's start_time
    let staked_mbrn = deps
        .querier
        .query::<StakedResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.staking_contract_addr.to_string(),
            msg: to_binary(&StakingQueryMsg::Staked {
                limit: None,
                start_after: None,
                end_before: Some(start_time),
                unstaking: false,
            })?,
        }))?
        .stakers;

    //Calc total voting power
    let total: Uint128;
    if staked_mbrn == vec![] {
        total = Uint128::zero()
    } else {
        total = staked_mbrn
            .into_iter()
            .map(|stake| {
                if stake.staker == config.builders_contract_addr {
                    stake.amount * config.builders_voting_power_multiplier
                } else {
                    stake.amount
                }
            })
            .collect::<Vec<Uint128>>()
            .into_iter()
            .sum();
    }

    Ok(total)
}

//Calc voting power for sender at a Popoosal's start_time
pub fn calc_voting_power(
    deps: Deps,
    sender: String,
    start_time: u64,
    builders: bool,
    receiver: Option<String>,
) -> StdResult<Uint128> {
    let config = CONFIG.load(deps.storage)?;

    //Pulls stake from before Proposal's start_time
    let staked_mbrn = deps
        .querier
        .query::<StakedResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.staking_contract_addr.to_string(),
            msg: to_binary(&StakingQueryMsg::Staked {
                limit: None,
                start_after: None,
                end_before: Some(start_time),
                unstaking: false,
            })?,
        }))?
        .stakers;

    let total: Uint128;
    //If calculating builder's voting power, we take from receiver's allocation
    if !builders {
        //Calc total voting power
        if staked_mbrn == vec![] {
            total = Uint128::zero()
        } else {
            total = staked_mbrn
                .into_iter()
                .map(|stake| {
                    if stake.staker == sender {
                        stake.amount
                    } else {
                        Uint128::zero()
                    }
                })
                .collect::<Vec<Uint128>>()
                .into_iter()
                .sum();
        }
    } else if receiver.is_some() {
        let receiver = receiver.unwrap();

        let allocation = deps
            .querier
            .query::<AllocationResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.builders_contract_addr.to_string(),
                msg: to_binary(&BuildersQueryMsg::Allocation { receiver })?,
            }))?;

        total = Uint128::from_str(&allocation.amount)? * config.builders_voting_power_multiplier;
    } else if builders {
        //If builder's but receiver isn't passed, use the sender
        let receiver = sender;

        let allocation = deps
            .querier
            .query::<AllocationResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.builders_contract_addr.to_string(),
                msg: to_binary(&BuildersQueryMsg::Allocation { receiver })?,
            }))?;

        total = Uint128::from_str(&allocation.amount)? * config.builders_voting_power_multiplier;
    } else {
        //This isn't necessary but fulfills the compiler
        total = Uint128::zero();
    }

    Ok(total)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Proposals { start, limit } => to_binary(&query_proposals(deps, start, limit)?),
        QueryMsg::Proposal { proposal_id } => to_binary(&query_proposal(deps, proposal_id)?),
        QueryMsg::ProposalVotes { proposal_id } => {
            to_binary(&query_proposal_votes(deps, proposal_id)?)
        }
        QueryMsg::UserVotingPower {
            user,
            proposal_id,
            builders,
        } => {
            let proposal = PROPOSALS.load(deps.storage, proposal_id.to_string())?;

            let user = deps.api.addr_validate(&user)?;

            to_binary(&calc_voting_power(
                deps,
                user.to_string(),
                proposal.start_time,
                builders,
                None,
            )?)
        }
        QueryMsg::TotalVotingPower { proposal_id } => {
            let proposal = PROPOSALS.load(deps.storage, proposal_id.to_string())?;
            to_binary(&calc_total_voting_power_at(deps, proposal.start_time)?)
        }
        QueryMsg::ProposalVoters {
            proposal_id,
            vote_option,
            start,
            limit,
        } => to_binary(&query_proposal_voters(
            deps,
            proposal_id,
            vote_option,
            start,
            limit,
        )?),
    }
}

pub fn query_proposal(deps: Deps, proposal_id: u64) -> StdResult<ProposalResponse> {
    let proposal = PROPOSALS.load(deps.storage, proposal_id.to_string())?;

    Ok(ProposalResponse {
        proposal_id: proposal.proposal_id,
        submitter: proposal.submitter,
        status: proposal.status,
        for_power: proposal.for_power,
        against_power: proposal.against_power,
        start_block: proposal.start_block,
        start_time: proposal.start_time,
        end_block: proposal.end_block,
        delayed_end_block: proposal.delayed_end_block,
        expiration_block: proposal.expiration_block,
        title: proposal.title,
        description: proposal.description,
        link: proposal.link,
        messages: proposal.messages,
    })
}

pub fn query_proposals(
    deps: Deps,
    start: Option<u64>,
    limit: Option<u32>,
) -> StdResult<ProposalListResponse> {
    let proposal_count = PROPOSAL_COUNT.load(deps.storage)?;

    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = start.map(|start| Bound::inclusive(start.to_string()));

    let proposal_list = PROPOSALS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            let (_, proposal) = item?;
            Ok(ProposalResponse {
                proposal_id: proposal.proposal_id,
                submitter: proposal.submitter,
                status: proposal.status,
                for_power: proposal.for_power,
                against_power: proposal.against_power,
                start_block: proposal.start_block,
                start_time: proposal.start_time,
                end_block: proposal.end_block,
                delayed_end_block: proposal.delayed_end_block,
                expiration_block: proposal.expiration_block,
                title: proposal.title,
                description: proposal.description,
                link: proposal.link,
                messages: proposal.messages,
            })
        })
        .collect::<StdResult<Vec<_>>>()?;

    Ok(ProposalListResponse {
        proposal_count,
        proposal_list,
    })
}

pub fn query_proposal_voters(
    deps: Deps,
    proposal_id: u64,
    vote_option: ProposalVoteOption,
    start: Option<u64>,
    limit: Option<u32>,
) -> StdResult<Vec<Addr>> {
    let limit = limit.unwrap_or(DEFAULT_VOTERS_LIMIT).min(MAX_VOTERS_LIMIT);
    let start = start.unwrap_or_default();

    let proposal = PROPOSALS.load(deps.storage, proposal_id.to_string())?;

    let voters = match vote_option {
        ProposalVoteOption::For => proposal.for_voters,
        ProposalVoteOption::Against => proposal.against_voters,
    };

    Ok(voters
        .iter()
        .skip(start as usize)
        .take(limit as usize)
        .cloned()
        .collect())
}

pub fn query_proposal_votes(deps: Deps, proposal_id: u64) -> StdResult<ProposalVotesResponse> {
    let proposal = PROPOSALS.load(deps.storage, proposal_id.to_string())?;

    Ok(ProposalVotesResponse {
        proposal_id,
        for_power: proposal.for_power,
        against_power: proposal.against_power,
    })
}
