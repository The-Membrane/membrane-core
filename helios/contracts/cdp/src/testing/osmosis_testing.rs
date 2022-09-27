use cosmwasm_std::{Coin, Decimal, Uint128 };
use osmosis_testing::{Account, Module, OsmosisTestApp, Wasm};

use membrane::types::{AssetPool, AssetInfo, Asset};
use membrane::osmosis_proxy::{ InstantiateMsg as OP_InstantiateMsg };
use membrane::oracle::{ InstantiateMsg as Oracle_InstantiateMsg };
use membrane::staking::{ InstantiateMsg as Staking_InstantiateMsg };
use membrane::builder_vesting::{ InstantiateMsg as BV_InstantiateMsg };
use membrane::governance::{ InstantiateMsg as Gov_InstantiateMsg, VOTING_PERIOD_INTERVAL, STAKE_INTERVAL };
use membrane::stability_pool::{ InstantiateMsg as SP_InstantiateMsg };
use membrane::positions::{ InstantiateMsg as CDP_InstantiateMsg };
use membrane::liq_queue::{ InstantiateMsg as LQ_InstantiateMsg };
use membrane::liquidity_check::{ InstantiateMsg as LC_InstantiateMsg };
use membrane::debt_auction::{ InstantiateMsg as DA_InstantiateMsg };

const PROPOSAL_VOTING_PERIOD: u64 = *VOTING_PERIOD_INTERVAL.start();
const PROPOSAL_EFFECTIVE_DELAY: u64 = 14399;
const PROPOSAL_EXPIRATION_PERIOD: u64 = 100799;
const PROPOSAL_REQUIRED_STAKE: u128 = *STAKE_INTERVAL.start();
const PROPOSAL_REQUIRED_QUORUM: &str = "0.50";
const PROPOSAL_REQUIRED_THRESHOLD: &str = "0.60";

