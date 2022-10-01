#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{
        mock_env, mock_info, MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{
        coins, from_binary, Attribute, ContractResult, Uint128, CosmosMsg, OwnedDeps, Querier, StdError,
        SystemError, SystemResult, to_binary,
    };
    use osmo_bindings::{ OsmosisMsg, OsmosisQuery};
    use std::marker::PhantomData;

    use membrane::osmosis_proxy::{ InstantiateMsg, ExecuteMsg, QueryMsg, GetDenomResponse, TokenInfoResponse };

    use crate::TokenFactoryError;
    use crate::contract::{instantiate, execute, query, reply};
    use crate::testing::multi_test::OsmosisApp;

    const DENOM_NAME: &str = "mydenom";
    const DENOM_PREFIX: &str = "factory";

    fn mock_dependencies_with_custom_quierier<Q: Querier>(
        querier: Q,
    ) -> OwnedDeps<MockStorage, MockApi, Q, OsmosisQuery> {
        OwnedDeps {
            storage: MockStorage::default(),
            api: MockApi::default(),
            querier,
            custom_query_type: PhantomData,
        }
    }

    
    pub fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, OsmosisApp, OsmosisQuery> {
        let custom_querier = OsmosisApp::new();
        mock_dependencies_with_custom_quierier(custom_querier)
    }

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
            owner: None, 
            add_owner: true, 
            debt_auction: Some(String::from("debt_auction")),
        };
        let info = mock_info("creator", &coins(2, "token"));
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        let msg = ExecuteMsg::UpdateConfig { 
            owner: Some(String::from("new_owner")), 
            add_owner: true, 
            debt_auction: None,
        };
        let info = mock_info("debt_auction", &coins(2, "token"));
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
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
    fn msg_create_denom_success() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();


        let subdenom: String = String::from(DENOM_NAME);

        let msg = ExecuteMsg::CreateDenom { 
            subdenom, 
            basket_id: String::from("1"),
            max_supply: Some(Uint128::new(10)), 
            liquidity_multiplier: None, 
        };
        let info = mock_info("creator", &coins(2, "token"));
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        assert_eq!(1, res.messages.len());

        let expected_message = CosmosMsg::from(OsmosisMsg::CreateDenom {
            subdenom: String::from(DENOM_NAME),
        });
        let actual_message = res.messages.get(0).unwrap();
        assert_eq!(expected_message, actual_message.msg);

        assert_eq!(5, res.attributes.len());

        let expected_attribute = Attribute::new("method", "create_denom");
        let actual_attribute = res.attributes.get(0).unwrap();
        assert_eq!(expected_attribute, actual_attribute);

        assert_eq!(res.data.ok_or(0), Err(0));       
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
            basket_id: String::from("1"),
            max_supply: Some(Uint128::new(10)), 
            liquidity_multiplier: None, 
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
    fn msg_change_admin_success() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        

        const NEW_ADMIN_ADDR: &str = "newadmin";

        let info = mock_info("creator", &coins(2, "token"));

        let full_denom_name: &str =
            &format!("{}/{}/{}", DENOM_PREFIX, MOCK_CONTRACT_ADDR, DENOM_NAME)[..];

        let msg = ExecuteMsg::ChangeAdmin {
            denom: String::from(full_denom_name),
            new_admin_address: String::from(NEW_ADMIN_ADDR),
        };
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        assert_eq!(1, res.messages.len());

        let expected_message = CosmosMsg::from(OsmosisMsg::ChangeAdmin {
            denom: String::from(full_denom_name),
            new_admin_address: String::from(NEW_ADMIN_ADDR),
        });
        let actual_message = res.messages.get(0).unwrap();
        assert_eq!(expected_message, actual_message.msg);

        assert_eq!(3, res.attributes.len());

        let expected_attribute = Attribute::new("method", "change_admin");
        let actual_attribute = res.attributes.get(0).unwrap();
        assert_eq!(expected_attribute, actual_attribute);

        assert_eq!(res.data.ok_or(0), Err(0));
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