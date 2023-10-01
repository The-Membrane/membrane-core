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
