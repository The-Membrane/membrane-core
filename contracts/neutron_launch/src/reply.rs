use std::convert::TryInto;
use std::str::FromStr;

use cosmwasm_std::{
    to_binary, Decimal, DepsMut, Env, WasmMsg, WasmQuery,
    Response, StdResult, Uint128, Reply, StdError, CosmosMsg, SubMsg, coins, QueryRequest, BankMsg,
};
use membrane::math::Uint256;
use crate::error::ContractError;
use crate::contracts::{POSITIONS_REPLY_ID, DEBT_AUCTION_REPLY_ID, SYSTEM_DISCOUNTS_REPLY_ID, DISCOUNT_VAULT_REPLY_ID, CREATE_DENOM_REPLY_ID, ORACLE_REPLY_ID, STAKING_REPLY_ID, LIQ_QUEUE_REPLY_ID, LTV_DISCO_REPLY_ID, TRANSMUTER_REPLY_ID, REVENUE_DISTRIBUTOR_REPLY_ID, ACQUISITION_REPLY_ID, YIELD_ARB_REPLY_ID, MARS_VT_REPLY_ID, POINTS_SYSTEM_REPLY_ID, EMISSIONS_VOTING_REPLY_ID, NO_ACTION_ID};
use crate::state::{ADDRESSES, CONFIG};

use membrane::staking::{InstantiateMsg as Staking_InstantiateMsg, ExecuteMsg as StakingExecuteMsg};
use membrane::vesting::ExecuteMsg as VestingExecuteMsg;
use membrane::cdp::{InstantiateMsg as CDP_InstantiateMsg, EditBasket, ExecuteMsg as CDPExecuteMsg, QueryMsg as CDPQueryMsg, UpdateConfig as CDPUpdateConfig, CreateBasket};
use membrane::neutron_oracle::{InstantiateMsg as Oracle_InstantiateMsg, ExecuteMsg as OracleExecuteMsg};
use membrane::liq_queue::{InstantiateMsg as LQInstantiateMsg, ExecuteMsg as LQExecuteMsg};
use membrane::mars_vault_token::{InstantiateMsg as MarsVT_InstantiateMsg, QueryMsg as MarsVT_QueryMsg};
use membrane::ltv_disco::{InstantiateMsg as LTVDisco_InstantiateMsg, ExecuteMsg as LTVDiscoExecuteMsg};
use membrane::transmuter::{InstantiateMsg as Transmuter_InstantiateMsg, ExecuteMsg as TransmuterExecuteMsg};
use membrane::revenue_distributor::{InstantiateMsg as RevenueDistributor_InstantiateMsg, ExecuteMsg as RevenueDistributorExecuteMsg, RDVaultInfoMessage, RevenueDestination as RevenueDistributorRevenueDestination};
use membrane::acquisition::{InstantiateMsg as Acquisition_InstantiateMsg, ExecuteMsg as Acquisition_ExecuteMsg};
use membrane::yield_arb::{InstantiateMsg as YieldArb_InstantiateMsg, ExecuteMsg as YieldArb_ExecuteMsg};
use membrane::points_system::{InstantiateMsg as PointsSystem_InstantiateMsg, ExecuteMsg as PointsSystem_ExecuteMsg};
use membrane::auction::{InstantiateMsg as DAInstantiateMsg, ExecuteMsg as DAExecuteMsg, UpdateConfig as AuctionUpdateConfig};
use membrane::neutron_proxy::{ExecuteMsg as ProxyExecuteMsg, QueryMsg as ProxyQueryMsg, ContractDenomsResponse};
use membrane::system_discounts::InstantiateMsg as SystemDiscountInstantiateMsg;
use membrane::discount_vault::{InstantiateMsg as DiscountVaultInstantiateMsg, ExecuteMsg as DiscountVaultExecuteMsg};
use membrane::emissions_voting::{InstantiateMsg as EmissionsVoting_InstantiateMsg, ExecuteMsg as EmissionsVotingExecuteMsg, GraphType};
use membrane::types::{Asset, AssetInfo, AssetOracleInfo, Basket, DepositDenom, DistributionEntry, LPAssetInfo, LiqAsset, NeutronOwner, PoolInfo, RevenueDestination, StringEntry, SupplyCap, TWAPPoolInfo, VaultEntry, VaultInfo, cAsset};
use membrane::neutron_proxy::NeutronOwnerEntry;
use membrane::transmuter::AssetPair;

const NEUTRON_USDC_DENOM: &str = "ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81";


/// Create Membrane denoms and instantiate oracle contract
pub fn handle_np_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Proxy address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.neutron_proxy = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            let mut sub_msgs = vec![];

            //Create CDT & MBRN denom
            let create_denom_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().neutron_proxy.to_string(), 
                msg: to_binary(&ProxyExecuteMsg::CreateDenom { 
                    subdenom: String::from("ucdt"), 
                    max_supply: None,
                })?, 
                funds: vec![],
            });            
            let create_denom_submsg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().neutron_proxy.to_string(), 
                msg: to_binary(&ProxyExecuteMsg::CreateDenom { 
                    subdenom: String::from("umbrn"), 
                    max_supply: Some(Uint128::new(80_000_000_000_000)),
                })?, 
                funds: vec![],
            });
            sub_msgs.push(SubMsg::reply_on_success(create_denom_submsg, CREATE_DENOM_REPLY_ID));

            //Instantiate Oracle
            let oracle_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()), 
                code_id: config.clone().neutron_oracle_id, 
                msg: to_binary(&Oracle_InstantiateMsg {
                    owner: None,
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


/// Called after the Osmosis Proxy (OP) reply to save created denoms
pub fn handle_create_denom_reply(deps: DepsMut, _env: Env, msg: Reply) -> StdResult<Response>{ 
    match msg.result.into_result() {
        Ok(_result) => {
        let mut config = CONFIG.load(deps.storage)?;
        let addrs = ADDRESSES.load(deps.storage)?;
        
        //Get denoms
        let res: ContractDenomsResponse = deps.querier.query_wasm_smart(addrs.neutron_proxy, &ProxyQueryMsg::GetContractDenoms { limit: None })?;
        //We know CDT is first
        config.credit_denom = res.denoms[0].clone();
        config.mbrn_denom = res.denoms[1].clone();

        //Save config
        CONFIG.save(deps.storage, &config)?;

        Ok(Response::new()
            .add_attribute("saved_denoms", format!("{:?}", res.denoms))
        )
    },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}


/// Instantiate Staking Contract
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Oracle address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.oracle = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            //Instantiate Staking
            let staking_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().staking_id, 
                msg: to_binary(&Staking_InstantiateMsg {
                    owner: None,
                    positions_contract: None,
                    auction_contract: None,
                    vesting_contract: None,
                    governance_contract: None,
                    osmosis_proxy: Some(addrs.neutron_proxy.to_string()), //param - using neutron_proxy address for osmosis_proxy field
                    emissions_voting_contract: None,
                    incentive_schedule: None,
                    unstaking_period: None,
                    mbrn_denom: config.clone().mbrn_denom,
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

/// Instantiate Positions Contract
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Staking address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.staking = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            //Instantiate Positions/CDP
            let cdp_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().positions_id, 
                msg: to_binary(&CDP_InstantiateMsg {
                    owner: None,
                    liq_fee: Decimal::zero(),
                    oracle_time_limit: 600u64,  
                    debt_minimum: Uint128::new(20u128),  
                    collateral_twap_timeframe: 60u64, 
                    credit_twap_timeframe: 480u64,  
                    rate_slope_multiplier: Decimal::from_str("1.618").unwrap(), 
                    base_debt_cap_multiplier: Uint128::new(1000_000_000u128), 
                    staking_contract: Some(addrs.clone().staking.to_string()),
                    oracle_contract: Some(addrs.clone().oracle.to_string()),
                    chain_proxy: Some(addrs.clone().neutron_proxy.to_string()),
                    debt_auction: None,
                    liquidity_contract: None,
                    discounts_contract: None,
                    ltv_disco: String::new(), 
                    create_basket: CreateBasket {
                        basket_id: Uint128::one(),
                        collateral_types: vec![],
                        credit_asset: Asset {
                            info: AssetInfo::NativeToken { denom: config.clone().credit_denom },
                            amount: Uint128::zero(),
                        },
                        credit_price: Decimal::one(),
                        base_interest_rate: Some(Decimal::percent(3)), 
                        credit_pool_infos: vec![],
                        liq_queue: None,
                    },
                })?, 
                funds: vec![], 
                label: String::from("positions"), 
            });
            let sub_msg = SubMsg::reply_on_success(cdp_instantiation, POSITIONS_REPLY_ID);
            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}


/// Instantiate LTV Disco contract
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save CDP/Positions address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.positions = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            //Instantiate Liquidation Queue
            let lq_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().liq_queue_id, 
                msg: to_binary(&LQInstantiateMsg {
                    owner: Some(config.owner.to_string()),
                    positions_contract: addrs.clone().positions.to_string(),
                    osmosis_proxy_contract: addrs.clone().neutron_proxy.to_string(), //param - using neutron_proxy as osmosis_proxy
                    waiting_period: 60u64, //param
                    minimum_bid: Uint128::new(5_000_000), //param
                    maximum_waiting_bids: 5_000u64, //param
                })?, 
                funds: vec![], 
                label: String::from("liquidation_queue"), 
            });
            let sub_msg = SubMsg::reply_on_success(lq_instantiation, LIQ_QUEUE_REPLY_ID);    

            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}


