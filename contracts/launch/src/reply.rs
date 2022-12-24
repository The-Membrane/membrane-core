use std::convert::TryInto;

use cosmwasm_std::{
    entry_point, to_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, WasmMsg, SubMsgResult, SubMsgResponse, WasmQuery,
    Response, StdResult, Uint128, Reply, StdError, CosmosMsg, SubMsg, Addr, coins, attr, Storage, QueryRequest,
};
use crate::error::ContractError;
use crate::contracts::{execute, instantiate, query, SECONDS_PER_DAY, POSITIONS_REPLY_ID, DEBT_AUCTION_REPLY_ID, CREATE_DENOM_REPLY_ID, ORACLE_REPLY_ID, OSMOSIS_PROXY_REPLY_ID, STAKING_REPLY_ID, VESTING_REPLY_ID, LIQ_QUEUE_REPLY_ID, GOVERNANCE_REPLY_ID, STABLESWAP_REPLY_ID, STABILITY_POOL_REPLY_ID ,LIQUIDITY_CHECK_REPLY_ID};
use crate::state::{ADDRESSES, CONFIG, CREDIT_POOL_IDS};

use membrane::governance::{InstantiateMsg as Gov_InstantiateMsg, VOTING_PERIOD_INTERVAL, STAKE_INTERVAL};
use membrane::helpers::{withdrawal_msg, get_contract_balances};
use membrane::launch::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig};
use membrane::math::{decimal_division, decimal_multiplication};
use membrane::stability_pool::{InstantiateMsg as SP_InstantiateMsg, ExecuteMsg as SPExecuteMsg, UpdateConfig as SPUpdateConfig};
use membrane::staking::{InstantiateMsg as Staking_InstantiateMsg, ExecuteMsg as StakingExecuteMsg};
use membrane::vesting::{InstantiateMsg as Vesting_InstantiateMsg, ExecuteMsg as VestingExecuteMsg};
use membrane::positions::{InstantiateMsg as CDP_InstantiateMsg, EditBasket, ExecuteMsg as CDPExecuteMsg, QueryMsg as CDPQueryMsg, UpdateConfig as CDPUpdateConfig};
use membrane::oracle::{InstantiateMsg as Oracle_InstantiateMsg, ExecuteMsg as OracleExecuteMsg};
use membrane::liq_queue::InstantiateMsg as LQInstantiateMsg;
use membrane::liquidity_check::{InstantiateMsg as LCInstantiateMsg, ExecuteMsg as LCExecuteMsg};
use membrane::debt_auction::InstantiateMsg as DAInstantiateMsg;
use membrane::osmosis_proxy::{ExecuteMsg as OPExecuteMsg, QueryMsg as OPQueryMsg};
use membrane::types::{AssetInfo, DebtTokenAsset, Position, Basket, Deposit, AssetPool, Asset, PoolInfo, LPAssetInfo, cAsset, TWAPPoolInfo, SupplyCap, LiquidityInfo, AssetOracleInfo, UserRatio, PoolStateResponse};

use osmosis_std::shim::Duration;
use osmosis_std::types::cosmos::base::v1beta1::Coin;
use osmosis_std::types::osmosis::gamm::poolmodels::balancer::v1beta1::MsgCreateBalancerPool;
use osmosis_std::types::osmosis::gamm::poolmodels::stableswap::v1beta1::{MsgCreateStableswapPool, MsgCreateStableswapPoolResponse, PoolParams as SSPoolParams};
use osmosis_std::types::osmosis::gamm::v1beta1::PoolParams;
use osmosis_std::types::osmosis::gamm::v1beta1::PoolAsset;
use osmosis_std::types::osmosis::incentives::MsgCreateGauge;
use osmosis_std::types::osmosis::lockup::QueryCondition;

//Governance constants
const PROPOSAL_VOTING_PERIOD: u64 = *VOTING_PERIOD_INTERVAL.start();
const PROPOSAL_EFFECTIVE_DELAY: u64 = 14399;
const PROPOSAL_EXPIRATION_PERIOD: u64 = 100799;
const PROPOSAL_REQUIRED_STAKE: u128 = *STAKE_INTERVAL.start();
const PROPOSAL_REQUIRED_QUORUM: &str = "0.50";
const PROPOSAL_REQUIRED_THRESHOLD: &str = "0.60";

