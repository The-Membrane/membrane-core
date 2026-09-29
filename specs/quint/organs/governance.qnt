// specs/quint/organs/governance.qnt
//
// Governance — proposal lifecycle, voting (For/Against/Amend/Remove/Align),
// quadratic voting, expedited proposals, vesting + delegation vote power.
//
// Source of truth (Rust):
//   contracts/governance/src/state.rs               (4 storage items)
//   contracts/governance/src/contracts.rs           (handlers; line cites below)
//   contracts/governance/src/error.rs               (error variants)
//   packages/membrane/src/governance.rs             (Config, ExecuteMsg, types)
module governance {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Constants
  // ================================================================
  pure val MIN_QUORUM_PCT:        Decimal = 330000000000000000
  pure val MIN_THRESHOLD_PCT:     Decimal = 510000000000000000
  pure val MIN_VOTING_PERIOD_S:   int = 86400
  pure val MAX_VOTING_PERIOD_S:   int = 1209600
  pure val MAX_DELAY_S:           int = 86400
  pure val MIN_EXPIRATION_S:      int = 86400
  pure val MAX_EXPIRATION_S:      int = 1209600
  pure val MIN_STAKE:             Uint128 = 1000000000
  pure val MAX_STAKE:             Uint128 = 5000000000

  // ================================================================
  // Types
  // ================================================================
  type ProposalStatus =
    | PS_Active
    | PS_Pending
    | PS_Passed
    | PS_AmendmentDesired
    | PS_Rejected
    | PS_Executed
    | PS_Expired

  type VoteOption = VO_For | VO_Against | VO_Amend | VO_Remove | VO_Align

  type ProposalMessage = {
    order: int,
    msg_data: str,
  }

  type Proposal = {
    proposal_id: int,
    submitter: Addr,
    status: ProposalStatus,
    voting_power: Addr -> Uint128,
    aligned_power: Uint128,
    for_power: Uint128,
    against_power: Uint128,
    amendment_power: Uint128,
    removal_power: Uint128,
    aligned_voters: Set[Addr],
    for_voters: Set[Addr],
    against_voters: Set[Addr],
    amendment_voters: Set[Addr],
    removal_voters: Set[Addr],
    start_time: Timestamp,
    end_time: Timestamp,
    delayed_end_time: Timestamp,
    expiration_time: Timestamp,
    title: str,
    description: str,
    link: Option[str],
    messages: Option[List[ProposalMessage]],
    expedited: bool,
    expedited_extended: bool,
  }

  type Config = {
    owner: Addr,
    governance: Addr,
    mbrn_denom: Denom,
    minimum_total_stake: Uint128,
    staking_contract_addr: Addr,
    vesting_contract_addr: Addr,
    vesting_voting_power_multiplier: Decimal,
    proposal_voting_period: int,
    expedited_proposal_voting_period: int,
    proposal_effective_delay: int,
    proposal_expiration_period: int,
    proposal_required_stake: Uint128,
    proposal_required_quorum: Decimal,
    proposal_required_threshold: Decimal,
    whitelisted_links: Set[str],
    quadratic_voting: bool,
  }

  // ================================================================
  // State
  // ================================================================
  var config:             Config
  var proposal_count:     int
  var proposals:          int -> Proposal
  var pending_proposals:  int -> Proposal