/// Add LQ to Basket and configure it
/// Note: LQ is instantiated elsewhere in the chain
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save LQ address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.liq_queue = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            //Instantiate LTV Disco
            let ltv_disco_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().ltv_disco_id, 
                msg: to_binary(&LTVDisco_InstantiateMsg {
                    owner: None,
                    cdp_contract: addrs.clone().positions.to_string(),
                    deposit_denom: DepositDenom {
                        denom: config.clone().mbrn_denom.clone(), //param
                        vault_info: None, //param - CDT is not a vault token
                    },
                    cdt_denom: config.clone().credit_denom.clone(),
                    minimum_deposit: Uint128::new(5_000_000u128), //param
                    max_ltv: Decimal::percent(90), //param
                    percent_to_disperse: Decimal::percent(20), //param
                    dispersal_window: 24u64, //param - hours
                    activation_window: 1u64, //param - hours
                    oracle_contract: addrs.clone().oracle.to_string(),
                    chain_proxy_contract: addrs.clone().neutron_proxy.to_string(),
                    emissions_voting_contract: None,
                    lock_duration_ceiling: Some(365u64), //param - days
                    affiliate_fee: Some(Decimal::percent(1)), //param
                    max_management_fee: Some(Decimal::percent(5)), //param
                    ltv_delta_minimum: None, //param - use default
                    points_system_contract: None, // Will be set later via UpdateConfig
                    revenue_distributor: None, // Will be set later via UpdateConfig
                    auction_contract: None, // Will be set later via UpdateConfig after auction is deployed
                    mbrn_denom: Some(config.clone().mbrn_denom.clone()), // MBRN denom for AddDepositTokenRevenue
                })?, 
                funds: vec![], 
                label: String::from("ltv_disco"), 
            });
            let sub_msg = SubMsg::reply_on_success(ltv_disco_instantiation, LTV_DISCO_REPLY_ID);

            let mut msgs = vec![];
            //Add positions contract to oracle contract to use EditBasket
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().oracle.to_string(), 
                    msg: to_binary(&OracleExecuteMsg::UpdateConfig { 
                        owner: None, 
                        positions_contract: Some(addrs.clone().positions.to_string()),
                    })?, 
                    funds: vec![],
                }));
            
            //Add LQ to Basket 
            let msg = CDPExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                liq_queue: Some(addrs.clone().liq_queue.to_string()),
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: Some(false),
                cpc_margin_of_error: Some(Decimal::percent(1)),
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                credit_pool_infos: Some(vec![]), //param - empty for now, can be configured later
                take_revenue: None,
            });
            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.clone().positions.to_string(),
                msg: to_binary(&msg)?,
                funds: vec![],
            });
            msgs.push(msg);

            //Liquidity Check removed - no longer needed
            
            Ok(Response::new()
                .add_messages(msgs)
                .add_submessage(sub_msg)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}


/// Instantiate Transmuter contract
pub fn handle_ltv_disco_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save LTV Disco address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.ltv_disco = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            //Instantiate Transmuter
            let transmuter_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().transmuter_id, 
                msg: to_binary(&Transmuter_InstantiateMsg {
                    owner: None,
                    tokenfactory_contract: None,
                    discounts_contract: String::new(), //param - will be set after system_discounts is instantiated
                    cdp_contract: addrs.clone().positions.to_string(),
                    vault_subdenom: String::from("usdc-transmuter"), 
                    deposit_pair: AssetPair {
                        cdt: config.clone().credit_denom, 
                        paired_asset:  String::from(NEUTRON_USDC_DENOM),
                    },
                    composition_leeway: Decimal::percent(5), 
                    asset_a_to_b_rate: Decimal::one(), 
                    cdt_target_ratio: Decimal::zero(), 
                    // 0.05% swap fee gives the market room to create public liquidity
                    usage_fee: Some(Decimal::from_str("0.0005").unwrap()),
                    usage_fee_utilization_threshold: Some(Decimal::percent(90)),
                    swap_history_cap: 100u32,  
                    volume_history_cap: 100u32, 
                    rate_limit_window_secs: Some(60 * 60 * 8),
                    rate_limit_threshold: Some(Decimal::percent(5)), 
                    revenue_distributor_addr: None, // Will be set later
                    revenue_distributions: None,
                    allowlist: None,
                    allowlist_rate_limit_threshold: None,
                    global_rate_limit_window_secs: Some(86400u64), 
                    global_rate_limit_threshold: Some(Decimal::percent(20)), 
                    lock_ceiling: 1460u64, 
                    affiliate_fee: Decimal::percent(1), 
                    send_swap_fee: Some(true), 
                })?, 
                funds: vec![], 
                label: String::from("transmuter"), 
            });
            let sub_msg = SubMsg::reply_on_success(transmuter_instantiation, TRANSMUTER_REPLY_ID);

            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Instantiate Revenue Distributor contract
pub fn handle_transmuter_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Transmuter address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.transmuter = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            //Query transmuter vault token denom - we'll need to get this from the transmuter
            //For now, construct it based on the pattern
            let vault_token_denom = format!("factory/{}/vt", addrs.transmuter.to_string());

            //Instantiate Revenue Distributor with 100% to LTV Disco
            // Note: ltv_distributions will be set later via SetPromises after Mars VT is instantiated
            let revenue_distributor_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().revenue_distributor_id, 
                msg: to_binary(&RevenueDistributor_InstantiateMsg {
                    owner: config.owner.to_string(),  
                    canonical_asset: Asset {
                        info: AssetInfo::NativeToken { denom: config.clone().credit_denom.clone() },
                        amount: Uint128::zero(),
                    },
                    revenue_destinations: vec![
                        RevenueDistributorRevenueDestination {
                            destination: addrs.clone().ltv_disco,
                            distribution_ratio: Decimal::percent(100), 
                        }
                    ],
                    ltv_disco: addrs.clone().ltv_disco.to_string(),
                    transmuter_vault: RDVaultInfoMessage {
                        vault_addr: addrs.clone().transmuter.to_string(),
                        deposit_token: config.clone().credit_denom,
                        vault_token: vault_token_denom,
                    },
                    points_system_contract: None, // Will be set later via UpdateConfig
                    cdp_contract: None, // Will be set later via UpdateConfig
                    revenue_dispersal_window: None, // Will be set later via UpdateConfig
                    acquisition_contract: None, // Will be set later via UpdateConfig
                    ltv_disco_contract: None, // Will be set later via UpdateConfig
                    auction_contract: None, // Will be set later via UpdateConfig after auction is deployed
                })?, 
                funds: vec![], 
                label: String::from("revenue_distributor"), 
            });
            let sub_msg = SubMsg::reply_on_success(revenue_distributor_instantiation, REVENUE_DISTRIBUTOR_REPLY_ID);

            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Instantiate Transmuter Lockdrop contract