//Called after the Osmosis Proxy (OP) reply
pub fn handle_create_denom_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response>{ 
    match msg.result.into_result() {
        Ok(result) => {
        let mut config = CONFIG.load(deps.storage)?;
        let addrs = ADDRESSES.load(deps.storage)?;
        
        //Get denoms
        let denoms: Vec<String> = deps.querier.query_wasm_smart::<Vec<String>>(addrs.osmosis_proxy, &OPQueryMsg::GetContractDenoms { limit: None })?;
        //We know CDT is first
        config.credit_denom = denoms[0].clone();
        config.mbrn_denom = denoms[1].clone();


        Ok(Response::new()
            .add_attribute("saved_denoms", format!("{:?}", denoms))
        )
    },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

pub fn handle_stableswap_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response>{    
    match msg.clone().result.into_result() {
        Ok(result) => {
        let config = CONFIG.load(deps.storage)?;
        let addrs = ADDRESSES.load(deps.storage)?;
        let mut msgs = vec![];
        
        //Mint MBRN for Incentives
        let op_msg = OPExecuteMsg::MintTokens { 
            denom: config.clone().mbrn_denom, 
            amount: Uint128::new(1_000_000_000_000), 
            mint_to_address: env.clone().contract.address.to_string(),
        };
        let op_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
            contract_addr: addrs.clone().osmosis_proxy.to_string(), 
            msg: to_binary(&msg)?, 
            funds: vec![], 
        });
        msgs.push(op_msg);
        
        //Get Stableswap denom from Response
        let mut pool_denom = String::from("");
        if let SubMsgResult::Ok(SubMsgResponse { data: Some(b), .. }) = msg.result {
            let res: MsgCreateStableswapPoolResponse = match b.try_into().map_err(ContractError::Std){
                Ok(res) => res,
                Err(err) => return Err(StdError::GenericErr { msg: String::from(err.to_string()) })
            };
            
            pool_denom = deps.querier.query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: addrs.clone().osmosis_proxy.to_string(), 
                msg: to_binary(&OPQueryMsg::PoolState {
                    id: res.pool_id,
                })?,
            }))?.shares.denom;
        }

        //Incentivize the stableswap
        let msg = MsgCreateGauge { 
            is_perpetual: false, 
            owner: addrs.clone().governance.to_string(),
            distribute_to: Some(QueryCondition { 
                lock_query_type: 0, //ByDuration
                denom: pool_denom,
                duration: Some(Duration { seconds: 14 * SECONDS_PER_DAY as i64, nanos: 0 }), 
                timestamp: None,
            }), 
            coins: vec![Coin {
                denom: config.clone().mbrn_denom, 
                amount: String::from("1_000_000_000_000"),
            }], 
            start_time: None, 
            num_epochs_paid_over: 90, //days
        }.into();
        msgs.push(msg);

        Ok(Response::new()
            .add_messages(msgs)
        )
    },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

