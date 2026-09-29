// specs/quint/tests/governance_test.qnt
//
// Governance scenarios — proposal lifecycle, voting, expedited extension.
module governance_test {
  import types.*     from "../lib/types"
  import money.*     from "../lib/money"
  import env.*       from "../lib/env"
  import governance.* from "../organs/governance"

  pure val OWNER: Addr = "owner"
  pure val GOV:   Addr = "governance"
  pure val ALICE: Addr = "alice"
  pure val BOB:   Addr = "bob"
  pure val CAROL: Addr = "carol"
  pure val STAKING: Addr = "staking"
  pure val VESTING: Addr = "vesting"

  pure val CFG: Config = {
    owner: OWNER, governance: GOV,
    mbrn_denom: "mbrn",
    minimum_total_stake: 1000000000000,
    staking_contract_addr: STAKING,
    vesting_contract_addr: VESTING,
    vesting_voting_power_multiplier: 200000000000000000,    // 20%
    proposal_voting_period: 432000,                          // 5 days
    expedited_proposal_voting_period: 86400,                 // 1 day
    proposal_effective_delay: 86400,
    proposal_expiration_period: 604800,                      // 7 days
    proposal_required_stake: 2000000000,                      // 2k MBRN
    proposal_required_quorum: 330000000000000000,             // 33%
    proposal_required_threshold: 510000000000000000,          // 51%
    whitelisted_links: Set("https://github.com/membrane-finance"),
    quadratic_voting: false,   // isqrt is naive 1..n; tests use linear voting power
  }

  action runInit: bool = init(CFG)

  action tick(dt: int): bool = all {
    advance_time(dt),
    config' = config,
    proposal_count' = proposal_count,
    proposals' = proposals,
    pending_proposals' = pending_proposals,
  }

  // ----------------------------------------------------------------
  // Scenario 1: submit + vote + end (passes)
  // ----------------------------------------------------------------
  run happy_path_pass =
    runInit
      .then(SubmitProposal(ALICE, "Test", "Test description", None, None,
                            10000000000000, 5000000000000000, false))
      .then(CastVote(BOB, 1, VO_For, 8000000000000))
      .then(tick(432001))
      .then(EndProposal(CAROL, 1, 100000000000000))
      .then(tick(86401))                             // delay
      .then(ExecuteProposal(BOB, 1))

  // ----------------------------------------------------------------
  // Scenario 2: pending → active alignment
  // ----------------------------------------------------------------
  run pending_align =
    runInit
      .then(SubmitProposal(ALICE, "Test", "Pending init", None, None,
                            500000000, 5000000000000000, false))     // below stake threshold
      .then(CastVote(BOB, 1, VO_Align, 5000000000))                  // pushes over threshold

  // ----------------------------------------------------------------
  // Scenario 3: expedited proposal misses quorum → extends to normal period
  // ----------------------------------------------------------------
  run expedited_extend =
    runInit
      .then(SubmitProposal(ALICE, "Urgent", "Quick", None, None,
                            10000000000000, 5000000000000000, true))
      .then(tick(86401))
      // Below quorum → extends to proposal_voting_period
      .then(EndProposal(CAROL, 1, 100000000000000))

  // ----------------------------------------------------------------
  // Scenario 4: self-voting forbidden
  // ----------------------------------------------------------------
  run self_vote_forbidden =
    runInit
      .then(SubmitProposal(ALICE, "Test", "T", None, None,
                            10000000000000, 5000000000000000, false))
      .then(CastVote(ALICE, 1, VO_For, 5000000000000))               // SHOULD revert

  // ----------------------------------------------------------------
  // Scenario 5: rejected proposal → can be removed
  // ----------------------------------------------------------------
  run rejected_remove =
    runInit
      .then(SubmitProposal(ALICE, "Bad", "Will fail", None, None,
                            10000000000000, 5000000000000000, false))
      .then(CastVote(BOB, 1, VO_Against, 100000000000000))
      .then(tick(432001))
      .then(EndProposal(CAROL, 1, 100000000000000))
      .then(RemoveCompletedProposal(ALICE, 1))                       // submitter removes
}