pub fn handle_revenue_distributor_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Revenue Distributor address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.revenue_distributor = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            //Instantiate Transmuter Lockdrop
            let acquisition_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().acquisition_id, 
                msg: to_binary(&Acquisition_InstantiateMsg {
                    owner: config.owner.to_string(),  
                    transmuter_contract: addrs.clone().transmuter.to_string(),
                    neutron_proxy: addrs.clone().neutron_proxy.to_string(),
                    lockdrop_incentive_size: Uint128::new(10_000_000_000_000u128),  
                    deposit_period_days: 5u64, 
                    withdrawal_period_days: 2u64,  
                    deposit_token: config.clone().credit_denom,
                    minimum_deposit: Uint128::new(1000_000u128),  
                    mbrn_denom: config.clone().mbrn_denom,
                    staking_contract: Some(addrs.clone().staking.to_string()),
                    mars_mirror_contract: None,
                    ltv_disco_contract: Some(addrs.clone().ltv_disco.to_string()),
                    discounts_contract: addrs.clone().system_discounts.to_string(),
                    maximum_boost: Decimal::percent(200),  
                    minimum_lock_days: 30u64,
                    emissions_voting_contract: None, // Set later via UpdateConfig in handle_emissions_voting_reply
                })?, 
                funds: vec![], 
                label: String::from("acquisition"), 
            });
            let sub_msg = SubMsg::reply_on_success(acquisition_instantiation, ACQUISITION_REPLY_ID);
            
            Ok(Response::new()
                .add_submessage(sub_msg)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Instantiate Yield Arb contract
pub fn handle_acquisition_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Transmuter Lockdrop address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.acquisition = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;
                       
            //Instantiate Yield Arb
            let yield_arb_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().yield_arb_id, 
                msg: to_binary(&YieldArb_InstantiateMsg {
                    owner: None,
                    cdt_denom: config.clone().credit_denom,
                    usdc_denom:  String::from(NEUTRON_USDC_DENOM), //param - Get the direct denom
                    mars_vault_addr: String::new(), // paramWill be set to Mars VT later
                    cdp_contract_addr: addrs.clone().positions.to_string(),
                    transmuter_addr: addrs.clone().transmuter.to_string(),
                })?, 
                funds: vec![], 
                label: String::from("yield_arb"), 
            });
            let sub_msg = SubMsg::reply_on_success(yield_arb_instantiation, YIELD_ARB_REPLY_ID);

            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Instantiate Mars Vault Token contract
pub fn handle_yield_arb_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Yield Arb address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.yield_arb = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;         

            //Instantiate Mars Vault Token
            let mars_vt_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().mars_vault_token_id, 
                msg: to_binary(&MarsVT_InstantiateMsg {
                    vault_subdenom: String::from("mars-usdc"), //param
                    deposit_token:  String::from(NEUTRON_USDC_DENOM),  
                    mars_redbank_addr: String::from(""), //param - Mars RedBank address
                    transmuter_addr: addrs.clone().transmuter.to_string(),
                    revenue_distributor_addr: addrs.clone().revenue_distributor.to_string(),
                    cdt_denom: config.clone().credit_denom,
                    cdp_contract_addr: addrs.clone().positions.to_string(),
                    revenue_distributions: vec![
                         LiqAsset {
                                info: AssetInfo::NativeToken { denom:  String::from(NEUTRON_USDC_DENOM) }, //param - (Get the direct denom)
                                amount: Decimal::percent(100), 
                            }
                    ]
                })?, 
                funds: vec![], 
                label: String::from("mars_vault_token"), 
            });
            let sub_msg = SubMsg::reply_on_success(mars_vt_instantiation, MARS_VT_REPLY_ID);

            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Instantiate Points System contract
