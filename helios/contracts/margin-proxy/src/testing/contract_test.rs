#[cfg(test)]
mod tests {
    use crate::contracts::{instantiate, execute};
    
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{to_binary, CosmosMsg, SubMsg, Uint128, Decimal, WasmMsg, coin};

    use membrane::margin_proxy::{InstantiateMsg, ExecuteMsg};
    use membrane::positions::ExecuteMsg as CDP_ExecuteMsg;

    #[test]
    fn deposit_to_new_position(){

        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            owner: None,
            positions_contract: String::from("positions_contract"),
            apollo_router_contract: String::from("apollo_router_contract"),
            max_slippage: Decimal::percent(10),
        };

        //Instantiating contract
        let v_info = mock_info("owner0000", &[]);
        let _res = instantiate(deps.as_mut(), mock_env(), v_info, msg).unwrap();

        //New user position
        let deposit_msg = ExecuteMsg::Deposit {
            basket_id: Uint128::new(1),
            position_id: None,
        };
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("sender88", &[coin(1_000, "debit")]),
            deposit_msg,
        )
        .unwrap();

        //Deposit Msg assertion
        assert_eq!(
            res.messages,
            vec![
                SubMsg::reply_on_success(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: String::from("positions_contract"),
                    msg: to_binary(&CDP_ExecuteMsg::Deposit {
                        basket_id: Uint128::new(1),
                        position_id: None,
                        position_owner: None,
                    })
                    .unwrap(),
                    funds: vec![coin(1_000, "debit")],
                }), 2u64)
        ]);
      
       
    }


}