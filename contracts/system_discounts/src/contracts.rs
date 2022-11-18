use cosmwasm_std::{
    attr, entry_point, to_binary, Binary, Decimal, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    QueryRequest, Response, StdResult, Uint128, WasmQuery,
};
use cw2::set_contract_version;

use membrane::math::decimal_multiplication;
use membrane::system_discounts::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::stability_pool::{QueryMsg as SP_QueryMsg, DepositResponse};
use membrane::staking::{QueryMsg as Staking_QueryMsg, Config as Staking_Config, StakerResponse, RewardsResponse};
use membrane::incentive_gauge_vault::{QueryMsg as IG_QueryMsg, UserResponse};
use membrane::discount_vault::{QueryMsg as Discount_QueryMsg, UserResponse as Discount_UserResponse};
use membrane::positions::{QueryMsg as CDP_QueryMsg, BasketResponse};
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::types::{AssetInfo, DebtTokenAsset};


use cw_storage_plus::Bound;

use crate::error::ContractError;
use crate::state::{CONFIG};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "system_discounts";
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
        info.sender
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
        basket_id: msg.basket_id,
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

fn get_discount(
    deps: DepsMut,
    user: String, 
){
  
    //Load Config
    let config = CONFIG.load(deps.storage)?;

    //Get the value of the user's capital in..
    //Stake, SP & Queriable LPs
    let user_value_in_network = get_user_value_in_network()?;

    //Get User's outstanding debt
    let user_outstanding_debt = //Query Positions
}

//Get the value of the user's capital in..
//Stake, SP & Queriable LPs
fn get

//Returns total_value & credit AssetInfo
fn get_incentive_guage_value(
    deps: DepsMut,
    config: Config,
    user: String,    
) -> StdResult<(Decimal, AssetInfo)>{
    
    //Get user info from the Gauge Vault
    let user = deps.querier.query::<UserResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().gauge_vault_contract.to_string(),
        msg: to_binary(&IG_QueryMsg::User {
            user,
        })?,
    }))?;
    let debt_token: DebtTokenAsset = user.total_debt_token;
    let accrued_incentives = user.accrued_incentives;

    //Get credit_price
    let credit_price = deps.querier.query::<BasketResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().positions_contract.to_string(),
        msg: to_binary(&CDP_QueryMsg::GetBasket { basket_id: debt_token.basket_id })?,
    }))?
    .credit_price;

    //Calc total value of the LPs
    let mut total_value = 
    decimal_multiplication(
        decimal_multiplication(
            Decimal::from_ratio(debt_token.amount, Uint128::one()), 
            credit_price
        ), 
        Decimal::percent(200)
    );
    //Assumption is that all LPs are 50/50

    //Add value of MBRN incentives
    let price = deps.querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().oracle_contract.to_string(),
        msg: to_binary(&Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken { denom: config.clone().mbrn_denom },
            twap_timeframe: 60,
            basket_id: None,
        })?,
    }))?
    .price;

    let value = decimal_multiplication(
        price, 
        Decimal::from_ratio(accrued_incentives.amount, Uint128::one())
    );

    total_value += value;

    Ok((total_value, debt_token.info))

}

fn get_discounts_vault_value(
    deps: DepsMut,
    config: Config,
    user: String,
) -> StdResult<Decimal>{

     //Get user info from the Gauge Vault
     let user = deps.querier.query::<Discount_UserResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().discount_vault_contract.to_string(),
        msg: to_binary(&Discount_QueryMsg::User {
            user,
        })?,
    }))?;
    let total_value = user.premium_user_value;

    Ok(total_value)

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
            contract_addr: config.clone().oracle_contract.to_string(),
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
        contract_addr: config.clone().oracle_contract.to_string(),
        msg: to_binary(&Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken { denom: config.clone().mbrn_denom },
            twap_timeframe: 60,
            basket_id: None,
        })?,
    }))?
    .price;

    let value = decimal_multiplication(price, Decimal::from_ratio(user_stake, Uint128::one()));

    staked_value += value;
    
    Ok( 
        staked_value )
}

//Gets user total Stability Pool funds
fn get_sp_funds(
    deps: DepsMut,
    config: Config,
    user: String,
    asset_info: AssetInfo,
) -> StdResult<Decimal>{

    //Query Stability Pool to see if the user has funds
    let user_deposits = deps.querier.query::<DepositResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().stability_pool_contract.to_string(),
        msg: to_binary(&SP_QueryMsg::AssetDeposits {
            user,
            asset_info,
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