pub fn handle_mars_vt_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Mars VT address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.mars_vault_token = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            //Instantiate Points System
            let points_system_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().points_system_id, 
                msg: to_binary(&PointsSystem_InstantiateMsg {
                    cdt_denom: config.clone().credit_denom,
                    mbrn_denom: config.clone().mbrn_denom,
                    oracle_contract: addrs.clone().oracle.to_string(),
                    positions_contract: addrs.clone().positions.to_string(),
                    stability_pool_contract: String::new(), // Not used in new flow
                    liq_queue_contract: addrs.clone().liq_queue.to_string(),
                    governance_contract: String::new(), // Not used in new flow
                    osmosis_proxy_contract: addrs.clone().neutron_proxy.to_string(),
                    emissions_voting_contract: None, // Will be set later via UpdateConfig
                })?, 
                funds: vec![], 
                label: String::from("points_system"), 
            });
            let sub_msg = SubMsg::reply_on_success(points_system_instantiation, POINTS_SYSTEM_REPLY_ID);

            Ok(Response::new().add_submessage(sub_msg))
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Final wiring: Update contract configs, whitelist yield arb in transmuter, add Mars VT to oracle/CDP/yield arb
pub fn handle_points_system_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Points System address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.points_system = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;
                       
            //Instantiate System Discounts
            let system_discounts_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().system_discounts_id, 
                msg: to_binary(&SystemDiscountInstantiateMsg {
                    owner: Some(config.owner.to_string()),  
                    oracle_contract: addrs.clone().oracle.to_string(),
                    positions_contract: addrs.clone().positions.to_string(),
                    staking_contract: addrs.clone().staking.to_string(),
                    lockdrop_contract: None,
                    discount_vault_contract: None, // Discount vault is no longer used
                    ltv_disco_contract: Some(addrs.clone().ltv_disco.to_string()), 
                    minimum_time_in_network: 7u64, 
                    max_discount: Some(Decimal::percent(75)), 
                    mbrn_at_max_discount: None, 
                    max_boost: None, 
                })?, 
                funds: vec![], 
                label: String::from("system_discounts"), 
            });
            let sub_msg = SubMsg::reply_on_success(system_discounts_instantiation, SYSTEM_DISCOUNTS_REPLY_ID);     
            
            Ok(Response::new()
                .add_submessage(sub_msg)
                .add_attribute("points_system", addrs.points_system.to_string())
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Handle Emissions Voting instantiation and create voting graphs
pub fn handle_emissions_voting_reply(deps: DepsMut, _env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Emissions Voting address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.emissions_voting = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            let mut sub_msgs: Vec<SubMsg> = vec![];

            // Create graphs for PointsMultipliers Decimal fields
            let points_multiplier_fields = vec![
                "interest_rate",
                "liquidation_execution",
                "liquidation_claims",
                "governance_votes",
                "transmuter_swap_fees",
                "disco_revenue",
            ];

            for field in points_multiplier_fields {
                let create_graph_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: addrs.emissions_voting.to_string(),
                    msg: to_binary(&EmissionsVotingExecuteMsg::CreateGraph {
                        label: field.to_string(),
                        graph_type: GraphType::Decimal,
                        range_min: "1.0".to_string(),
                        range_max: "10.0".to_string(),
                        period_days: 7,
                        callback_contract: addrs.points_system.to_string(),
                    })?,
                    funds: vec![],
                });
                sub_msgs.push(SubMsg::new(create_graph_msg));
            }

            // Create graph for acquisition (Uint128)
            let lockdrop_graph_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.emissions_voting.to_string(),
                msg: to_binary(&EmissionsVotingExecuteMsg::CreateGraph {
                    label: "acquisition".to_string(),
                    graph_type: GraphType::Uint128,
                    range_min: "0".to_string(),
                    range_max: "1000000000000".to_string(), // 1M with 6 decimals
                    period_days: 7,
                    callback_contract: addrs.acquisition.to_string(),
                    persistent_voting: None,
                })?,
                funds: vec![],
            });
            sub_msgs.push(SubMsg::new(lockdrop_graph_msg));

            // Create graph for transmuter_total_emissions (Uint128) with persistent_voting=true
            let total_emissions_graph_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.emissions_voting.to_string(),
                msg: to_binary(&EmissionsVotingExecuteMsg::CreateGraph {
                    label: membrane::transmuter::TOTAL_EMISSIONS_GRAPH_LABEL.to_string(),
                    graph_type: GraphType::Uint128,
                    range_min: "0".to_string(),
                    range_max: "100_000_000_000_000000".to_string(), // Adjust as needed
                    period_days: 30, // Monthly
                    callback_contract: addrs.transmuter.to_string(),
                    persistent_voting: Some(true),
                })?,
                funds: vec![],
            });
            sub_msgs.push(SubMsg::new(total_emissions_graph_msg));

            // Create graph for transmuter_acquisition_percentage (Decimal) with persistent_voting=true
            let acquisition_percentage_graph_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.emissions_voting.to_string(),
                msg: to_binary(&EmissionsVotingExecuteMsg::CreateGraph {
                    label: membrane::transmuter::ACQUISITION_PERCENTAGE_GRAPH_LABEL.to_string(),
                    graph_type: GraphType::Decimal,
                    range_min: "0.0".to_string(),
                    range_max: "0.2".to_string(), // 0-20%
                    period_days: 30, // Monthly
                    callback_contract: addrs.transmuter.to_string(),
                    persistent_voting: Some(true),
                })?,
                funds: vec![],
            });
            sub_msgs.push(SubMsg::new(acquisition_percentage_graph_msg));

            // Update transmuter config to set emissions_voting_contract
            let update_transmuter_config_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.transmuter.to_string(),
                msg: to_binary(&TransmuterExecuteMsg::UpdateConfig {
                    owner: None,
                    deposit_pair: None,
                    composition_leeway: None,
                    cdt_target_ratio: None,
                    tokenfactory_contract: None,
                    discounts_contract: None,
                    cdp_contract: None,
                    usage_fee: None,
                    usage_fee_utilization_threshold: None,
                    swap_history_cap: None,
                    volume_history_cap: None,
                    rate_limit_window_secs: None,
                    rate_limit_threshold: None,
                    allowlist: None,
                    allowlist_rate_limit_threshold: None,
                    global_rate_limit_window_secs: None,
                    global_rate_limit_threshold: None,
                    revenue_distributor_addr: None,
                    revenue_distributions: None,
                    lock_ceiling: None,
                    affiliate_fee: Decimal::percent(1), // Required field
                    send_swap_fee: None,
                    revenue_distributor_fee_percentage: None,
                    emissions_voting_contract: Some(addrs.emissions_voting.to_string()),
                    acquisition_contract: None,
                    points_system_contract: Some(addrs.points_system.to_string()),
                })?,
                funds: vec![],
            });
            sub_msgs.push(SubMsg::new(update_transmuter_config_msg));

            // Update points system config to set emissions_voting_contract
            let update_points_config_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.points_system.to_string(),
                msg: to_binary(&PointsSystem_ExecuteMsg::UpdateConfig {
                    owner: None,
                    cdt_denom: None,
                    mbrn_denom: None,
                    oracle_contract: None,
                    positions_contract: None,
                    stability_pool_contract: None,
                    liq_queue_contract: None,
                    governance_contract: None,
                    osmosis_proxy_contract: None,
                    transmuter_contract: None,
                    ltv_disco_contract: None,
                    system_discounts_contract: None,
                    emissions_voting_contract: Some(addrs.emissions_voting.to_string()),
                    revenue_distributor_contract: Some(addrs.revenue_distributor.to_string()),
                    mbrn_per_point: None,
                    max_mbrn_distribution: None,
                    points_per_dollar: None,
                    points_multipliers: None,
                })?,
                funds: vec![],
            });
            
            // Update acquisition config to set emissions_voting_contract
            let update_lockdrop_config_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.acquisition.to_string(),
                msg: to_binary(&Acquisition_ExecuteMsg::UpdateConfig {
                    owner: None,
                    transmuter_contract: None,
                    neutron_proxy: None,
                    lockdrop_incentive_size: None,
                    deposit_period_days: None,
                    withdrawal_period_days: None,
                    deposit_token: None,
                    minimum_deposit: None,
                    mbrn_denom: None,
                    staking_contract: None,
                    mars_mirror_contract: None,
                    ltv_disco_contract: None,
                    discounts_contract: None,
                    maximum_boost: None,
                    minimum_lock_days: None,
                    emissions_voting_contract: Some(addrs.emissions_voting.to_string()),
                })?,
                funds: vec![],
            });
            
            // Update staking config to set emissions_voting_contract
            let update_staking_config_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.staking.to_string(),
                msg: to_binary(&StakingExecuteMsg::UpdateConfig {
                    owner: None,
                    positions_contract: None,
                    auction_contract: None,
                    vesting_contract: None,
                    governance_contract: None,
                    osmosis_proxy: None,
                    emissions_voting_contract: Some(addrs.emissions_voting.to_string()),
                    mbrn_denom: None,
                    incentive_schedule: None,
                    unstaking_period: None,
                    max_commission_rate: None,
                    keep_raw_cdt: None,
                    vesting_rev_multiplier: None,
                    buyback_and_burn: None,
                })?,
                funds: vec![],
            });
            
            // Update ltv_disco config to set emissions_voting_contract and revenue_distributor
            let update_disco_config_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.ltv_disco.to_string(),
                msg: to_binary(&LTVDiscoExecuteMsg::UpdateConfig {
                    owner: None,
                    cdp_contract: None,
                    deposit_denom: None,
                    cdt_denom: None,
                    minimum_deposit: None,
                    percent_to_disperse: None,
                    dispersal_window: None,
                    activation_window: None,
                    oracle_contract: None,
                    chain_proxy_contract: None,
                    emissions_voting_contract: Some(addrs.emissions_voting.to_string()),
                    lock_duration_ceiling: None,
                    affiliate_fee: None,
                    max_management_fee: None,
                    ltv_delta_minimum: None,
                    points_system_contract: Some(addrs.points_system.to_string()),
                    revenue_distributor: Some(addrs.revenue_distributor.to_string()),
                    auction_contract: Some(addrs.mbrn_auction.to_string()),
                    mbrn_denom: Some(config.mbrn_denom.clone()),
                })?,
                funds: vec![],
            });
            
            // Update revenue distributor config to set points_system_contract, cdp_contract, lockdrop, and disco addresses
            let update_revenue_distributor_config_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.revenue_distributor.to_string(),
                msg: to_binary(&RevenueDistributorExecuteMsg::UpdateConfig {
                    revenue_destinations: None,
                    ltv_disco: None,
                    transmuter_vault: None,
                    points_system_contract: Some(addrs.points_system.to_string()),
                    cdp_contract: Some(addrs.positions.to_string()),
                    revenue_dispersal_window: Some(7u64),
                    acquisition_contract: Some(addrs.acquisition.to_string()),
                    ltv_disco_contract: Some(addrs.ltv_disco.to_string()),
                    auction_contract: Some(addrs.mbrn_auction.to_string()),
                })?,
                funds: vec![],
            });
            
            // Update CDP config to set revenue_distributor and points_contract
            let update_cdp_config_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.positions.to_string(),
                msg: to_binary(&CDPExecuteMsg::UpdateConfig(CDPUpdateConfig {
                    owner: None,
                    staking_contract: None,
                    oracle_contract: None,
                    chain_proxy: None,
                    debt_auction: None,
                    liquidity_contract: None,
                    discounts_contract: None,
                    ltv_disco: None,
                    revenue_distributor: Some(addrs.revenue_distributor.to_string()),
                    liq_fee: None,
                    oracle_time_limit: None,
                    cpc_multiplier: None,
                    rate_slope_multiplier: None,
                    debt_minimum: None,
                    base_debt_cap_multiplier: None,
                    collateral_twap_timeframe: None,
                    credit_twap_timeframe: None,
                    affiliate_fee_max: None,
                    skip_credit_price_accrual: None,
                    liquidation_stat_limit: None,
                    transmuter_addr: None,
                    irm_config: None,
                    points_contract: Some(addrs.points_system.to_string()),
                }))?,
                funds: vec![],
            });
            
            Ok(Response::new()
                .add_submessages(sub_msgs)
                .add_message(update_points_config_msg)
                .add_message(update_lockdrop_config_msg)
                .add_message(update_staking_config_msg)
                .add_message(update_disco_config_msg)
                .add_message(update_revenue_distributor_config_msg)
                .add_message(update_cdp_config_msg)
                .add_attribute("emissions_voting", addrs.emissions_voting.to_string())
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Instantiate Discount Vault
pub fn handle_discount_vault_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save Discount Vault address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.discount_vault = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;
                       
            //Instantiate System Discounts
            let system_discounts_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().system_discounts_id, 
                msg: to_binary(&SystemDiscountInstantiateMsg {
                    owner: Some(config.owner.to_string()),  
                    oracle_contract: addrs.clone().oracle.to_string(),
                    positions_contract: addrs.clone().positions.to_string(),
                    staking_contract: addrs.clone().staking.to_string(),
                    lockdrop_contract: None,
                    discount_vault_contract: Some(addrs.clone().discount_vault.to_string()),
                    ltv_disco_contract: Some(addrs.clone().ltv_disco.to_string()), 
                    minimum_time_in_network: 7u64, 
                    max_discount: Some(Decimal::percent(75)), 
                    mbrn_at_max_discount: None, 
                    max_boost: None, 
                })?, 
                funds: vec![], 
                label: String::from("system_discounts"), 
            });
            let sub_msg = SubMsg::reply_on_success(system_discounts_instantiation, SYSTEM_DISCOUNTS_REPLY_ID);     
            
            
            Ok(Response::new()
                .add_submessage(sub_msg)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Instantiate Debt Auction
pub fn handle_system_discounts_reply(deps: DepsMut, env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;
            //Save System Discounts address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.system_discounts = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;

            let mut msgs: Vec<CosmosMsg> = vec![];

            //Update Transmuter with system_discounts address
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().transmuter.to_string(), 
                    msg: to_binary(&TransmuterExecuteMsg::UpdateConfig {
                        owner: None,
                        deposit_pair: None,
                        composition_leeway: None,
                        asset_a_to_b_rate: None,
                        cdt_target_ratio: None,
                        tokenfactory_contract: None,
                        discounts_contract: Some(addrs.clone().system_discounts.to_string()),
                        cdp_contract: None,
                        usage_fee: None,
                        usage_fee_utilization_threshold: None,
                        swap_history_cap: None,
                        volume_history_cap: None,
                        rate_limit_window_secs: None,
                        rate_limit_threshold: None,
                        allowlist: Some(vec![
                            StringEntry {
                                entry: addrs.clone().yield_arb.to_string(),
                                remove: false,
                            }
                        ]),
                        allowlist_rate_limit_threshold: None,
                        global_rate_limit_window_secs: None,
                        global_rate_limit_threshold: None,
                        revenue_distributor_addr: None,
                        revenue_distributions: None,
                        lock_ceiling: None,
                        affiliate_fee: Decimal::percent(1), // Required field
                        send_swap_fee: None,
                    })?, 
                    funds: vec![],
                })
            );

            // Update yield arb with Mars VT address
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().yield_arb.to_string(), 
                    msg: to_binary(&YieldArb_ExecuteMsg::UpdateConfig {
                        owner: None,
                        cdt_denom: None,
                        usdc_denom: None,
                        mars_vault_addr: Some(addrs.clone().mars_vault_token.to_string()),
                        cdp_contract_addr: None,
                        transmuter_addr: None,
                        vault_cost_index: None,
                    })?, 
                    funds: vec![],
                })
            );

            // Query Mars VT contract to get vault token denom
            let mars_vt_config: membrane::mars_vault_token::Config = deps.querier.query_wasm_smart(
                addrs.clone().mars_vault_token.to_string(),
                &MarsVT_QueryMsg::Config {},
            )?;
            let mars_vt_denom = mars_vt_config.vault_token;

            // Configure Transmuter revenue distributions to send to revenue distributor with mars_vt_denom
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().transmuter.to_string(), 
                    msg: to_binary(&TransmuterExecuteMsg::UpdateConfig {
                        owner: None,
                        deposit_pair: None,
                        composition_leeway: None,
                        asset_a_to_b_rate: None,
                        cdt_target_ratio: None,
                        tokenfactory_contract: None,
                        discounts_contract: None,
                        cdp_contract: None,
                        usage_fee: None,
                        usage_fee_utilization_threshold: None,
                        swap_history_cap: None,
                        volume_history_cap: None,
                        rate_limit_window_secs: None,
                        rate_limit_threshold: None,
                        allowlist: None,
                        allowlist_rate_limit_threshold: None,
                        global_rate_limit_window_secs: None,
                        global_rate_limit_threshold: None,
                        revenue_distributor_addr: Some(addrs.clone().revenue_distributor.to_string()),
                        revenue_distributions: Some(vec![
                            DistributionEntry {
                                asset: LiqAsset {
                                    info: AssetInfo::NativeToken { denom: mars_vt_denom.clone() },
                                    amount: Decimal::percent(100), 
                                },
                                remove: false,
                            }
                        ]),
                        lock_ceiling: None,
                        affiliate_fee: Decimal::percent(1), // Required field
                        send_swap_fee: Some(true), // Ensure swap fees are sent to revenue distributor (which sends 100% to LTV disco)
                    })?, 
                    funds: vec![], 
                })
            );

            // Configure revenue distributor to send swap fees to marsUSDC disco market
            // This sets up the LTV_DISCO_DISTRIBUTION map so revenue goes to the correct market
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().revenue_distributor.to_string(), 
                    msg: to_binary(&RevenueDistributorExecuteMsg::SetPromises {
                        promises: vec![], // Empty promises - we're just setting up the distribution map
                        ltv_disco_distribution: Some(vec![
                            Asset {
                                info: AssetInfo::NativeToken { denom: mars_vt_denom.clone() },
                                amount: Uint128::new(1_000_000_000_000u128), // Ratio amount - this sets 100% since it's the only asset
                            }
                        ]),
                    })?, 
                    funds: vec![],
                })
            );

            // Add Mars VT to oracle
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().oracle.to_string(), 
                    msg: to_binary(&OracleExecuteMsg::AddAsset { 
                        asset_info: mars_vt_denom.clone(), 
                        oracle_info: membrane::types::NeutronOracleInfo { 
                            is_usd_par: false, //param
                            decimals: 12u64,
                            slinky_base_symbol: Some(String::from("USDC")), //param - confirm correct symbol
                            slinky_max_blocks_old: Some(255u8), //param - can be set later if needed (max u8 value)
                        },
                    })?, 
                    funds: vec![],
                })
            );

            // Add Mars VT to CDP as collateral and set Mars VT contract as interest rate manager
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().positions.to_string(), 
                    msg: to_binary(&CDPExecuteMsg::EditBasket(EditBasket {
                        added_cAsset: Some(cAsset {
                            asset: Asset {
                                info: AssetInfo::NativeToken { denom: mars_vt_denom.clone() },
                                amount: Uint128::zero(),
                            },
                            max_borrow_LTV: Decimal::percent(90), 
                            max_LTV: Decimal::percent(96), 
                            pool_info: None,
                            rate_index: Decimal::one(),
                            force_redemptions: Some(true),
                        }),
                        liq_queue: None,
                        collateral_supply_caps: Some(vec![
                            SupplyCap {
                                asset_info: AssetInfo::NativeToken { denom: mars_vt_denom.clone() },
                                current_supply: Uint128::zero(),
                                debt_total: Uint128::zero(),
                                supply_cap_ratio: Decimal::percent(100),
                                lp: false,
                                stability_pool_ratio_for_debt_cap: None,
                            }
                        ]),
                        multi_asset_supply_caps: None,
                        base_interest_rate: None,
                        credit_asset_twap_price_source: None,
                        negative_rates: None,
                        cpc_margin_of_error: None,
                        frozen: None,
                        distribute_revenue: None,
                        credit_pool_infos: Some(vec![]), //param - empty for now, can be configured later
                        take_revenue: None,
                    }))?,
                    funds: vec![],
                })
            );

            //AddQueues for Mars VT
            let msg = LQExecuteMsg::AddQueue { 
                bid_for: AssetInfo::NativeToken { denom: mars_vt_denom.clone() }, 
                max_premium: Uint128::new(4), 
                bid_threshold: Uint256::from(1_000_000_000_000u128), 
            };
            let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().liq_queue.to_string(), 
                msg: to_binary(&msg)?, 
                funds: vec![], 
            });
            msgs.push(msg);

            //Update LTV Disco queue for Mars VT to set max_LTV to 96% and percent_to_disperse to 5%
            let msg = LTVDiscoExecuteMsg::UpdateQueue {
                asset: mars_vt_denom.clone(),
                max_ltv: Some(Decimal::percent(96)),
                percent_to_disperse: Some(Decimal::percent(5)),
            };
            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.clone().ltv_disco.to_string(),
                msg: to_binary(&msg)?,
                funds: vec![],
            });
            msgs.push(msg);
                       
            //Instantiate Debt Auction
            let da_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),   
                code_id: config.clone().mbrn_auction_id, 
                msg: to_binary(&DAInstantiateMsg {
                    owner: None,
                    oracle_contract: addrs.clone().oracle.to_string(),
                    osmosis_proxy: addrs.clone().neutron_proxy.to_string(), 
                    positions_contract: addrs.clone().positions.to_string(),
                    governance_contract: addrs.clone().staking.to_string(), //staking contract as placeholder (governance removed)
                    staking_contract: addrs.clone().staking.to_string(),
                    twap_timeframe: 60u64,
                    mbrn_denom: config.clone().mbrn_denom.clone(),
                    initial_discount: Decimal::percent(1),
                    discount_increase_timeframe: 36, //Discount will hit 100% in 1 hour, 1.6667% per minute
                    discount_increase: Decimal::percent(1),
                    // New fields for fee auction routing
                    delay_window_minutes: Some(60u64), // 60 minute delay for CDT, 0 for MBRN
                    revenue_distributor_contract: Some(addrs.clone().revenue_distributor.to_string()),
                    ltv_disco_contract: Some(addrs.clone().ltv_disco.to_string()),
                })?, 
                funds: vec![], 
                label: String::from("auction"), 
            });
            let sub_msg = SubMsg::reply_on_success(da_instantiation, DEBT_AUCTION_REPLY_ID);     
            
            //Instantiate Emissions Voting
            let emissions_voting_instantiation = CosmosMsg::Wasm(WasmMsg::Instantiate { 
                admin: Some(config.owner.to_string()),  
                code_id: config.clone().emissions_voting_id, 
                msg: to_binary(&EmissionsVoting_InstantiateMsg {
                    owner: Some(config.owner.to_string()),
                    ltv_disco_contract: addrs.clone().ltv_disco.to_string(),
                    staking_contract: addrs.clone().staking.to_string(),
                })?, 
                funds: vec![], 
                label: String::from("emissions_voting"), 
            });
            let emissions_voting_submsg = SubMsg::reply_on_success(emissions_voting_instantiation, EMISSIONS_VOTING_REPLY_ID);
            
            Ok(Response::new()
                .add_messages(msgs)
                .add_submessage(sub_msg)
                .add_submessage(emissions_voting_submsg)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

/// Add Owners & contracts to the Osmosis Proxy.
/// Add contracts to contract configurations & change owners to Governance.
/// Query saved share tokens in Position's contract & add Supply Caps for them.
/// Instantiate Margin Proxy.
pub fn handle_auction_reply(deps: DepsMut, _env: Env, msg: Reply)-> StdResult<Response>{
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
                        .any(|attr| attr.key == "_contract_address")
                })
                .ok_or_else(|| {
                    StdError::generic_err(format!("unable to find instantiate event"))
                })?;

            let contract_address = &instantiate_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .unwrap()
                .value;

            let valid_address = deps.api.addr_validate(&contract_address)?;

            //Save MBRN Auction address
            let mut addrs = ADDRESSES.load(deps.storage)?;
            addrs.mbrn_auction = valid_address.clone();
            ADDRESSES.save(deps.storage, &addrs)?;
            
            // Query Mars VT contract to get vault token denom
            let mars_vt_config: membrane::mars_vault_token::Config = deps.querier.query_wasm_smart(
                addrs.clone().mars_vault_token.to_string(),
                &MarsVT_QueryMsg::Config {},
            )?;
            let mars_vt_vault_token = mars_vt_config.vault_token;
            
            let mut msgs = vec![];

            //Add owners to Neutron Proxy
            let msg = ProxyExecuteMsg::UpdateConfig { 
                owners: Some(vec![
                    NeutronOwnerEntry {
                        owner: NeutronOwner {
                        owner: addrs.clone().positions, 
                        non_token_contract_auth: false,
                        },
                        remove: false,
                    },
                    NeutronOwnerEntry {
                        owner: NeutronOwner {
                        owner: addrs.clone().vesting, 
                        non_token_contract_auth: false,
                    },
                        remove: false,
                    },
                    NeutronOwnerEntry {
                        owner: NeutronOwner {
                        owner: addrs.clone().staking, 
                        non_token_contract_auth: false,
                        },
                        remove: false,
                    },
                    NeutronOwnerEntry {
                        owner: NeutronOwner {
                        owner: addrs.clone().liq_queue,  //For repayment burns
                        non_token_contract_auth: false,
                        },
                        remove: false,
                    },
                    NeutronOwnerEntry {
                        owner: NeutronOwner {
                        owner: addrs.clone().mbrn_auction, 
                        non_token_contract_auth: false,
                        },
                        remove: false,
                    }
                ]), 
                transmutation_pairs: None,
                debt_auction: Some(addrs.clone().mbrn_auction.to_string()), 
                transmuter_contract: Some(addrs.clone().transmuter.to_string()), 
                vaults: Some(vec![
                    VaultEntry {
                        vault_info: VaultInfo {
                            vault_addr: addrs.clone().mars_vault_token,
                            vault_token: mars_vt_vault_token,
                            deposit_token:  String::from(NEUTRON_USDC_DENOM),  
                        },
                        remove: false,
                    }
                ]),
                astroport_factory: None, 
                astroport_router: None, 
                enable_dynamic_routing: None,
                transmute_supply_thresholds: None,
                vesting_contract: None,
                vesting_period: None,
            };
            let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: addrs.clone().neutron_proxy.to_string(), 
                msg: to_binary(&msg)?, 
                funds: vec![], 
            });
            msgs.push(msg);
            
            ////Add contracts to contract configurations & change owners to Governance
            //Staking
            msgs.push(
                CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: addrs.clone().staking.to_string(), 
                    msg: to_binary(&StakingExecuteMsg::UpdateConfig { 
                        owner: Some(config.owner.to_string()),  
                        positions_contract: Some(addrs.clone().positions.to_string()),
                        auction_contract: Some(addrs.clone().mbrn_auction.to_string()),
                        osmosis_proxy: Some(addrs.clone().neutron_proxy.to_string()), //param - using neutron_proxy as osmosis_proxy
                        emissions_voting_contract: None,
                        vesting_contract: Some(addrs.clone().vesting.to_string()),
                        governance_contract: None, 
                        mbrn_denom: None,
                        incentive_schedule: None,
                        unstaking_period: None,
                        max_commission_rate: None,
                        keep_raw_cdt: None,
                        vesting_rev_multiplier: None,
                        buyback_and_burn: None,
                    })?, 
                    funds: vec![],
                }));
            
            /////Query saved share tokens in Position's contract & add Supply Caps for them
            let basket: Basket = deps.querier.query_wasm_smart(
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
                    stability_pool_ratio_for_debt_cap: Some(Decimal::percent(33)),
                })
                .collect::<Vec<SupplyCap>>();
            
            let msg = CDPExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                liq_queue: None,
                collateral_supply_caps: Some(lp_supply_caps),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: Some(false),
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
                credit_pool_infos: Some(vec![]),
                take_revenue: None,
            });
            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: addrs.clone().positions.to_string(),
                msg: to_binary(&msg)?,
                funds: vec![],
            });
            msgs.push(msg);

            
            // Neutron oracle UpdateConfig only supports owner and positions_contract
            // TWAP pools and other config are set via AddAsset/EditAsset messages
            // This section removed - configure oracle assets via AddAsset instead

            Ok(Response::new()
                .add_messages(msgs)
            )
        },
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }    
}

