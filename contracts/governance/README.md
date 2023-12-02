Fork of: https://github.com/astroport-fi/astroport-governance/tree/main/contracts/assembly

Changes: 
- Vesting tokens voting power is reduced by a config multiplier
- Change deposit requirement to stake requirement
- Add proposal voting period minimum for proposals with executables
- Minimum total staked to submit proposals
- Vesting contract can expedit proposals 
- Config can toggle quadratic voting
- Voter can change vote at any time
- Amend and Remove VoteOptions, Remove removes the proposal during end_proposal()
- Proposals can be aligned with to make active if proposed by small holders, these are pending until aligned

v2 Changes:
- Add a submessage to check_messages that makes sure the staking queries necessary for governance don’t error on upgrade 
- VP calculations on porposal submission
- No quadratic voting for alignment
- Query delegations before calculating votes to minimize gas costs
- Quadratic uses uint_ceiling so that non-quadratic alignment votes are not less than a user's VP 
- Recipients can use the Gov contract to submit & vote 
- The sum of delegated Votes is equal to the delegations (i.e. individual quadratics for delegated votes)
- TVP uses the VP calculated on proposal submission + vesting recipients 