  action init(cfg: Config): bool = all {
    config' = cfg,
    proposal_count' = 0,
    proposals' = Map(),
    pending_proposals' = Map(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure def total_votes(p: Proposal): Uint128 =
    p.for_power + p.against_power + p.amendment_power + p.removal_power

  pure def quadratic_apply(power: Uint128, q: bool): Uint128 =
    if (q) isqrt(power) else power

  pure def voted_already(p: Proposal, user: Addr): bool =
    p.for_voters.contains(user) or
    p.against_voters.contains(user) or
    p.amendment_voters.contains(user) or
    p.removal_voters.contains(user) or
    p.aligned_voters.contains(user)

  pure val EMPTY_PROPOSAL: Proposal = {
    proposal_id: 0,
    submitter: SELF,
    status: PS_Rejected,
    voting_power: Map(),
    aligned_power: 0,
    for_power: 0, against_power: 0,
    amendment_power: 0, removal_power: 0,
    aligned_voters: Set(),
    for_voters: Set(), against_voters: Set(),
    amendment_voters: Set(), removal_voters: Set(),
    start_time: 0, end_time: 0,
    delayed_end_time: 0, expiration_time: 0,
    title: "", description: "", link: None,
    messages: None,
    expedited: false, expedited_extended: false,
  }

  pure def add_for_vote(p: Proposal, sender: Addr, q_power: Uint128): Proposal = {
    proposal_id: p.proposal_id, submitter: p.submitter, status: p.status,
    voting_power: p.voting_power,
    aligned_power: p.aligned_power,
    for_power: p.for_power + q_power,
    against_power: p.against_power,
    amendment_power: p.amendment_power,
    removal_power: p.removal_power,
    aligned_voters: p.aligned_voters,
    for_voters: p.for_voters.union(Set(sender)),
    against_voters: p.against_voters,
    amendment_voters: p.amendment_voters,
    removal_voters: p.removal_voters,
    start_time: p.start_time, end_time: p.end_time,
    delayed_end_time: p.delayed_end_time, expiration_time: p.expiration_time,
    title: p.title, description: p.description, link: p.link,
    messages: p.messages,
    expedited: p.expedited, expedited_extended: p.expedited_extended,
  }

  pure def add_against_vote(p: Proposal, sender: Addr, q_power: Uint128): Proposal = {
    proposal_id: p.proposal_id, submitter: p.submitter, status: p.status,
    voting_power: p.voting_power,
    aligned_power: p.aligned_power,
    for_power: p.for_power,
    against_power: p.against_power + q_power,
    amendment_power: p.amendment_power,
    removal_power: p.removal_power,
    aligned_voters: p.aligned_voters,
    for_voters: p.for_voters,
    against_voters: p.against_voters.union(Set(sender)),
    amendment_voters: p.amendment_voters,
    removal_voters: p.removal_voters,
    start_time: p.start_time, end_time: p.end_time,
    delayed_end_time: p.delayed_end_time, expiration_time: p.expiration_time,
    title: p.title, description: p.description, link: p.link,
    messages: p.messages,
    expedited: p.expedited, expedited_extended: p.expedited_extended,
  }

  pure def add_amend_vote(p: Proposal, sender: Addr, q_power: Uint128): Proposal = {
    proposal_id: p.proposal_id, submitter: p.submitter, status: p.status,
    voting_power: p.voting_power,
    aligned_power: p.aligned_power,
    for_power: p.for_power,
    against_power: p.against_power,
    amendment_power: p.amendment_power + q_power,
    removal_power: p.removal_power,
    aligned_voters: p.aligned_voters,
    for_voters: p.for_voters,
    against_voters: p.against_voters,
    amendment_voters: p.amendment_voters.union(Set(sender)),
    removal_voters: p.removal_voters,
    start_time: p.start_time, end_time: p.end_time,
    delayed_end_time: p.delayed_end_time, expiration_time: p.expiration_time,
    title: p.title, description: p.description, link: p.link,
    messages: p.messages,
    expedited: p.expedited, expedited_extended: p.expedited_extended,
  }

  pure def add_remove_vote(p: Proposal, sender: Addr, q_power: Uint128): Proposal = {
    proposal_id: p.proposal_id, submitter: p.submitter, status: p.status,
    voting_power: p.voting_power,
    aligned_power: p.aligned_power,
    for_power: p.for_power,
    against_power: p.against_power,
    amendment_power: p.amendment_power,
    removal_power: p.removal_power + q_power,
    aligned_voters: p.aligned_voters,
    for_voters: p.for_voters,
    against_voters: p.against_voters,
    amendment_voters: p.amendment_voters,
    removal_voters: p.removal_voters.union(Set(sender)),
    start_time: p.start_time, end_time: p.end_time,
    delayed_end_time: p.delayed_end_time, expiration_time: p.expiration_time,
    title: p.title, description: p.description, link: p.link,
    messages: p.messages,
    expedited: p.expedited, expedited_extended: p.expedited_extended,
  }

  pure def add_align_vote(p: Proposal, sender: Addr, q_power: Uint128): Proposal = {
    proposal_id: p.proposal_id, submitter: p.submitter, status: p.status,
    voting_power: p.voting_power,
    aligned_power: p.aligned_power + q_power,
    for_power: p.for_power,
    against_power: p.against_power,
    amendment_power: p.amendment_power,
    removal_power: p.removal_power,
    aligned_voters: p.aligned_voters.union(Set(sender)),
    for_voters: p.for_voters,
    against_voters: p.against_voters,
    amendment_voters: p.amendment_voters,
    removal_voters: p.removal_voters,
    start_time: p.start_time, end_time: p.end_time,
    delayed_end_time: p.delayed_end_time, expiration_time: p.expiration_time,
    title: p.title, description: p.description, link: p.link,
    messages: p.messages,
    expedited: p.expedited, expedited_extended: p.expedited_extended,
  }

  pure def set_status(p: Proposal, s: ProposalStatus): Proposal = {
    proposal_id: p.proposal_id, submitter: p.submitter, status: s,
    voting_power: p.voting_power,
    aligned_power: p.aligned_power,
    for_power: p.for_power,
    against_power: p.against_power,
    amendment_power: p.amendment_power,
    removal_power: p.removal_power,
    aligned_voters: p.aligned_voters,
    for_voters: p.for_voters,
    against_voters: p.against_voters,
    amendment_voters: p.amendment_voters,
    removal_voters: p.removal_voters,
    start_time: p.start_time, end_time: p.end_time,
    delayed_end_time: p.delayed_end_time, expiration_time: p.expiration_time,
    title: p.title, description: p.description, link: p.link,
    messages: p.messages,
    expedited: p.expedited, expedited_extended: p.expedited_extended,
  }

  // When an expedited proposal is extended on quorum miss, end_time
  // shifts forward to (block_time + proposal_voting_period). The
  // downstream timestamps must also shift so the time-ordering invariant
  // (start <= end <= delayed_end <= expiration) holds. Caller passes
  // pre-computed values for delay/expiration.
  pure def set_status_and_extend(p: Proposal, s: ProposalStatus,
                                  new_end: Timestamp,
                                  new_delayed_end: Timestamp,
                                  new_expiration: Timestamp): Proposal = {
    proposal_id: p.proposal_id, submitter: p.submitter, status: s,
    voting_power: p.voting_power,
    aligned_power: p.aligned_power,
    for_power: p.for_power,
    against_power: p.against_power,
    amendment_power: p.amendment_power,
    removal_power: p.removal_power,
    aligned_voters: p.aligned_voters,
    for_voters: p.for_voters,
    against_voters: p.against_voters,
    amendment_voters: p.amendment_voters,
    removal_voters: p.removal_voters,
    start_time: p.start_time, end_time: new_end,
    delayed_end_time: new_delayed_end, expiration_time: new_expiration,
    title: p.title, description: p.description, link: p.link,
    messages: p.messages,
    expedited: p.expedited, expedited_extended: true,
  }

  // ================================================================
  // Execute actions
  // ================================================================

  // --- SubmitProposal (contracts.rs:147-278) ---
  action SubmitProposal(sender: Addr,
                         title: str,
                         description: str,
                         link: Option[str],
                         messages: Option[List[ProposalMessage]],
                         submitter_voting_power: Uint128,
                         total_system_stake: Uint128,
                         expedited: bool): bool = {
    val voting_period: int = if (expedited) config.expedited_proposal_voting_period
                              else config.proposal_voting_period
    val end_t: Timestamp = block_time + voting_period
    val delayed_end_t: Timestamp = end_t + config.proposal_effective_delay
    val expiration_t: Timestamp = delayed_end_t + config.proposal_expiration_period
    val pid: int = proposal_count + 1
    val initial_aligned: Uint128 = quadratic_apply(submitter_voting_power, config.quadratic_voting)
    val starts_active: bool = initial_aligned >= config.proposal_required_stake
    val proposal: Proposal = {
      proposal_id: pid,
      submitter: sender,
      status: if (starts_active) PS_Active else PS_Pending,
      voting_power: Map().put(sender, initial_aligned),
      aligned_power: initial_aligned,
      for_power: 0, against_power: 0,
      amendment_power: 0, removal_power: 0,
      aligned_voters: Set(sender),
      for_voters: Set(), against_voters: Set(),
      amendment_voters: Set(), removal_voters: Set(),
      start_time: block_time,
      end_time: end_t,
      delayed_end_time: delayed_end_t,
      expiration_time: expiration_t,
      title: title, description: description, link: link,
      messages: messages,
      expedited: expedited,
      expedited_extended: false,
    }
    all {
      total_system_stake >= config.minimum_total_stake,
      submitter_voting_power > 0,
      proposal_count' = pid,
      proposals' = if (starts_active) proposals.put(pid, proposal) else proposals,
      pending_proposals' = if (starts_active) pending_proposals
                           else pending_proposals.put(pid, proposal),
      config' = config,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- CastVote (contracts.rs:283-455) ---
  action CastVote(sender: Addr,
                   proposal_id: int,
                   vote: VoteOption,
                   voting_power: Uint128): bool = {
    val in_pending: bool = pending_proposals.keys().contains(proposal_id)
    val in_active: bool = proposals.keys().contains(proposal_id)
    val p_old: Proposal = if (in_active) proposals.get(proposal_id)
                          else if (in_pending) pending_proposals.get(proposal_id)
                          else EMPTY_PROPOSAL
    val q_power: Uint128 = quadratic_apply(voting_power, config.quadratic_voting)
    val p_new: Proposal = match vote {
      | VO_For => add_for_vote(p_old, sender, q_power)
      | VO_Against => add_against_vote(p_old, sender, q_power)
      | VO_Amend => add_amend_vote(p_old, sender, q_power)
      | VO_Remove => add_remove_vote(p_old, sender, q_power)
      | VO_Align => add_align_vote(p_old, sender, q_power)
    }
    val now_active: bool =
      p_new.status == PS_Pending and
      p_new.aligned_power >= config.proposal_required_stake
    val p_promoted: Proposal = if (now_active) set_status(p_new, PS_Active) else p_new
    val proposals_after: int -> Proposal =
      if (in_active or now_active) proposals.put(proposal_id, p_promoted)
      else proposals
    val pending_after: int -> Proposal =
      if (now_active)
        pending_proposals.keys().exclude(Set(proposal_id))
          .mapBy(k => pending_proposals.get(k))
      else if (in_pending)
        pending_proposals.put(proposal_id, p_promoted)
      else pending_proposals
    val align_only_for_pending: bool =
      p_old.status == PS_Active or vote == VO_Align
    all {
      voting_power > 0,
      in_pending or in_active,
      sender != p_old.submitter,
      block_time <= p_old.end_time,
      p_old.status == PS_Active or p_old.status == PS_Pending,
      not(voted_already(p_old, sender)),
      align_only_for_pending,
      proposals' = proposals_after,
      pending_proposals' = pending_after,
      config' = config, proposal_count' = proposal_count,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- EndProposal (contracts.rs:458-567) ---
  action EndProposal(sender: Addr,
                      proposal_id: int,
                      total_voting_power: Uint128): bool = {
    val has_proposal = proposals.keys().contains(proposal_id)
    val p_old: Proposal = if (has_proposal) proposals.get(proposal_id) else EMPTY_PROPOSAL
    val tv: Uint128 = total_votes(p_old)
    val quorum_num: Uint128 = tv + p_old.aligned_power
    val quorum_met: bool =
      decimal_from_ratio(quorum_num, total_voting_power) >= config.proposal_required_quorum
    val for_share: Decimal = if (tv == 0) 0 else decimal_from_ratio(p_old.for_power, tv)
    val amend_share: Decimal = if (tv == 0) 0
      else decimal_from_ratio(p_old.for_power + p_old.amendment_power, tv)
    val remove_share: Decimal = if (tv == 0) 0 else decimal_from_ratio(p_old.removal_power, tv)
    val passed: bool = for_share >= config.proposal_required_threshold
    val amend_desired: bool = (not(passed)) and amend_share >= config.proposal_required_threshold
    val removed: bool = remove_share >= config.proposal_required_threshold
    val should_extend: bool =
      (not(quorum_met)) and p_old.expedited and (not(p_old.expedited_extended))
    val new_end: Timestamp = block_time + config.proposal_voting_period
    val new_delayed_end: Timestamp = new_end + config.proposal_effective_delay
    val new_expiration: Timestamp = new_delayed_end + config.proposal_expiration_period
    val p_new: Proposal =
      if (should_extend) set_status_and_extend(p_old, PS_Active, new_end,
                                                  new_delayed_end, new_expiration)
      else if (not(quorum_met)) set_status(p_old, PS_Rejected)
      else if (passed) set_status(p_old, PS_Passed)
      else if (amend_desired) set_status(p_old, PS_AmendmentDesired)
      else if (removed) set_status(p_old, PS_Rejected)
      else set_status(p_old, PS_Rejected)
    all {
      has_proposal,
      p_old.status == PS_Active,
      block_time > p_old.end_time,
      proposals' = proposals.put(proposal_id, p_new),
      pending_proposals' = pending_proposals,
      config' = config, proposal_count' = proposal_count,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- ExecuteProposal (contracts.rs:570-612) ---
  action ExecuteProposal(sender: Addr, proposal_id: int): bool = {
    val has_proposal = proposals.keys().contains(proposal_id)
    val p_old: Proposal = if (has_proposal) proposals.get(proposal_id) else EMPTY_PROPOSAL
    val p_new: Proposal = set_status(p_old, PS_Executed)
    all {
      has_proposal,
      p_old.status == PS_Passed,
      block_time >= p_old.delayed_end_time,
      block_time <= p_old.expiration_time,
      proposals' = proposals.put(proposal_id, p_new),
      msg_queue' = msg_queue.append(Msg_Reply({ id: 0, data: "proposal_messages" })),
      pending_proposals' = pending_proposals,
      config' = config, proposal_count' = proposal_count,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- RemoveCompletedProposal (contracts.rs:928-980) ---
  action RemoveCompletedProposal(sender: Addr, proposal_id: int): bool = {
    val in_pending: bool = pending_proposals.keys().contains(proposal_id)
    val in_active: bool = proposals.keys().contains(proposal_id)
    val p: Proposal = if (in_active) proposals.get(proposal_id)
                      else if (in_pending) pending_proposals.get(proposal_id)
                      else EMPTY_PROPOSAL
    val expired: bool = block_time > p.expiration_time
    val rejected: bool = p.status == PS_Rejected
    val by_submitter: bool = sender == p.submitter
    val proposals_after: int -> Proposal =
      if (in_active) proposals.keys().exclude(Set(proposal_id))
                        .mapBy(k => proposals.get(k))
      else proposals
    val pending_after: int -> Proposal =
      if (in_pending) pending_proposals.keys().exclude(Set(proposal_id))
                        .mapBy(k => pending_proposals.get(k))
      else pending_proposals
    all {
      in_pending or in_active,
      p.status == PS_Expired or rejected or by_submitter or expired,
      proposals' = proposals_after,
      pending_proposals' = pending_after,
      config' = config, proposal_count' = proposal_count,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- UpdateConfig (contracts.rs:983-1058) ---
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    sender == SELF,
    new_cfg.proposal_required_quorum >= MIN_QUORUM_PCT,
    new_cfg.proposal_required_threshold >= MIN_THRESHOLD_PCT,
    new_cfg.proposal_voting_period >= MIN_VOTING_PERIOD_S,
    new_cfg.proposal_voting_period <= MAX_VOTING_PERIOD_S,
    new_cfg.expedited_proposal_voting_period >= MIN_VOTING_PERIOD_S,
    new_cfg.expedited_proposal_voting_period <= MAX_VOTING_PERIOD_S,
    new_cfg.proposal_effective_delay <= MAX_DELAY_S,
    new_cfg.proposal_expiration_period >= MIN_EXPIRATION_S,
    new_cfg.proposal_expiration_period <= MAX_EXPIRATION_S,
    new_cfg.proposal_required_stake >= MIN_STAKE,
    new_cfg.proposal_required_stake <= MAX_STAKE,
    new_cfg.whitelisted_links.size() > 0,
    config' = new_cfg,
    proposal_count' = proposal_count,
    proposals' = proposals,
    pending_proposals' = pending_proposals,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_proposal_count_monotone: bool =
    proposals.keys().forall(id => id >= 1 and id <= proposal_count) and
    pending_proposals.keys().forall(id => id >= 1 and id <= proposal_count)

  val inv_pending_below_threshold: bool =
    pending_proposals.keys().forall(id => {
      val p = pending_proposals.get(id)
      p.status == PS_Pending and p.aligned_power < config.proposal_required_stake
    })

  val inv_active_above_threshold: bool =
    proposals.keys().forall(id => {
      val p = proposals.get(id)
      not(p.status == PS_Active and p.aligned_power < config.proposal_required_stake)
    })

  val inv_no_self_voting: bool =
    proposals.keys().forall(id => {
      val p = proposals.get(id)
      not(p.for_voters.contains(p.submitter) or
           p.against_voters.contains(p.submitter) or
           p.amendment_voters.contains(p.submitter) or
           p.removal_voters.contains(p.submitter))
    })

  val inv_executed_no_expired: bool =
    proposals.keys().forall(id => {
      val p = proposals.get(id)
      p.status != PS_Executed or block_time <= p.expiration_time
    })

  val inv_time_ordering: bool =
    proposals.keys().forall(id => {
      val p = proposals.get(id)
      p.start_time <= p.end_time and
      p.end_time <= p.delayed_end_time and
      p.delayed_end_time <= p.expiration_time
    })

  val inv_quorum_threshold_bounded: bool =
    config.proposal_required_quorum >= MIN_QUORUM_PCT and
    config.proposal_required_threshold >= MIN_THRESHOLD_PCT and
    config.proposal_required_quorum <= DECIMAL_ONE and
    config.proposal_required_threshold <= DECIMAL_ONE

  val inv_voting_period_bounded: bool =
    config.proposal_voting_period >= MIN_VOTING_PERIOD_S and
    config.proposal_voting_period <= MAX_VOTING_PERIOD_S and
    config.expedited_proposal_voting_period >= MIN_VOTING_PERIOD_S and
    config.expedited_proposal_voting_period <= MAX_VOTING_PERIOD_S

  val inv_required_stake_bounded: bool =
    config.proposal_required_stake >= MIN_STAKE and
    config.proposal_required_stake <= MAX_STAKE

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_GOV: bool =
    inv_quorum_threshold_bounded and
    inv_voting_period_bounded and
    inv_required_stake_bounded and
    inv_owner_governance_distinct

  val safety: bool =
    inv_proposal_count_monotone and
    inv_pending_below_threshold and
    inv_active_above_threshold and
    inv_no_self_voting and
    inv_executed_no_expired and
    inv_time_ordering and
    inv_no_unilateral_capital_loss_path_GOV

  action step: bool = all {
    advance_time(1),
    config' = config,
    proposal_count' = proposal_count,
    proposals' = proposals,
    pending_proposals' = pending_proposals,
  }
}