// Removed: handle_balancer_reply - balancer pool functionality removed
// pub fn handle_balancer_reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response>{
//     match msg.clone().result.into_result() {
//         Ok(result) => {
//         let mut osmo_pool_id = OSMO_POOL_ID.load(deps.storage)?;
//         let addrs = ADDRESSES.load(deps.storage)?;
//         let config = CONFIG.load(deps.storage)?;

//         let mut sub_msgs: Vec<SubMsg> = vec![];
//         let mut msgs: Vec<CosmosMsg> = vec![];
        
//         //Get Balancer Pool denom from Response
//         if let Some(b) = result.data {
//             let res: MsgCreateBalancerPoolResponse = match b.try_into().map_err(ContractError::Std){
//                 Ok(res) => res,
//                 Err(err) => return Err(StdError::GenericErr { msg: String::from(err.to_string()) })
//             };

//             //Save Pool ID if unsaved
//             //this is the first replying message so skip the rest of the logic
//             if let Err(_) = MBRN_POOL.load(deps.storage){
//                 MBRN_POOL.save(deps.storage, &res.pool_id)?;

//                 return Ok(Response::new()
//                     .add_attribute("pool_saved", res.pool_id.to_string())
//                 )
//             }
            
//             //Save Pool ID
//             //OSMO pool replies 2nd
//             if osmo_pool_id == 0 {
//                 osmo_pool_id = res.pool_id;