pub fn handle_op_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Osmosis Proxy address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.osmosis_proxy = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);

            let mut sub_msgs = vec![];

            //Create CDT & MBRN denom
            let create_denom_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().osmosis_proxy.to_string(), 
                msg: to_binary(&OPExecuteMsg::CreateDenom { 
                    subdenom: String::from("cdt"), 
                    max_supply: None,
                })?, 
                funds: coins(10_000_000, "uosmo"),
            });            
            let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().osmosis_proxy.to_string(), 
                msg: to_binary(&OPExecuteMsg::CreateDenom { 
                    subdenom: String::from("mbrn"), 
                    max_supply: None,
                })?, 
                funds: coins(10_000_000, "uosmo"),
            });
            sub_msgs.push(SubMsg::reply_on_success(msg, CREATE_DENOM_REPLY_ID));

            //Instantiate Oracle
            let oracle_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(env.contract.address.to_string()), 
                code_id: config.clone().oracle_id, 
                msg: to_binary(&Oracle_InstantiateMsg {
                    owner: None,
                    osmosis_proxy: valid_address.to_string(),
                    positions_contract: None,
                })?, 
                funds: vec![], 
                label: String::from("oracle"), 
            });
            sub_msgs.push(SubMsg::reply_on_success(oracle_instantiation, ORACLE_REPLY_ID));
            
            Ok(Response::new()
                .add_message(create_denom_msg)
                .add_submessages(sub_msgs)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

pub fn handle_oracle_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Oracle address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.oracle = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);

            //Instantiate Staking
            let staking_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(env.contract.address.to_string()), 
                code_id: config.clone().staking_id, 
                msg: to_binary(&Staking_InstantiateMsg {
                    owner: None,
                    positions_contract: None,
                    vesting_contract: None,
                    governance_contract: None,
                    osmosis_proxy: Some(addrs.osmosis_proxy.to_string()),
                    staking_rate: None,
                    fee_wait_period: None,
                    unstaking_period: None,
                    mbrn_denom: config.clone().mbrn_denom,
                    dex_router: Some(config.clone().apollo_router.to_string()),
                    max_spread: Some(Decimal::percent(10)),
                })?, 
                funds: vec![], 
                label: String::from("staking"), 
            });
            let sub_msg = SubMsg::reply_on_success(staking_instantiation, STAKING_REPLY_ID);
            
            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}


pub fn handle_staking_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Staking address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.staking = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);

            //Instantiate Vesting
            let vesting_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(env.contract.address.to_string()), 
                code_id: config.clone().vesting_id, 
                msg: to_binary(&Vesting_InstantiateMsg {
                    owner: None,
                    initial_allocation: Uint128::new(20_000_000_000_000),
                    labs_addr: config.clone().labs_addr.to_string(),
                    mbrn_denom: config.clone().mbrn_denom,
                    osmosis_proxy: addrs.clone().osmosis_proxy.to_string(),
                    staking_contract: addrs.clone().staking.to_string(),
                })?, 
                funds: vec![], 
                label: String::from("vesting"), 
            });
            let sub_msg = SubMsg::reply_on_success(vesting_instantiation, VESTING_REPLY_ID);            
            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

pub fn handle_vesting_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Vesting address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.vesting = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);

            //Instantiate Gov
            let gov_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(env.contract.address.to_string()), 
                code_id: config.clone().governance_id, 
                msg: to_binary(&Gov_InstantiateMsg {
                    mbrn_staking_contract_addr: addrs.clone().staking.to_string(),
                    vesting_contract_addr: addrs.clone().vesting.to_string(),
                    vesting_voting_power_multiplier: Decimal::percent(50),
                    proposal_voting_period: PROPOSAL_VOTING_PERIOD,
                    expedited_proposal_voting_period: PROPOSAL_VOTING_PERIOD,
                    proposal_effective_delay: PROPOSAL_EFFECTIVE_DELAY,
                    proposal_expiration_period: PROPOSAL_EXPIRATION_PERIOD,
                    proposal_required_stake: Uint128::from(PROPOSAL_REQUIRED_STAKE),
                    proposal_required_quorum: String::from(PROPOSAL_REQUIRED_QUORUM),
                    proposal_required_threshold: String::from(PROPOSAL_REQUIRED_THRESHOLD),
                    //TODO
                    whitelisted_links: vec!["https://some.link/".to_string()],
                })?, 
                funds: vec![], 
                label: String::from("governance"), 
            });
            let sub_msg = SubMsg::reply_on_success(gov_instantiation, GOVERNANCE_REPLY_ID);            
            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

