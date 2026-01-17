use std::cmp::min;

use cosmwasm_std::{
    entry_point, to_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, WasmMsg,
    Response, StdResult, Uint128, Reply, StdError, CosmosMsg, SubMsg, Addr, coin, attr, Storage, Empty, WasmQuery, QueryRequest,
};
use cw2::set_contract_version;

use membrane::helpers::{withdrawal_msg, get_contract_balances};
use membrane::neutron_launch::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig, MigrateMsg};
use membrane::math::{decimal_division, decimal_multiplication};
use membrane::staking::ExecuteMsg as StakingExecuteMsg;
use membrane::neutron_proxy::ExecuteMsg as Proxy_ExecuteMsg;
use membrane::types::{AssetInfo, Asset, UserRatio, Lockdrop, LockedUser, Lock, Owner, VestingPeriod};

use membrane::vesting::{QueryMsg as VestingQueryMsg, ExecuteMsg as VestingExecuteMsg, RecipientsResponse};
use membrane::cdp::{ExecuteMsg as CDPExecuteMsg, UpdateConfig as CDPUpdateConfig};
use membrane::oracle::ExecuteMsg as OracleExecuteMsg;
use membrane::liq_queue::ExecuteMsg as LQExecuteMsg;
use membrane::liquidity_check::ExecuteMsg as LCExecuteMsg;
use membrane::auction::{ExecuteMsg as DAExecuteMsg, UpdateConfig as AuctionUpdateConfig};
use membrane::discount_vault::ExecuteMsg as DiscountVaultExecuteMsg;

use osmosis_std::types::cosmos::base::v1beta1::Coin;
use osmosis_std::types::osmosis::gamm::poolmodels::balancer::v1beta1::MsgCreateBalancerPool;
use osmosis_std::types::osmosis::gamm::v1beta1::PoolParams;
use osmosis_std::types::osmosis::gamm::v1beta1::PoolAsset;


use crate::error::ContractError;
use crate::state::{CONFIG, ADDRESSES, LaunchAddrs};
use crate::reply::{handle_auction_reply, handle_cdp_reply, handle_create_denom_reply, handle_lq_reply, handle_np_reply, handle_oracle_reply, handle_staking_reply, handle_discount_vault_reply, handle_system_discounts_reply, handle_ltv_disco_reply, handle_transmuter_reply, handle_revenue_distributor_reply, handle_transmuter_lockdrop_reply, handle_yield_arb_reply, handle_mars_vt_reply, handle_points_system_reply, handle_emissions_voting_reply};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "neutron_launch";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Reply ID
pub const NEUTRON_PROXY_REPLY_ID: u64 = 1;
pub const ORACLE_REPLY_ID: u64 = 2;
pub const STAKING_REPLY_ID: u64 = 3;
pub const VESTING_REPLY_ID: u64 = 4;
pub const POSITIONS_REPLY_ID: u64 = 6;
pub const LTV_DISCO_REPLY_ID: u64 = 17;
pub const TRANSMUTER_REPLY_ID: u64 = 18;
pub const REVENUE_DISTRIBUTOR_REPLY_ID: u64 = 19;
pub const TRANSMUTER_LOCKDROP_REPLY_ID: u64 = 20;
pub const YIELD_ARB_REPLY_ID: u64 = 21;
pub const MARS_VT_REPLY_ID: u64 = 22;
pub const POINTS_SYSTEM_REPLY_ID: u64 = 23;
pub const EMISSIONS_VOTING_REPLY_ID: u64 = 24;
pub const LIQ_QUEUE_REPLY_ID: u64 = 8;
pub const DEBT_AUCTION_REPLY_ID: u64 = 10;
pub const CREATE_DENOM_REPLY_ID: u64 = 12;
pub const SYSTEM_DISCOUNTS_REPLY_ID: u64 = 13;
pub const DISCOUNT_VAULT_REPLY_ID: u64 = 14;
pub const NO_ACTION_ID: u64 = 16;

