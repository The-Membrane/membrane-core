
#[cfg(test)]
mod tests {
    use crate::contract::{query, instantiate, execute};

    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{from_binary, to_binary, CosmosMsg, SubMsg, Uint128, WasmMsg, Addr, coin, attr};

    use membrane::vesting::{Config, QueryMsg, InstantiateMsg, ExecuteMsg, RecipientResponse, AllocationResponse, UnlockedResponse, RecipientsResponse};
    use membrane::staking::ExecuteMsg as StakingExecuteMsg;
    use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
    use membrane::types::VestingPeriod;

    #[test]
    fn recipients() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            owner: Some(String::from("owner0000")),
            initial_allocation: Uint128::new(30_000_000_000_000u128),
            mbrn_denom: String::from("mbrn_denom"),
            osmosis_proxy: String::from("osmosis_proxy"),
            staking_contract: String::from("staking_contract"),
            labs_addr: String::from("labs")
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &[]);
        let _res = instantiate(deps.as_mut(), mock_env(), v_info, msg).unwrap();

        //AddRecipient
        let add_msg = ExecuteMsg::AddRecipient {
            recipient: String::from("recipient0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //Error: Duplicate Recipient
        let add_msg = ExecuteMsg::AddRecipient {
            recipient: String::from("recipient0000"),
        };
        let err = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            String::from("Custom Error val: \"Duplicate Recipient\"")
        );

        //RemoveRecipient
        let add_msg = ExecuteMsg::RemoveRecipient {
            recipient: String::from("recipient0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //AddRecipient
        let add_msg = ExecuteMsg::AddRecipient {
            recipient: String::from("recipient0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //Query Recipients
        let msg = QueryMsg::Recipients {};
        let res = query(deps.as_ref(), mock_env(), msg).unwrap();

        let resp: RecipientsResponse = from_binary(&res).unwrap();
        assert_eq!(resp.recipients[1].recipient, String::from("recipient0000"));
        assert_eq!(resp.recipients.len().to_string(), String::from("2"));
    }

    #[test]
    fn allocations() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            owner: Some(String::from("owner0000")),
            initial_allocation: Uint128::new(30_000_000_000_000u128),
            mbrn_denom: String::from("mbrn_denom"),
            osmosis_proxy: String::from("osmosis_proxy"),
            staking_contract: String::from("staking_contract"),
            labs_addr: String::from("labs")
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &[]);
        let _res = instantiate(deps.as_mut(), mock_env(), v_info, msg).unwrap();

        //Update Config: Increase allocations
        let msg = ExecuteMsg::UpdateConfig { 
            owner: None, 
            mbrn_denom: None,
            osmosis_proxy: None,
            staking_contract: None,
            additional_allocation: Some( Uint128::new(1_000_000_000_000u128) ),
        };
        let _res = execute( deps.as_mut(),mock_env(), mock_info("owner0000", &[]), msg )
        .unwrap();

        //AddRecipient
        let add_msg = ExecuteMsg::AddRecipient {
            recipient: String::from("recipient0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //AddAllocation: Unauthorized
        let allocation_msg = ExecuteMsg::AddAllocation {
            recipient: String::from(""),
            allocation: Uint128::new(0u128),
            vesting_period: Some(VestingPeriod {
                cliff: 365u64,
                linear: 365u64,
            }),
        };
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("not_an_owner", &[]),
            allocation_msg,
        )
        .unwrap_err();

        //AddAllocation
        let allocation_msg = ExecuteMsg::AddAllocation {
            recipient: String::from("recipient0000"),
            allocation: Uint128::new(1_000_000_000_000u128),
            vesting_period: Some(VestingPeriod {
                cliff: 365u64,
                linear: 365u64,
            }),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            allocation_msg,
        )
        .unwrap();

        //AddAllocation from the latest recipient to divvy their allocation
        let allocation_msg = ExecuteMsg::AddAllocation {
            recipient: String::from("sub_recipient"),
            allocation: Uint128::new(500_000_000_000u128),
            vesting_period: None,
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("recipient0000", &[]),
            allocation_msg,
        )
        .unwrap();

        //Query Allocation
        let query_msg = QueryMsg::Allocation { recipient: String::from("sub_recipient") };                                                                   //
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();

        let resp: AllocationResponse = from_binary(&res).unwrap();
        assert_eq!(resp.amount, Uint128::new(500_000_000_000u128));

        //Query Allocation
        let query_msg = QueryMsg::Allocation { recipient: String::from("recipient0000") };                                                                   //
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();

        let resp: AllocationResponse = from_binary(&res).unwrap();
        assert_eq!(resp.amount, Uint128::new(500_000_000_000u128));
                
        //AddRecipient
        let add_msg = ExecuteMsg::AddRecipient {
            recipient: String::from("recipient1"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //Error: AddAllocation over Allocation limit
        let allocation_msg = ExecuteMsg::AddAllocation {
            recipient: String::from("recipient1"),
            allocation: Uint128::new(30_000_000_000_001u128),
            vesting_period: Some(VestingPeriod {
                cliff: 365u64,
                linear: 365u64,
            }),
        };
        let err = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            allocation_msg,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            String::from("Increase is over contract's allocation")
        );
    }

    #[test]
    fn vesting_unlocks() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            owner: Some(String::from("owner0000")),
            initial_allocation: Uint128::new(30_000_000_000_000u128),
            mbrn_denom: String::from("mbrn_denom"),
            osmosis_proxy: String::from("osmosis_proxy"),
            staking_contract: String::from("staking_contract"),
            labs_addr: String::from("labs")
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &[]);
        let _res = instantiate(deps.as_mut(), mock_env(), v_info, msg).unwrap();

        //AddRecipient that won't get an allocation
        let add_msg = ExecuteMsg::AddRecipient {
            recipient: String::from("not_an_allocation"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //AddRecipient
        let add_msg = ExecuteMsg::AddRecipient {
            recipient: String::from("recipient0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();
        
        //Update Config: Increase allocations
        let msg = ExecuteMsg::UpdateConfig { 
            owner: None, 
            mbrn_denom: None,
            osmosis_proxy: None,
            staking_contract: None,
            additional_allocation: Some( Uint128::new(1_000_000_000_000u128) ),
        };
        let _res = execute( deps.as_mut(),mock_env(), mock_info("owner0000", &[]), msg )
        .unwrap();

        //AddAllocation
        let allocation_msg = ExecuteMsg::AddAllocation {
            recipient: String::from("recipient0000"),
            allocation: Uint128::new(1_000_000_000_000u128),
            vesting_period: Some(VestingPeriod {
                cliff: 365u64,
                linear: 365u64,
            }),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            allocation_msg,
        )
        .unwrap();

        //Query Unlocked
        let query_msg = QueryMsg::UnlockedTokens {
            recipient: String::from("recipient0000"),
        };
        //
        let mut env = mock_env();
        env.block.time = env.block.time.plus_seconds(47304000u64); //1.5 years
                                                                   //
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();

        let resp: UnlockedResponse = from_binary(&res).unwrap();
        assert_eq!(resp.unlocked_amount, Uint128::new(500_000_000_000u128));

        ///Invalid Recipient withdraw
        let withdraw_msg = ExecuteMsg::WithdrawUnlocked {};
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("not_a_recipient", &[]),
            withdraw_msg,
        )
        .unwrap_err();

        ///Recipient w/ no Allocaition 'Withdraw'
        let withdraw_msg = ExecuteMsg::WithdrawUnlocked {};
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("not_an_allocation", &[]),
            withdraw_msg,
        )
        .unwrap_err();

        ///Withdraw unlocked
        let withdraw_msg = ExecuteMsg::WithdrawUnlocked {};
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("recipient0000", &[]),
            withdraw_msg,
        )
        .unwrap();

        //Can withdraw half since halfway thru linear vesting
        assert_eq!(res.messages, vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens { 
                    denom: String::from("mbrn_denom"), 
                    amount: Uint128::new(500000000000), 
                    mint_to_address: String::from("recipient0000") 
                })
                .unwrap()
            }))
        ]);
        assert_eq!(
            res.attributes,
            vec![
                attr("method", "withdraw_unlocked"),
                attr("recipient", String::from("recipient0000")),
                attr("withdrawn_amount", String::from("500000000000")),
            ]
        );

        ///Withdraw unlocked but nothing to withdraw
        let withdraw_msg = ExecuteMsg::WithdrawUnlocked {};
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("recipient0000", &[]),
            withdraw_msg,
        )
        .unwrap();

        //Can't withdraw anything bc no time has past since last withdrawal
        assert_eq!(
            res.attributes,
            vec![
                attr("method", "withdraw_unlocked"),
                attr("recipient", String::from("recipient0000")),
                attr("withdrawn_amount", String::from("0")),
            ]
        );

        env.block.time = env.block.time.plus_seconds(99999999u64); //buncha years

         ///Withdraw unlocked
         let withdraw_msg = ExecuteMsg::WithdrawUnlocked {};
         let res = execute(
             deps.as_mut(),
             env.clone(),
             mock_info("recipient0000", &[]),
             withdraw_msg,
         )
         .unwrap();
 
         //Can withdraw the rest (ie the other half)
         assert_eq!(res.messages, vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens { 
                    denom: String::from("mbrn_denom"), 
                    amount: Uint128::new(500000000000), 
                    mint_to_address: String::from("recipient0000") 
                })
                .unwrap()
            }))
        ]);
        assert_eq!(
            res.attributes,
            vec![
                attr("method", "withdraw_unlocked"),
                attr("recipient", String::from("recipient0000")),
                attr("withdrawn_amount", String::from("500000000000")),
            ]
        );
 
    }

    #[test]
    fn initial_allocation() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            owner: Some(String::from("owner0000")),
            initial_allocation: Uint128::new(30_000_000_000_000u128),
            mbrn_denom: String::from("mbrn_denom"),
            osmosis_proxy: String::from("osmosis_proxy"),
            staking_contract: String::from("staking_contract"),
            labs_addr: String::from("labs")
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &[]);
        let res = instantiate(deps.as_mut(), mock_env(), v_info, msg).unwrap();

        //Assert Mint and Stake Msgs
        assert_eq!(
            res.messages,
            vec![
                SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: String::from("osmosis_proxy"),
                    funds: vec![],
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom: String::from("mbrn_denom"),
                        amount: Uint128::new(30_000_000_000_000u128),
                        mint_to_address: String::from("cosmos2contract")
                    })
                    .unwrap()
                })),
                SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: String::from("staking_contract"),
                    funds: vec![coin(30_000_000_000_000, "mbrn_denom")],
                    msg: to_binary(&StakingExecuteMsg::Stake { user: None }).unwrap()
                })),
            ]
        );

        let msg = QueryMsg::Config {  };
        let res = query(deps.as_ref(), mock_env(), msg).unwrap();

        let resp: Config = from_binary(&res).unwrap();
        assert_eq!(resp, 
            Config { 
                owner: Addr::unchecked("owner0000"),
                total_allocation: Uint128::new(30_000_000_000_000), 
                mbrn_denom: String::from("mbrn_denom"),
                osmosis_proxy: Addr::unchecked("osmosis_proxy"),
                staking_contract: Addr::unchecked("staking_contract"),
            });
    }
}
