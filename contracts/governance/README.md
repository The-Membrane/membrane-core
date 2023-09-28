Fork of: https://github.com/astroport-fi/astroport-governance/tree/main/contracts/assembly

Changes: 
- Count vesting staked tokens differently than normal users
- Change deposit requirement to stake requirement
- Add proposal voting period minimum for proposals with executables
- Minimum total staked to submit proposals
- Vesting contract can expedit proposals 
- Config can toggle quadratic voting
- Voter can change vote at any time
- Amend and Remove VoteOptions, remove removes the proposal during end_proposal()
- Proposals can be aligned with to make active if proposed by small holders, these are pending until aligned