#[test]
fn osmosis_testing(){
        
    let app = OsmosisTestApp::new();

    let accs = app
        .init_accounts(
            &[
                Coin::new(1_000_000_000_000, "uatom"),
                Coin::new(1_000_000_000_000, "uosmo"),
            ],
            1,
        )
        .unwrap();
    let admin = &accs[0];
    // let new_admin = &accs[1];

    let wasm = Wasm::new(&app);

    //Store OP code
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/osmosis_proxy.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    //Instantiate OP contract
    let op_contract_addr = wasm
        .instantiate(
            code_id,
            &OP_InstantiateMsg { },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"osmosis_proxy" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;

    //Store and Instantiate Oracle contract
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/oracle.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    let oracle_contract_addr = wasm
        .instantiate(
            code_id,
            &Oracle_InstantiateMsg { 
                owner: None, 
                osmosis_proxy: op_contract_addr.clone(), 
                positions_contract: None, 
            },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"oracle" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;

    //Store and Instantiate Staking contract
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/staking.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    let staking_contract_addr = wasm
        .instantiate(
            code_id,
            &Staking_InstantiateMsg {
                owner: None,
                dex_router: Some(String::from("router_addr")),
                max_spread: Some(Decimal::percent(10)),
                positions_contract: None,
                builders_contract: None,
                osmosis_proxy: Some(op_contract_addr.clone()),
                staking_rate: Some(Decimal::percent(10)),
                fee_wait_period: None,
                mbrn_denom: String::from("mbrn_denom"),
                unstaking_period: None,
            },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"staking" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;

    //Store and Instantiate Builder's Vesting contract
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/builder_vesting.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    let bv_contract_addr = wasm
        .instantiate(
            code_id,
            &BV_InstantiateMsg {
                owner: None,
                osmosis_proxy: op_contract_addr.clone(),
                mbrn_denom: String::from("mbrn_denom"),
                initial_allocation: Uint128::new(30_000_000_000_000u128),
                staking_contract: staking_contract_addr.clone(),
            },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"builder_vesting" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;

    //Store and Instantiate Gov contract
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/governance.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    let gov_contract_addr = wasm
        .instantiate(
            code_id,
            &Gov_InstantiateMsg {
                mbrn_staking_contract_addr: staking_contract_addr.to_string(),
                builders_contract_addr: bv_contract_addr.to_string(),
                builders_voting_power_multiplier: Decimal::percent(33),
                proposal_voting_period: PROPOSAL_VOTING_PERIOD,
                proposal_effective_delay: PROPOSAL_EFFECTIVE_DELAY,
                proposal_expiration_period: PROPOSAL_EXPIRATION_PERIOD,
                proposal_required_stake: Uint128::from(PROPOSAL_REQUIRED_STAKE),
                proposal_required_quorum: String::from(PROPOSAL_REQUIRED_QUORUM),
                proposal_required_threshold: String::from(PROPOSAL_REQUIRED_THRESHOLD),
                whitelisted_links: vec!["https://some.link/".to_string()],
            },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"governance" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;

    //Store and Instantiate Position's contract
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/cdp.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    let cdp_contract_addr = wasm
        .instantiate(
            code_id,
            &CDP_InstantiateMsg {
                owner: None,
                liq_fee: Decimal::percent(1),
                stability_pool: None,
                dex_router: Some("router_contract_addr".to_string()),
                staking_contract: Some(staking_contract_addr.to_string()),
                oracle_contract: Some(oracle_contract_addr.to_string()),
                interest_revenue_collector: Some("fee_collector".to_string()),
                osmosis_proxy: Some(op_contract_addr.to_string()),
                debt_auction: None,
                liquidity_contract: None,
                oracle_time_limit: 60u64,
                debt_minimum: Uint128::new(2000u128),
                collateral_twap_timeframe: 60u64,
                credit_twap_timeframe: 90u64,
            },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"cdp" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;

    //Store and Instantiate SP contract
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/stability_pool.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    let sp_contract_addr = wasm
        .instantiate(
            code_id,
            &SP_InstantiateMsg {
                owner: None,
                asset_pool: Some(AssetPool {
                    credit_asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "credit".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
                dex_router: Some(String::from("router_addr")),
                max_spread: Some(Decimal::percent(10)),
                desired_ratio_of_total_credit_supply: Some(Decimal::percent(80)),
                osmosis_proxy: op_contract_addr.to_string(),
                mbrn_denom: String::from("mbrn_denom"),
                incentive_rate: Some(Decimal::percent(10)),
                positions_contract: cdp_contract_addr.to_string(),
                max_incentives: None,
            },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"stability_pool" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;
    
    //Store and Instantiate LQ contract
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/liq_queue.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    let lq_contract_addr = wasm
        .instantiate(
            code_id,
            &LQ_InstantiateMsg {
                owner: None,
                positions_contract: cdp_contract_addr.to_string(),
                waiting_period: 60u64,
                basket_id: Some( Uint128::new(1u128) ),
                bid_asset: None,
            },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"liq_queue" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;

    //Store and Instantiate LC contract
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/liquidity_check.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    let lc_contract_addr = wasm
        .instantiate(
            code_id,
            &LC_InstantiateMsg {
                owner: None,
                positions_contract: cdp_contract_addr.to_string(),
                osmosis_proxy: op_contract_addr.clone(),
            },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"liquidity_check" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;

    //Store and Instantiate Auction contract
    let wasm_byte_code = std::fs::read("/home/trix/MembraneGit/artifacts/debt_auction.wasm").unwrap();
    let code_id = wasm
        .store_code(&wasm_byte_code, None, admin)
        .unwrap()
        .data
        .code_id;

    let auction_contract_addr = wasm
        .instantiate(
            code_id,
            &DA_InstantiateMsg {
                owner: None,
                positions_contract: cdp_contract_addr.to_string(),
                osmosis_proxy: op_contract_addr.clone(),
                oracle_contract: oracle_contract_addr.clone(),
                twap_timeframe: 90u64,
                mbrn_denom: String::from("mbrn_denom"),
                initial_discount: Decimal::percent(1),
                discount_increase_timeframe: 30u64,
                discount_increase: Decimal::percent(1),
            },
            None, // contract admin used for migration, not the same as cw1_whitelist admin
            Some( &"debt_auction" ), // contract label
            &[], // funds
            admin, // signer
        )
        .unwrap()
        .data
        .address;

    // let admin_list = wasm
    //     .query::<QueryMsg, AdminListResponse>(&contract_addr, &QueryMsg::AdminList {})
    //     .unwrap();

    // assert_eq!(admin_list.admins, init_admins);
    // assert!(admin_list.mutable);

    // ============= NEW CODE ================

    // update admin list and recheck the state
    // let new_admins = vec![new_admin.address()];
    // wasm.execute::<ExecuteMsg>(
    //     &contract_addr,
    //     &ExecuteMsg::UpdateAdmins {
    //         admins: new_admins.clone(),
    //     },
    //     &[],
    //     admin,
    // )
    // .unwrap();

}   //Edit CDP config 

    //Create first basket