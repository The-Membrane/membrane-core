use crate::contracts::{execute, instantiate, query, SECONDS_PER_DAY};

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{
    attr, coin, coins, from_binary, to_binary, Addr, BankMsg, CosmosMsg, Decimal, SubMsg, Uint128,
    WasmMsg,
};
use cw20::Cw20ReceiveMsg;

use membrane::apollo_router::{ExecuteMsg as RouterExecuteMsg, SwapToAssetsInput};
use membrane::helpers::SECONDS_PER_YEAR;
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::launch::{
    Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig
};
use membrane::types::{Asset, AssetInfo};


#[test]
fn update_config(){

    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {        
        owner: None,
        labs_addr: String::from("labs"),
        apollo_router: String::from("router"),
        //Contract IDs
        osmosis_proxy_id: 0,
        oracle_id: 0,
        staking_id: 0,
        vesting_id: 0,
        governance_id: 0,
        positions_id: 0,
        stability_pool_id: 0,
        liq_queue_id: 0,
        liquidity_check_id: 0,
        mbrn_auction_id: 0,    
    };

    //Instantiating contract
    let info = mock_info("sender88", &[coin(20_000_000, "uosmo")]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    
    let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
        credit_denom: Some(String::from("new_credit_denom")),
        mbrn_denom: Some(String::from("new_mbrn_denom")),
        osmo_denom: Some(String::from("new_osmo_denom")),
        usdc_denom: Some(String::from("new_usdc_denom")),
    });

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("labs", &vec![]),
        msg,
    )
    .unwrap();

    //Query Config
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::Config {},
    )
    .unwrap();
    let config: Config = from_binary(&res).unwrap();

    assert_eq!(
        config.mbrn_denom,        
        String::from("new_mbrn_denom"),
    );
    assert_eq!(
        config.credit_denom,        
        String::from("new_credit_denom"),
    );
    assert_eq!(
        config.osmo_denom,        
        String::from("new_osmo_denom"),
    );
    assert_eq!(
        config.usdc_denom,    
        String::from("new_usdc_denom"),
    );
}


#[test]
fn lock() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {        
        owner: None,
        labs_addr: String::from("labs"),
        apollo_router: String::from("router"),
        //Contract IDs
        osmosis_proxy_id: 0,
        oracle_id: 0,
        staking_id: 0,
        vesting_id: 0,
        governance_id: 0,
        positions_id: 0,
        stability_pool_id: 0,
        liq_queue_id: 0,
        liquidity_check_id: 0,
        mbrn_auction_id: 0,    
    };

    //Instantiating contract
    let info = mock_info("sender88", &[coin(20_000_000, "uosmo")]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Invalid lock asset
    let msg = ExecuteMsg::Lock { lock_up_duration: 0u64 };
    let info = mock_info("user1", &[coin(10, "not_uosmo")]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Generic error: No valid lockdrop asset, looking for uosmo".to_string()
    ); 

    //Invalid lock duration
    let msg = ExecuteMsg::Lock { lock_up_duration: 366u64 };
    let info = mock_info("user1", &[coin(10, "not_uosmo")]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Custom Error val: Can't lock that long".to_string()
    ); 
    
    //Lock uosmo for 7 days
    let msg = ExecuteMsg::Lock { lock_up_duration: 7u64 };
    let info = mock_info("user1", &[coin(10, "uosmo")]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("user", "user1"),
            attr("lock_up_duration", "7"),
            attr("deposit", "10 uosmo"),
        ]
    );    

    //Lock attempt after deposit period
    let msg = ExecuteMsg::Lock { lock_up_duration: 7u64 };
    let info = mock_info("user1", &[coin(10, "uosmo")]);
    
    let mut env = mock_env();
    env.block.time = env.block.time.plus_seconds(5 * SECONDS_PER_DAY + 1); // 5 days + 1
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Deposit period over".to_string()
    ); 
 

    // //Query and Assert totals
    // let res = query(deps.as_ref(), mock_env(), QueryMsg::TotalStaked {}).unwrap();

    // let resp: TotalStakedResponse = from_binary(&res).unwrap();

    // assert_eq!(resp.total_not_including_vested, Uint128::new(10));
    // assert_eq!(resp.vested_total,  Uint128::new(11));
}