//                 //Mint MBRN for Incentives
//                 let op_msg = ProxyExecuteMsg::MintTokens { 
//                     denom: config.clone().mbrn_denom, 
//                     amount: Uint128::new(2_000_000_000_000), 
//                     mint_to_address: env.clone().contract.address.to_string(),
//                 };
//                 let op_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
//                     contract_addr: addrs.clone().neutron_proxy.to_string(), 
//                     msg: to_binary(&op_msg)?, 
//                     funds: vec![], 
//                 });
//                 sub_msgs.push(SubMsg::reply_on_error(op_msg, NO_ACTION_ID));
                
//                 //Get Balancer denom from Response
//                 let pool_denom = deps.querier.query::<PoolStateResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
//                     contract_addr: addrs.clone().neutron_proxy.to_string(), 
//                     msg: to_binary(&ProxyQueryMsg::PoolState {
//                         id: res.pool_id,
//                     })?,
//                 }))?.shares.denom;
                
//                 //Set the CDT/OSMO LP denom oracle
//                 msgs.push(
//                     CosmosMsg::Wasm(WasmMsg::Execute { 
//                         contract_addr: addrs.clone().oracle.to_string(), 
//                         msg: to_binary(&OracleExecuteMsg::AddAsset { 
//                             asset_info: AssetInfo::NativeToken { denom: pool_denom.clone() }, 
//                             oracle_info: AssetOracleInfo { 
//                                 basket_id: Uint128::one(), 
//                                 pools_for_osmo_twap: vec![],
//                                 is_usd_par: false,
//                                 lp_pool_info: Some(
//                                     PoolInfo { 
//                                         pool_id: osmo_pool_id,
//                                         asset_infos: vec![
//                                             LPAssetInfo { 
//                                                 info: AssetInfo::NativeToken { denom: config.clone().osmo_denom }, 
//                                                 decimals: 6, 
//                                                 ratio: Decimal::percent(50),
//                                             },
//                                             LPAssetInfo { 
//                                                 info: AssetInfo::NativeToken { denom: config.clone().credit_denom }, 
//                                                 decimals: 6, 
//                                                 ratio: Decimal::percent(50),
//                                             },
//                                         ],
//                                     }
//                                 ),
//                                 decimals: 18,
//                                 pyth_price_feed_id: None,
//                                 vault_info: None,
//                             },
//                         })?, 
//                         funds: vec![],
//                     }));
//                 //Set CDT oracle
//                 msgs.push(
//                     CosmosMsg::Wasm(WasmMsg::Execute { 
//                         contract_addr: addrs.clone().oracle.to_string(), 
//                         msg: to_binary(&OracleExecuteMsg::AddAsset { 
//                             asset_info: AssetInfo::NativeToken { denom: config.clone().credit_denom }, 
//                             oracle_info: AssetOracleInfo { 
//                                 basket_id: Uint128::one(), 
//                                 pools_for_osmo_twap: vec![
//                                     TWAPPoolInfo { 
//                                         pool_id: osmo_pool_id,
//                                         base_asset_denom: config.clone().credit_denom.to_string(),  
//                                         quote_asset_denom: config.clone().osmo_denom.to_string(), 
//                                     }
//                                 ],
//                                 is_usd_par: false,
//                                 lp_pool_info: None,
//                                 decimals: 6,
//                                 pyth_price_feed_id: None,
//                                 vault_info: None,
//                             },
//                         })?, 
//                         funds: vec![],
//                     }));
//                 //Set Auction's desired asset to the CDT/OSMO LP denom
//                 //and switch owner to governance
//                 msgs.push(
//                     CosmosMsg::Wasm(WasmMsg::Execute { 
//                         contract_addr: addrs.clone().mbrn_auction.to_string(), 
//                         msg: to_binary(&DAExecuteMsg::UpdateConfig(AuctionUpdateConfig {
//                             owner: Some(addrs.clone().governance.to_string()),
//                             oracle_contract: None,
//                             neutron_proxy: None,
//                             mbrn_denom: None,
//                             cdt_denom: None,
//                             desired_asset: Some(pool_denom.clone()),
//                             positions_contract: None,
//                             governance_contract: None,
//                             staking_contract: None,
//                             twap_timeframe: None,
//                             initial_discount: None,
//                             discount_increase_timeframe: None,
//                             discount_increase: None,
//                             send_to_stakers: None,
//                         }))?, 
//                         funds: vec![],
//                     })
//                 );

