#[cfg(test)]
mod tests {
    use cosmwasm_std::testing::{
        mock_env, mock_info, mock_dependencies, MockApi, MockStorage, MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{
        coins, from_binary, Attribute, Uint128, CosmosMsg, OwnedDeps, Querier, StdError, Addr, Decimal
    };
    use membrane::types::Owner;
    use std::marker::PhantomData;

    use membrane::osmosis_proxy::{InstantiateMsg, ExecuteMsg, QueryMsg, GetDenomResponse, Config};

    use crate::TokenFactoryError;
    use crate::contract::{instantiate, execute, query};

    const DENOM_NAME: &str = "mydenom";
    const DENOM_PREFIX: &str = "factory";
   
    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn update_config() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        let msg = ExecuteMsg::UpdateConfig { 
            owner: Some(vec![String::from("new_owner")]), 
            add_owner: true, 
            debt_auction: Some(String::from("debt_auction")),
            positions_contract: Some(String::from("positions_contract")),
            liquidity_contract: Some(String::from("liquidity_contract")),
        };
        let info = mock_info("creator", &coins(2, "token"));
        execute(deps.as_mut(), mock_env(), info, msg).unwrap();       

        //Set expected_config
        let expected_config = Config {
            owners: vec![ Owner {
                owner: Addr::unchecked("creator"),
                total_minted: Uint128::zero(),
                liquidity_multiplier: Some(Decimal::zero()),
                non_token_contract_auth: true, 
            },
            Owner {
                owner: Addr::unchecked("new_owner"),
                total_minted: Uint128::zero(),
                liquidity_multiplier: Some(Decimal::zero()),
                non_token_contract_auth: true, 
            }],
            debt_auction: Some(Addr::unchecked("debt_auction")),
            positions_contract: Some(Addr::unchecked("positions_contract")),
            liquidity_contract: Some(Addr::unchecked("liquidity_contract")),
        };

        let response = query(deps.as_ref(), mock_env(), QueryMsg::Config {  }).unwrap();
        let config: Config = from_binary(&response).unwrap();
        assert_eq!(
            config,
            expected_config
        );

    }

    #[test]
    fn query_get_denom() {
        let deps = mock_dependencies();
        let get_denom_query = QueryMsg::GetDenom {
            creator_address: String::from(MOCK_CONTRACT_ADDR),
            subdenom: String::from(DENOM_NAME),
        };
        let response = query(deps.as_ref(), mock_env(), get_denom_query).unwrap();
        let get_denom_response: GetDenomResponse = from_binary(&response).unwrap();
        assert_eq!(
            format!("{}/{}/{}", DENOM_PREFIX, MOCK_CONTRACT_ADDR, DENOM_NAME),
            get_denom_response.denom
        );
    }

    #[test]
    fn msg_create_denom_invalid_subdenom() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        

        let subdenom: String = String::from("");

        let msg = ExecuteMsg::CreateDenom { 
            subdenom, 
            max_supply: Some(Uint128::new(10)), 
        };
        let info = mock_info("creator", &coins(2, "token"));
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert_eq!(
            TokenFactoryError::InvalidSubdenom {
                subdenom: String::from("")
            },
            err
        );
    }

    #[test]
    fn msg_change_admin_empty_address() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        

        const EMPTY_ADDR: &str = "";

        let info = mock_info("creator", &coins(2, "token"));

        let msg = ExecuteMsg::ChangeAdmin {
            denom: String::from(DENOM_NAME),
            new_admin_address: String::from(EMPTY_ADDR),
        };
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        match err {
            TokenFactoryError::Std(StdError::GenericErr { msg, .. }) => {
                assert!(msg.contains("human address too short"))
            }
            e => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn msg_change_admin_invalid_denom() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        

        const NEW_ADMIN_ADDR: &str = "newadmin";

        let info = mock_info("creator", &coins(2, "token"));

        // too many parts in denom
        let full_denom_name: &str = &format!(
            "{}/{}/{}/invalid",
            DENOM_PREFIX, MOCK_CONTRACT_ADDR, DENOM_NAME
        )[..];

        let msg = ExecuteMsg::ChangeAdmin {
            denom: String::from(full_denom_name),
            new_admin_address: String::from(NEW_ADMIN_ADDR),
        };
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();

        let expected_error = TokenFactoryError::InvalidDenom {
            denom: String::from(full_denom_name),
            message: String::from("denom must have 3 parts separated by /, had 4"),
        };

        assert_eq!(expected_error, err);
    }


    #[test]
    fn msg_mint_invalid_denom() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        

        const NEW_ADMIN_ADDR: &str = "newadmin";

        let mint_amount = Uint128::new(100_u128);

        let info = mock_info("creator", &coins(2, "token"));

        let full_denom_name: &str = &format!("{}/{}", DENOM_PREFIX, MOCK_CONTRACT_ADDR)[..];
        let msg = ExecuteMsg::MintTokens {
            denom: String::from(full_denom_name),
            amount: mint_amount,
            mint_to_address: String::from(NEW_ADMIN_ADDR),
        };
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        let expected_error = TokenFactoryError::InvalidDenom {
            denom: String::from(full_denom_name),
            message: String::from("denom must have 3 parts separated by /, had 2"),
        };

        assert_eq!(expected_error, err);
    }


    #[test]
    fn msg_burn_tokens_input_address() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        

        const BURN_FROM_ADDR: &str = "burnfrom";
        let burn_amount = Uint128::new(100_u128);
        let full_denom_name: &str =
            &format!("{}/{}/{}", DENOM_PREFIX, MOCK_CONTRACT_ADDR, DENOM_NAME)[..];

        let info = mock_info("creator", &coins(2, "token"));

        let msg = ExecuteMsg::BurnTokens {
            denom: String::from(full_denom_name),
            burn_from_address: String::from(BURN_FROM_ADDR),
            amount: burn_amount,
        };
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();

        let expected_error = TokenFactoryError::BurnFromAddressNotSupported {
            address: String::from(BURN_FROM_ADDR),
        };

        assert_eq!(expected_error, err)
    }



}