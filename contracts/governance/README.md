Fork of: https://github.com/astroport-fi/astroport-governance/tree/main/contracts/assembly

WARNING: This contract can NOT be mutable if owned by itself or a contract it owns. Executed msgs currently clear the admin but if they don't then this stands.

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

v3 Changes: 
- Proposal submitter can remove the proposal 
- Execute Msgs end with the passed_messages query to ensure proposability- Change voting block window to use seconds bc the block times are getting shorter 
- Change voting_power calculations to individualized per vote
- Change check_msg staking queries to reflect new VP query usage
- If Staker query ever errors, make sure Gov can use delegations only or vesting only for proposals
-  Alignment votes are quadratic past the threshold & once calc'd for the quorum. squareRooting all at the end results in less VP than desired. (this solution is still imperfect bc a group of small stakers will lose more relative vp by aligning compared to 1 larger staker)
- Separate delegation vp calcs when adds/subs to total delegated vp
- Add freeze toggle
- Add ability to set chosen asset supply caps to 0
- Add a gov config query to the check_msgs so we can't inadvertingly choose the wrong code ID