//Constants
pub const SECONDS_PER_DAY: u64 = 86_400u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {

    //Need 20 OSMO for CreateDenom Msgs
    // if deps.querier.query_balance(env.clone().contract.address, "uosmo")?.amount < Uint128::new(20_000_000){ return Err(ContractError::NeedOsmo {}) }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config = Config {
        owner: info.sender.clone(),
        mbrn_denom: String::from(""),
        credit_denom: String::from(""),
        pre_launch_contributors: Addr::unchecked(""), //param - set if needed
        pre_launch_community: vec![], //param - set if needed
        apollo_router: Addr::unchecked(""), //param - set if needed
        mbrn_launch_amount: Uint128::zero(), //param - set if needed
        atom_denom: String::from(""), //param - set if needed
        osmo_denom: String::from(""), //param - set if needed
        usdc_denom: String::from(""), //param - set if needed
        atomosmo_pool_id: 0u64, //param - set if needed
        osmousdc_pool_id: 0u64, //param - set if needed
        neutron_proxy_id: msg.neutron_proxy_id,
        neutron_oracle_id: msg.neutron_oracle_id,
        staking_id: msg.staking_id,
        vesting_id: msg.vesting_id,
        positions_id: msg.positions_id,
        liq_queue_id: msg.liq_queue_id,
        mbrn_auction_id: msg.mbrn_auction_id,
        system_discounts_id: msg.system_discounts_id,
        discount_vault_id: msg.discount_vault_id,
        ltv_disco_id: msg.ltv_disco_id,
        transmuter_id: msg.transmuter_id,
        revenue_distributor_id: msg.revenue_distributor_id,
        transmuter_lockdrop_id: msg.transmuter_lockdrop_id,
        yield_arb_id: msg.yield_arb_id,
        mars_vault_token_id: msg.mars_vault_token_id,
        points_system_id: msg.points_system_id,
        emissions_voting_id: msg.emissions_voting_id,
        // atom_denom: String::from("ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"), //testnet: ibc/A8C2D23A1E6F95DA4E48BA349667E322BD7A6C996D8A4AAE8BA72E190F3D1477
        // osmo_denom: String::from("uosmo"),
        // usdc_denom: String::from("ibc/D189335C6E4A68B513C10AB227BF1C1D38C746766278BA3EEB4FB14124F1D858"),  //axl wrapped usdc //testnet: 6F34E1BD664C36CE49ACC28E60D62559A5F96C4F9A6CCE4FC5A67B2852E24CFE
        // atomosmo_pool_id: 1, //testnet is 12
        // osmousdc_pool_id: 678, //axl wrapped usdc, testnet is 5
    };
    CONFIG.save(deps.storage, &config)?;

    ADDRESSES.save(deps.storage, &LaunchAddrs {
        neutron_proxy: Addr::unchecked(""),
        oracle: Addr::unchecked(""),
        staking: Addr::unchecked(""),
        vesting: Addr::unchecked(""),
        positions: Addr::unchecked(""),
        liq_queue: Addr::unchecked(""),
        mbrn_auction: Addr::unchecked(""),
        discount_vault: Addr::unchecked(""),
        system_discounts: Addr::unchecked(""),
        ltv_disco: Addr::unchecked(""),
        transmuter: Addr::unchecked(""),
        revenue_distributor: Addr::unchecked(""),
        transmuter_lockdrop: Addr::unchecked(""),
        yield_arb: Addr::unchecked(""),
        mars_vault_token: Addr::unchecked(""),
        points_system: Addr::unchecked(""),
        emissions_voting: Addr::unchecked(""),
    })?;

    let msg = CosmosMsg::Wasm(WasmMsg::Instantiate { 
        admin: Some(info.sender.to_string()),
        code_id: config.clone().neutron_proxy_id,
        msg: to_binary(&Empty {})?,
        funds: vec![],
        label: String::from("neutron_proxy") 
    });
    let sub_msg = SubMsg::reply_on_success(msg, NEUTRON_PROXY_REPLY_ID);

    Ok(Response::new()
        .add_submessage(sub_msg)
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
    )
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig(update) => update_config(deps, info, update),
    }
}

/// Update contract configuration
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert authority
    if info.sender != config.clone().pre_launch_contributors {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(credit_denom) = update.credit_denom {
        config.credit_denom = credit_denom;
    }
    if let Some(mbrn_denom) = update.mbrn_denom {
        config.mbrn_denom = mbrn_denom;
    }
    if let Some(osmo_denom) = update.osmo_denom {
        config.osmo_denom = osmo_denom;
    }
    if let Some(usdc_denom) = update.usdc_denom {
        config.usdc_denom = usdc_denom;
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("new_config", format!("{:?}", config)))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::ContractAddresses {} => to_binary(&ADDRESSES.load(deps.storage)?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        NEUTRON_PROXY_REPLY_ID => handle_np_reply(deps, env, msg),
        ORACLE_REPLY_ID => handle_oracle_reply(deps, env, msg),
        STAKING_REPLY_ID => handle_staking_reply(deps, env, msg),
        POSITIONS_REPLY_ID => handle_cdp_reply(deps, env, msg),
        LTV_DISCO_REPLY_ID => handle_ltv_disco_reply(deps, env, msg),
        TRANSMUTER_REPLY_ID => handle_transmuter_reply(deps, env, msg),
        REVENUE_DISTRIBUTOR_REPLY_ID => handle_revenue_distributor_reply(deps, env, msg),
        TRANSMUTER_LOCKDROP_REPLY_ID => handle_transmuter_lockdrop_reply(deps, env, msg),
        YIELD_ARB_REPLY_ID => handle_yield_arb_reply(deps, env, msg),
        MARS_VT_REPLY_ID => handle_mars_vt_reply(deps, env, msg),
        POINTS_SYSTEM_REPLY_ID => handle_points_system_reply(deps, env, msg),
        EMISSIONS_VOTING_REPLY_ID => handle_emissions_voting_reply(deps, env, msg),
        LIQ_QUEUE_REPLY_ID => handle_lq_reply(deps, env, msg),
        DEBT_AUCTION_REPLY_ID => handle_auction_reply(deps, env, msg),
        CREATE_DENOM_REPLY_ID => handle_create_denom_reply(deps, env, msg),
        SYSTEM_DISCOUNTS_REPLY_ID => handle_system_discounts_reply(deps, env, msg),
        DISCOUNT_VAULT_REPLY_ID => handle_discount_vault_reply(deps, env, msg),
        NO_ACTION_ID => Ok(Response::new()),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    
    //Mint incorrectly burnt MBRN to staking contract
    // let message = CosmosMsg::Wasm(WasmMsg::Execute {
    //     contract_addr: "osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd".to_string(),
    //     msg: to_binary(&OPExecuteMsg::MintTokens {
    //         denom: String::from("factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/umbrn"),
    //         amount: Uint128::new(7382206489624),
    //         mint_to_address: "osmo1fty83rfxqs86jm5fmlql5e340e8pe0v9j8ez0lcc6zwt2amegwvsfp3gxj".to_string(),
    //     })?,
    //     funds: vec![],
    // });

    Ok(Response::new()
    // .add_message(message)
)
}