//                 //Incentivize the OSMO/CDT pool
//                 //14 day guage
//                 let msg: CosmosMsg = MsgCreateGauge { 
//                     pool_id:  0,
//                     is_perpetual: false, 
//                     owner: env.clone().contract.address.to_string(),
//                     distribute_to: Some(QueryCondition { 
//                         lock_query_type: 0, //ByDuration
//                         denom: pool_denom,
//                         duration: Some(Duration { seconds: 14 * SECONDS_PER_DAY as i64, nanos: 0 }), 
//                         timestamp: None,
//                     }), 
//                     coins: vec![Coin {
//                         denom: config.clone().mbrn_denom, 
//                         amount: String::from("2_000_000_000_000"),
//                     }], 
//                     start_time: Some(
//                         Timestamp { 
//                             seconds: env.clone().block.time.seconds()as i64,
//                             nanos: 0 
//                         }
//                     ), 
//                     num_epochs_paid_over: 365, //days, 1 year
//                 }.into();
//                 sub_msgs.push(SubMsg::reply_on_error(msg, NO_ACTION_ID));                
//             } 
//             OSMO_POOL_ID.save(deps.storage, &osmo_pool_id)?;

//             //Set credit_pool_infos
//             //Add Credit LPs to Basket
//             let msg = CDPExecuteMsg::EditBasket(EditBasket {
//                 added_cAsset: None,
//                 liq_queue: None,
//                 credit_pool_infos: Some(vec![
//                     membrane::types::PoolType::Balancer { pool_id: osmo_pool_id }
//                     ]),
//                 collateral_supply_caps: None,
//                 multi_asset_supply_caps: None,
//                 base_interest_rate: None,
//                 credit_asset_twap_price_source: Some(TWAPPoolInfo {
//                     pool_id: osmo_pool_id,
//                     base_asset_denom: config.clone().credit_denom,
//                     quote_asset_denom: config.clone().osmo_denom,
//                 }),
//                 negative_rates: None,
//                 cpc_margin_of_error: None,
//                 frozen: None,
//                 distribute_revenue: None,
//                 take_revenue: None,
//             });
//             let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
//                 contract_addr: addrs.clone().positions.to_string(), 
//                 msg: to_binary(&msg)?, 
//                 funds: vec![], 
//             });
//             msgs.push(msg);
//             // Add OSMO Pool as accepted LP for the Discount Vault
//             let msg = DiscountVaultExecuteMsg::EditAcceptedLPs { 
//                 pool_ids: vec![osmo_pool_id], 
//                 remove: false 
//             };
//             let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
//                 contract_addr: addrs.clone().discount_vault.to_string(), 
//                 msg: to_binary(&msg)?, 
//                 funds: vec![], 
//             });
//             msgs.push(msg);
//             // Set DV Owner to governance
//             let msg = DiscountVaultExecuteMsg::ChangeOwner { owner: addrs.clone().governance.to_string() };
//             let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
//                 contract_addr: addrs.clone().discount_vault.to_string(), 
//                 msg: to_binary(&msg)?, 
//                 funds: vec![], 
//             });
//             msgs.push(msg);

