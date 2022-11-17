use cosmwasm_std::{
    attr, entry_point, to_binary, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    QueryRequest, Response, StdResult, Uint128, WasmQuery,
};
use cw2::set_contract_version;

use membrane::discounts::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::stability_pool::{QueryMsg as SP_QueryMsg, DepositResponse};
use membrane::staking::{QueryMsg as Staking_QueryMsg, Config as Staking_Config, StakerResponse, RewardsResponse};
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::types::{AssetInfo, LiquidityInfo};

use osmo_bindings::PoolStateResponse;

use cw_storage_plus::Bound;

use crate::error::ContractError;
use crate::state::{ASSETS, CONFIG};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "discounts";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const MAX_LIMIT: u64 = 31u64;

pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config: Config;
    let owner = if let Some(owner) = msg.owner {
        deps.api.addr_validate(&owner)?
    } else {
        info.sender()
    };

    ///Query mbrn_denom
    let staking_contract = deps.api.addr_validate(&msg.staking_contract)?;

    let mbrn_denom = deps.querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::Config {})?,
    }))?
    .mbrn_denom;

    config = Config {
        owner,
        mbrn_denom,
        positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
        oracle_contract: deps.api.addr_validate(&msg.oracle_contract)?,
        staking_contract,
        stability_pool_contract: deps.api.addr_validate(&msg.stability_pool_contract)?,
        gauge_vault_contract: deps.api.addr_validate(&msg.gauge_vault_contract)?,
        discount_vault_contract: deps.api.addr_validate(&msg.discount_vault_contract)?,
    };
    

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig {
            owner,
            positions_contract,
            oracle_contract,
            staking_contract,
            stability_pool_contract,
            gauge_vault_contract,
            discount_vault_contract,
        } => update_config(
            deps, info, owner, 
            positions_contract,
            oracle_contract,
            staking_contract,
            stability_pool_contract,
            gauge_vault_contract,
            discount_vault_contract,
        ),
    }
}

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    oracle_contract: Option<String>,
    positions_contract: Option<String>,
    staking_contract: Option<String>,
    stability_pool_contract: Option<String>,
    gauge_vault_contract: Option<String>,
    discount_vault_contract: Option<String>,
) -> Result<Response, ContractError> {

    let mut config = CONFIG.load(deps.storage)?;

    //Assert authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    //Save optionals
    if let Some(addr) = owner {
        config.owner = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = positions_contract {
        config.positions_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = oracle_contract {
        config.oracle_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = staking_contract {
        config.staking_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = stability_pool_contract {
        config.stability_pool_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = gauge_vault_contract {
        config.gauge_vault_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = discount_vault_contract {
        config.discount_vault_contract = deps.api.addr_validate(&addr)?;
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::UserDiscount { user } => to_binary(&get_discount(deps, user)?),
    }
}

//Calc value of staked MBRN & pending rewards
fn get_staked_MBRN_value(
    deps: DepsMut,
    config: Config,
    user: String,
) -> StdResult<Decimal>{

    let mut user_stake = deps.querier.query::<StakerResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::UserStake {
            staker: user,
        })?,
    }))?
    .total_staked;

    let rewards = deps.querier.query::<RewardsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::StakerRewards {
            staker: user,
        })?,
    }))?;

    //Add accrued interest to user_stake
    user_stake += rewards.accrued_interest;

    //Calculate staked value from reward.claimables
    let mut staked_value = Decimal::zero();
    
    for asset in rewards.claimables {

        let price = deps.querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().oracle.to_string(),
            msg: to_binary(&Oracle_QueryMsg::Price {
                asset_info: asset.info,
                twap_timeframe: 60,
                basket_id: None,
            })?,
        }))?
        .price;

        let value = decimal_multiplication(price, Decimal::from_ratio(asset.amount, Uint128::one()));

        staked_value += value;
    }

    //Add MBRN value to staked_value
    let price = deps.querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().oracle.to_string(),
        msg: to_binary(&Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken { denom: config.clone().mbrn_denom },
            twap_timeframe: 60,
            basket_id: None,
        })?,
    }))?
    .price;

    let value = decimal_multiplication(price, Decimal::from_ratio(user_stake, Uint128::one()));

    staked_value += value;
    
    Ok( staked_value )
}

//Gets user total Stability Pool funds
fn get_sp_funds(
    deps: DepsMut,
    config: Config,
    user: String,
) -> StdResult<Decimal>{

    //Query Stability Pool to see if the user has funds
    let user_deposits = deps.querier.query::<DepositResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().stability_pool_contract.to_string(),
        msg: to_binary(&SP_QueryMsg::AssetDeposits {
            user: position_owner.clone(),
            asset_info: basket.clone().credit_asset.info,
        })?,
    }))?
    .deposits;

    let total_user_deposit: Decimal = user_deposits
        .iter()
        .map(|user_deposit| user_deposit.amount)
        .collect::<Vec<Decimal>>()
        .into_iter()
        .sum();

    Ok( total_user_deposit )
}