pub fn handle_gov_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Gov address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.governance = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);

            let mut msgs = vec![];
            //Update previous contract admins to Governance
            msgs.push(CosmosMsg::Wasm(WasmMsg::UpdateAdmin { 
                contract_addr: addrs.osmosis_proxy.to_string(), 
                admin: addrs.clone().governance.to_string(),
            }));
            msgs.push(CosmosMsg::Wasm(WasmMsg::UpdateAdmin { 
                contract_addr: addrs.oracle.to_string(), 
                admin: addrs.clone().governance.to_string(),
            }));
            msgs.push(CosmosMsg::Wasm(WasmMsg::UpdateAdmin { 
                contract_addr: addrs.staking.to_string(), 
                admin: addrs.clone().governance.to_string(),
            }));
            msgs.push(CosmosMsg::Wasm(WasmMsg::UpdateAdmin { 
                contract_addr: addrs.vesting.to_string(), 
                admin: addrs.clone().governance.to_string(),
            }));
            msgs.push(CosmosMsg::Wasm(WasmMsg::UpdateAdmin { 
                contract_addr: addrs.governance.to_string(), 
                admin: addrs.clone().governance.to_string(),
            }));

            
            //Instantiate Positions
            let cdp_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(addrs.clone().governance.to_string()), 
                code_id: config.clone().positions_id, 
                msg: to_binary(&CDP_InstantiateMsg {
                    owner: None,
                    liq_fee: Decimal::percent(1),
                    oracle_time_limit: 60u64,
                    debt_minimum: Uint128::new(2000_000_000u128),
                    collateral_twap_timeframe: 60u64,
                    credit_twap_timeframe: 480u64,
                    stability_pool: None,
                    dex_router: Some(config.clone().apollo_router.to_string()),
                    staking_contract: Some(addrs.clone().staking.to_string()),
                    oracle_contract: Some(addrs.clone().oracle.to_string()),
                    osmosis_proxy: Some(addrs.clone().osmosis_proxy.to_string()),
                    debt_auction: None,
                    liquidity_contract: None,
                    discounts_contract: None,
                })?, 
                funds: vec![], 
                label: String::from("positions"), 
            });
            let sub_msg = SubMsg::reply_on_success(cdp_instantiation, POSITIONS_REPLY_ID);     
            
            
            Ok(Response::new()
                .add_messages(msgs)
                .add_submessage(sub_msg)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

pub fn handle_cdp_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save CDP address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.positions = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);

            let mut msgs = vec![];
            
            //Add Collateral Oracles
            /// ATOM
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().oracle.to_string(), 
                    msg: to_binary(&OracleExecuteMsg::AddAsset { 
                        asset_info: AssetInfo::NativeToken { denom: config.clone().atom_denom }, 
                        oracle_info: AssetOracleInfo { 
                            basket_id: Uint128::one(), 
                            osmosis_pools_for_twap: vec![
                                //ATOM/OSMO
                                TWAPPoolInfo { 
                                    pool_id: config.clone().atomosmo_pool_id, 
                                    base_asset_denom: config.clone().atom_denom.to_string(), 
                                    quote_asset_denom: config.clone().osmo_denom.to_string(),  
                                },
                                //OSMO/USDC
                                TWAPPoolInfo { 
                                    pool_id: config.clone().osmousdc_pool_id, 
                                    base_asset_denom: config.clone().osmo_denom.to_string(), 
                                    quote_asset_denom: config.clone().usdc_denom.to_string(),  
                                },
                            ],
                            static_price: None,
                        },
                    })?, 
                    funds: vec![],
                }));
            /// OSMO
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().oracle.to_string(), 
                    msg: to_binary(&OracleExecuteMsg::AddAsset { 
                        asset_info: AssetInfo::NativeToken { denom: config.clone().osmo_denom }, 
                        oracle_info: AssetOracleInfo { 
                            basket_id: Uint128::one(), 
                            osmosis_pools_for_twap: vec![TWAPPoolInfo { 
                                pool_id: config.clone().osmousdc_pool_id, 
                                base_asset_denom: config.clone().osmo_denom.to_string(), 
                                quote_asset_denom: config.clone().usdc_denom.to_string(),  
                            }],
                            static_price: None,
                        },
                    })?, 
                    funds: vec![],
                }));
            /// USDC
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().oracle.to_string(), 
                    msg: to_binary(&OracleExecuteMsg::AddAsset { 
                        asset_info: AssetInfo::NativeToken { denom: config.clone().usdc_denom }, 
                        oracle_info: AssetOracleInfo { 
                            basket_id: Uint128::one(), 
                            osmosis_pools_for_twap: vec![],
                            static_price: Some(Decimal::one()),
                        },
                    })?, 
                    funds: vec![],
                }));

            //CreateBasket
            let msg = CDPExecuteMsg::CreateBasket {
                basket_id: Uint128::one(),
                collateral_types: vec![cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: config.clone().atom_denom,
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                },
                cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: config.clone().osmo_denom,
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                },
                cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: config.clone().usdc_denom,
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(90),
                    max_LTV: Decimal::percent(96),
                    pool_info: None,
                    rate_index: Decimal::one(),
                }],
                credit_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: config.clone().credit_denom,
                    },
                    amount: Uint128::from(0u128),
                },
                credit_price: Decimal::one(),
                base_interest_rate: Some(Decimal::percent(1)),
                credit_pool_ids: CREDIT_POOL_IDS.load(deps.storage)?.to_vec(),
                liquidity_multiplier_for_debt_caps: Some(Decimal::percent(500)),
                liq_queue: None,
            };
            let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().positions.to_string(), 
                msg: to_binary(&msg)?, 
                funds: vec![], 
            });
            msgs.push(msg);

            //Instantiate SP
            let sp_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(addrs.clone().governance.to_string()), 
                code_id: config.clone().stability_pool_id, 
                msg: to_binary(&SP_InstantiateMsg {
                    owner: Some(addrs.clone().governance.to_string()),
                    asset_pool: AssetPool { 
                        credit_asset: Asset { info: AssetInfo::NativeToken { denom: config.clone().credit_denom }, amount: Uint128::zero()}, 
                        liq_premium: Decimal::percent(10), 
                        deposits: vec![] 
                    },
                    incentive_rate: None,
                    max_incentives: None,
                    desired_ratio_of_total_credit_supply: None,
                    osmosis_proxy: addrs.clone().osmosis_proxy.to_string(),
                    positions_contract: addrs.clone().positions.to_string(),
                    mbrn_denom: config.clone().mbrn_denom,
                })?, 
                funds: vec![], 
                label: String::from("stability_pool"), 
            });
            let sub_msg = SubMsg::reply_on_success(sp_instantiation, STABILITY_POOL_REPLY_ID);     

            Ok(Response::new().add_messages(msgs).add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

pub fn handle_sp_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Stability Pool address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.stability_pool = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);
                       
            //Instantiate Liquidation Queue
            let lq_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(addrs.clone().governance.to_string()), 
                code_id: config.clone().liq_queue_id, 
                msg: to_binary(&LQInstantiateMsg {
                    owner: Some(addrs.clone().governance.to_string()),
                    positions_contract: addrs.clone().positions.to_string(),
                    waiting_period: 60u64,
                })?, 
                funds: vec![], 
                label: String::from("liquidation_queue"), 
            });
            let sub_msg = SubMsg::reply_on_success(lq_instantiation, LIQ_QUEUE_REPLY_ID);     
            
            
            Ok(Response::new()
                .add_submessage(sub_msg)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

pub fn handle_lq_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save LQ address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.liq_queue = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);

            let mut msgs = vec![];
            //Add LQ to Basket alongside 1/2 LPs & 3/5 SupplyCaps
            let msg = CDPExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: config.clone().atomosmo_pool_id.to_string(), //This gets auto-filled
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: Some(PoolInfo { 
                        pool_id: config.clone().atomosmo_pool_id, 
                        asset_infos: vec![
                            LPAssetInfo { info: AssetInfo::NativeToken { denom: config.clone().atom_denom }, decimals: 6, ratio: Decimal::percent(50) },
                            LPAssetInfo { info: AssetInfo::NativeToken { denom: config.clone().osmo_denom }, decimals: 6, ratio: Decimal::percent(50) }], 
                    }),
                    rate_index: Decimal::one(),
                }),
                liq_queue: Some(addrs.clone().liq_queue.to_string()),
                liquidity_multiplier: None,
                collateral_supply_caps: Some(vec![
                SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: config.clone().osmo_denom,
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                },
                SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: config.clone().atom_denom,
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                },
                SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: config.clone().usdc_denom,
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: Some(TWAPPoolInfo {
                    pool_id: CREDIT_POOL_IDS.load(deps.storage)?.stableswap,
                    base_asset_denom: config.clone().credit_denom,
                    quote_asset_denom: config.clone().usdc_denom,
                }),
                negative_rates: Some(false),
                cpc_margin_of_error: Some(Decimal::percent(1)),
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
                credit_pool_ids: None,
            });
            let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().positions.to_string(), 
                msg: to_binary(&msg)?, 
                funds: vec![], 
            });
            msgs.push(msg);
            //Add 2/2 LPs
            let msg = CDPExecuteMsg::EditBasket(EditBasket {
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: config.clone().osmousdc_pool_id.to_string(), //This gets auto-filled
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: Some(PoolInfo { 
                        pool_id: config.clone().osmousdc_pool_id, 
                        asset_infos: vec![
                            LPAssetInfo { info: AssetInfo::NativeToken { denom: config.clone().osmo_denom }, decimals: 6, ratio: Decimal::percent(50) },
                            LPAssetInfo { info: AssetInfo::NativeToken { denom: config.clone().usdc_denom }, decimals: 6, ratio: Decimal::percent(50) }], 
                    }),
                    rate_index: Decimal::one(),
                }),
                liq_queue: None,
                liquidity_multiplier: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
                credit_pool_ids: None,
            });
            let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().positions.to_string(), 
                msg: to_binary(&msg)?, 
                funds: vec![], 
            });
            msgs.push(msg);
            
                       
            //Instantiate Liquidity Check
            let lc_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(addrs.clone().governance.to_string()), 
                code_id: config.clone().liquidity_check_id, 
                msg: to_binary(&LCInstantiateMsg {
                    owner: Some(addrs.clone().governance.to_string()),
                    positions_contract: addrs.clone().positions.to_string(),
                    osmosis_proxy: addrs.clone().osmosis_proxy.to_string(),
                    
                })?, 
                funds: vec![], 
                label: String::from("liquidity_check"), 
            });
            let sub_msg = SubMsg::reply_on_success(lc_instantiation, LIQUIDITY_CHECK_REPLY_ID);     
            
            
            Ok(Response::new()
                .add_messages(msgs)
                .add_submessage(sub_msg)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

pub fn handle_lc_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Liquidity Check address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.liquidity_check = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);
                       
            //Instantiate Debt Auction
            let da_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(addrs.clone().governance.to_string()), 
                code_id: config.clone().mbrn_auction_id, 
                msg: to_binary(&DAInstantiateMsg {
                    owner: Some(addrs.clone().governance.to_string()),
                    positions_contract: addrs.clone().positions.to_string(),
                    oracle_contract: addrs.clone().oracle.to_string(),
                    osmosis_proxy: addrs.clone().osmosis_proxy.to_string(),
                    twap_timeframe: 60u64,
                    mbrn_denom: config.clone().mbrn_denom,
                    initial_discount: Decimal::percent(1),
                    discount_increase_timeframe: 15u64,
                    discount_increase: Decimal::percent(1),
                })?, 
                funds: vec![], 
                label: String::from("liquidation_queue"), 
            });
            let sub_msg = SubMsg::reply_on_success(da_instantiation, DEBT_AUCTION_REPLY_ID);     
            
            
            Ok(Response::new()
                .add_submessage(sub_msg)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}