//             // Add MBRN-OSMO LP to oracle for MBRN pricing
//             // msgs.push(
//             //     CosmosMsg::Wasm(WasmMsg::Execute { 
//             //         contract_addr: addrs.clone().oracle.to_string(), 
//             //         msg: to_binary(&OracleExecuteMsg::AddAsset { 
//             //             asset_info: AssetInfo::NativeToken { denom: config.clone().mbrn_denom }, 
//             //             oracle_info: AssetOracleInfo { 
//             //                 basket_id: Uint128::one(), 
//             //                 pools_for_osmo_twap: vec![
//             //                     TWAPPoolInfo { 
//             //                         pool_id: MBRN_POOL.load(deps.storage)?,
//             //                         base_asset_denom: config.clone().mbrn_denom.to_string(),  
//             //                         quote_asset_denom: config.clone().osmo_denom.to_string(), 
//             //                     }
//             //                 ],
//             //                 is_usd_par: false,
//             //                 lp_pool_info: None,
//             //                 decimals: 6,       
//             //                 pyth_price_feed_id: None,                     
//             //             },
//             //         })?, 
//             //         funds: vec![],
//             //     }));
//             // Set oracle ownership to governance & add USD Par TWAP pool
//             msgs.push(
//                 CosmosMsg::Wasm(WasmMsg::Execute { 
//                     contract_addr: addrs.clone().oracle.to_string(), 
//                     msg: to_binary(&OracleExecuteMsg::UpdateConfig { 
//                         owner: Some(addrs.clone().governance.to_string()), 
//                         positions_contract: None,
//                         neutron_proxy_contract: Some(addrs.clone().neutron_proxy.to_string()),
//                         pyth_osmosis_address: None,
//                         osmo_usd_pyth_feed_id: None,
//                         pools_for_usd_par_twap: Some(vec![
//                             TWAPPoolInfo { 
//                                 pool_id: config.clone().osmousdc_pool_id, 
//                                 base_asset_denom: config.clone().osmo_denom.to_string(), 
//                                 quote_asset_denom: config.clone().usdc_denom.to_string(),  
//                             }
//                         ])
//                     })?, 
//                     funds: vec![],
//                 }));

//             //Add CDT/OSMO LP to Liquidity Check
//             // msgs.push(
//             //     CosmosMsg::Wasm(WasmMsg::Execute { 
//             //         contract_addr: addrs.clone().liquidity_check.to_string(), 
//             //         msg: to_binary(&LCExecuteMsg::AddAsset { asset: LiquidityInfo {
//             //             asset: AssetInfo::NativeToken {
//             //                 denom: config.clone().credit_denom,
//             //             },
//             //             pool_infos: vec![PoolType::Balancer { pool_id: osmo_pool_id }]
//             //         } })?, 
//             //         funds: vec![],
//             //     }));
//             // //Change LC ownership to governance 
//             // msgs.push(
//             //     CosmosMsg::Wasm(WasmMsg::Execute { 
//             //         contract_addr: addrs.clone().liquidity_check.to_string(), 
//             //         msg: to_binary(&LCExecuteMsg::UpdateConfig { 
//             //             owner: Some(addrs.clone().governance.to_string()),
//             //             neutron_proxy: None, 
//             //             positions_contract: None, 
//             //             stableswap_multiplier: None
//             //         })?, 
//             //         funds: vec![],
//             //     }));
//             // Change Positions contract ownership to governance & add misc. contracts
//             msgs.push(
//                 CosmosMsg::Wasm(WasmMsg::Execute { 
//                     contract_addr: addrs.clone().positions.to_string(), 
//                     msg: to_binary(&CDPExecuteMsg::UpdateConfig(CDPUpdateConfig {
//                         owner: Some(addrs.clone().governance.to_string()), 
//                         stability_pool: Some(addrs.clone().stability_pool.to_string()), 
//                         dex_router: None,
//                         neutron_proxy: None,
//                         debt_auction: Some(addrs.clone().mbrn_auction.to_string()), 
//                         staking_contract: None,
//                         oracle_contract: None,
//                         liquidity_contract: Some(addrs.clone().liquidity_check.to_string()), 
//                         discounts_contract: Some(addrs.clone().system_discounts.to_string()), 
//                         liq_fee: None,
//                         debt_minimum: None,
//                         base_debt_cap_multiplier: None,
//                         oracle_time_limit: None,
//                         credit_twap_timeframe: None,
//                         collateral_twap_timeframe: None,
//                         cpc_multiplier: None,
//                         rate_slope_multiplier: None,
//                         rate_hike_rate: None,
//                         redemption_fee: None,
//                     }))?, 
//                     funds: vec![],
//                 }));

//             //Query contract balance of any GAMM shares 
//             //but we only care about MBRN/CDT-OSMO LP
//             let coins: Vec<cosmwasm_std::Coin> = deps.querier.query_all_balances(env.contract.address.to_string())?;
//             let gamm_coins = coins
//                 .into_iter()
//                 .filter( |coin| coin.denom.contains("gamm"))
//                 .collect::<Vec<cosmwasm_std::Coin>>();
                
                
//             //Send gamm_coins to Governance
//             let msg = BankMsg::Send {
//                 to_address: addrs.clone().governance.to_string(),
//                 amount: gamm_coins,
//             };
//             msgs.push(msg.into());
//         }

//         Ok(Response::new()
//             //If incentive msgs error I don't want it to halt the launch since we are using a duration that is untestable on testnet
//             .add_submessages(sub_msgs)
//             .add_messages(msgs)
//             .add_attribute("pool_saved", format!("{:?}", osmo_pool_id))
//             .add_attribute("mbrn_pool_saved", format!("{:?}", MBRN_POOL.load(deps.storage).unwrap_or(0)))
//         )
//     },
//         Err(err) => return Err(StdError::GenericErr { msg: err }),
//     }    
// }