pub fn handle_auction_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
    match msg.result.into_result() {
        Ok(result) => {
            let config = CONFIG.load(deps.storage)?;
            
            //Get contract address
            let instantiate_event = result
                .events
                .iter()
                .find(|e| {
                    e.attributes
                        .iter()
                        .any(|attr| attr.key == "method")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save MBRN Auction address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.mbrn_auction = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs);
            
            let mut msgs = vec![];

            //Add owners & new contracts to OP
            let msg = OPExecuteMsg::UpdateConfig { 
                owner: Some(vec![
                    addrs.clone().positions.to_string(), 
                    addrs.clone().vesting.to_string(), 
                    addrs.clone().staking.to_string(), 
                    addrs.clone().stability_pool.to_string(), 
                    addrs.clone().governance.to_string(), 
                    addrs.clone().mbrn_auction.to_string(),
                    ]), 
                add_owner: true, 
                debt_auction: Some(addrs.clone().mbrn_auction.to_string()), 
                positions_contract: Some(addrs.clone().positions.to_string()), 
                liquidity_contract: Some(addrs.clone().liquidity_check.to_string()),
            };
            let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().osmosis_proxy.to_string(), 
                msg: to_binary(&msg)?, 
                funds: vec![], 
            });
            msgs.push(msg);

            
            ////Add contracts to contract configurations & change owners to Governance
            msgs.push(
            CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().oracle.to_string(), 
                msg: to_binary(&OracleExecuteMsg::UpdateConfig { 
                    owner: Some(addrs.clone().governance.to_string()), 
                    positions_contract: Some(addrs.clone().positions.to_string()),
                    osmosis_proxy: None, 
                })?, 
                funds: vec![],
            }));
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().staking.to_string(), 
                    msg: to_binary(&StakingExecuteMsg::UpdateConfig { 
                        owner: Some(addrs.clone().governance.to_string()), 
                        positions_contract: Some(addrs.clone().positions.to_string()),
                        osmosis_proxy: None,
                        vesting_contract: Some(addrs.clone().vesting.to_string()),
                        governance_contract: Some(addrs.clone().governance.to_string()),
                        mbrn_denom: None,
                        staking_rate: None,
                        unstaking_period: None,
                        fee_wait_period: None,
                        dex_router: None,
                        max_spread: None, 
                    })?, 
                    funds: vec![],
                }));
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().vesting.to_string(), 
                    msg: to_binary(&VestingExecuteMsg::UpdateConfig { 
                        owner: Some(addrs.clone().governance.to_string()), 
                        osmosis_proxy: None,
                        mbrn_denom: None,
                        staking_contract: None,
                        additional_allocation: None, 
                    })?, 
                    funds: vec![],
                }));
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().positions.to_string(), 
                    msg: to_binary(&CDPExecuteMsg::UpdateConfig(CDPUpdateConfig {
                        owner: Some(addrs.clone().governance.to_string()), 
                        stability_pool: Some(addrs.clone().stability_pool.to_string()), 
                        dex_router: None,
                        osmosis_proxy: None,
                        debt_auction: Some(addrs.clone().mbrn_auction.to_string()), 
                        staking_contract: None,
                        oracle_contract: None,
                        liquidity_contract: Some(addrs.clone().liquidity_check.to_string()), 
                        discounts_contract: None, //TODO
                        liq_fee: None,
                        debt_minimum: None,
                        base_debt_cap_multiplier: None,
                        oracle_time_limit: None,
                        credit_twap_timeframe: None,
                        collateral_twap_timeframe: None,
                        cpc_multiplier: None,
                        rate_slope_multiplier: None,
                    }))?, 
                    funds: vec![],
                }));
            
            /////Query saved share tokens in Position's contract & add Supply Caps for them
            let basket = deps.querier.query_wasm_smart::<Basket>(
                addrs.clone().positions.to_string(), 
            &CDPQueryMsg::GetBasket {  }
            )?;
            let lp_supply_caps = basket.clone().collateral_types
                .into_iter()
                .filter(|cAsset| cAsset.pool_info.is_some())
                .collect::<Vec<cAsset>>()
                .into_iter()
                .map(|cAsset| SupplyCap {
                    asset_info: cAsset.asset.info,
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::one(),
                    lp: true,
                    stability_pool_ratio_for_debt_cap: Some(Decimal::percent(10)),
                })
                .collect::<Vec<SupplyCap>>();
            
            let msg = CDPExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                liq_queue: None,
                liquidity_multiplier: None,
                collateral_supply_caps: Some(lp_supply_caps),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: Some(false),
                cpc_margin_of_error: None,
                frozen: None,
                rev_to_stakers: None,
                multi_asset_supply_caps: None,
                credit_pool_ids: None,
            });
            let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().positions.to_string(), 
                msg: to_binary(&msg)?, 
                funds: vec![], 
            });
            msgs.push(msg);


            Ok(Response::new()
                .add_messages(msgs)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}
