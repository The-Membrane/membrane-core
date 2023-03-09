use cosmwasm_std::{
    entry_point, to_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo,
    QueryRequest, Response, StdResult, Uint128, WasmQuery, QuerierWrapper,
};
use cw2::set_contract_version;

use membrane::math::{decimal_multiplication, decimal_division};
use membrane::system_discounts::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig};
use membrane::stability_pool::{QueryMsg as SP_QueryMsg, ClaimsResponse};
use membrane::staking::{QueryMsg as Staking_QueryMsg, Config as Staking_Config, StakerResponse, RewardsResponse};
use membrane::discount_vault::{QueryMsg as Discount_QueryMsg, UserResponse as Discount_UserResponse};
use membrane::cdp::{QueryMsg as CDP_QueryMsg, PositionResponse};
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::types::{AssetInfo, Basket, Deposit, AssetPool};

use crate::error::ContractError;
use crate::state::CONFIG;

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "system_discounts";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_PER_DAY: u64 = 86_400u64;


pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let mut config: Config;
    let owner = if let Some(owner) = msg.owner {
        deps.api.addr_validate(&owner)?
    } else {
        info.sender
    };

    ///Query mbrn_denom
    let staking_contract = deps.api.addr_validate(&msg.staking_contract)?;

    let mbrn_denom = deps.querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: staking_contract.to_string(),
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
        lockdrop_contract: None,
        discount_vault_contract: None,
        minimum_time_in_network: msg.minimum_time_in_network,
    };
    //Store optionals
    if let Some(lockdrop_contract) = msg.lockdrop_contract{
        config.lockdrop_contract = Some(deps.api.addr_validate(&lockdrop_contract)?);
    }
    if let Some(discount_vault_contract) = msg.discount_vault_contract{
        config.discount_vault_contract = Some(deps.api.addr_validate(&discount_vault_contract)?);
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
    )
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
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
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    //Save optionals
    if let Some(addr) = update.owner {
        config.owner = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.positions_contract {
        config.positions_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.oracle_contract {
        config.oracle_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.staking_contract {
        config.staking_contract = deps.api.addr_validate(&addr)?;
        
        let mbrn_denom = deps.querier.query::<Staking_Config>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: addr.to_string(),
            msg: to_binary(&Staking_QueryMsg::Config {})?,
        }))?
        .mbrn_denom;

        config.mbrn_denom = mbrn_denom;
    }
    if let Some(addr) = update.stability_pool_contract {
        config.stability_pool_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.lockdrop_contract {
        config.lockdrop_contract = Some(deps.api.addr_validate(&addr)?);
    }
    if let Some(addr) = update.discount_vault_contract {
        config.discount_vault_contract = Some(deps.api.addr_validate(&addr)?);
    }
    if let Some(time) = update.minimum_time_in_network {
        config.minimum_time_in_network = time;
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("new_config", format!("{:?}", config)))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::UserDiscount { user } => to_binary(&get_discount(deps, env, user)?),
    }
}

/// Returns % of interest that is discounted,
/// i.e. 95% of 1% interest is .05% interest
fn get_discount(
    deps: Deps,
    env: Env,
    user: String, 
)-> StdResult<Decimal>{
    //Load Config
    let config = CONFIG.load(deps.storage)?;

    //Get the value of the user's capital in..
    //Stake, SP & Queriable LPs
    let user_value_in_network = get_user_value_in_network(deps.querier, env, config.clone(), user.clone())?;

    //Get User's outstanding debt
    let user_positions: Vec<PositionResponse> = deps.querier.query::<Vec<PositionResponse>>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().positions_contract.to_string(),
        msg: to_binary(&CDP_QueryMsg::GetUserPositions {
            user: user.clone(),
            limit: None,
        })?,
    }))?;

    let user_outstanding_debt: Uint128 = user_positions
        .into_iter()    
        .map(|position| position.credit_amount)
        .collect::<Vec<Uint128>>()
        .into_iter()
        .sum();
    let user_outstanding_debt = Decimal::from_ratio(user_outstanding_debt, Uint128::one());

    let percent_discount = {
        if user_value_in_network >= user_outstanding_debt {
            Decimal::one()
        } else {
            decimal_division(user_value_in_network, user_outstanding_debt)?
        }
    };

    Ok(percent_discount)
}

/// Get the value of the user's capital in
/// the Stability Pool, Discount Vault LPs & staking
fn get_user_value_in_network(
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    user: String,
)-> StdResult<Decimal>{

    let basket: Basket = querier.query::<Basket>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().positions_contract.to_string(),
        msg: to_binary(&CDP_QueryMsg::GetBasket { })?,
    }))?;
    let credit_price = basket.clone().credit_price;

    let mbrn_price = querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().oracle_contract.to_string(),
        msg: to_binary(&Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken { denom: config.clone().mbrn_denom },
            twap_timeframe: 60,
            basket_id: None,
        })?,
    }))?
    .price;

    //Initialize total_value
    let mut total_value = Decimal::zero();

    total_value += get_sp_value(querier, config.clone(), env.clone().block.time.seconds(), user.clone(), mbrn_price)?;
    total_value += get_staked_MBRN_value(querier, config.clone(), user.clone(), mbrn_price.clone(), credit_price.clone())?;

    if config.discount_vault_contract.is_some(){
        total_value += get_discounts_vault_value(querier, config.clone(), user.clone())?;
    }   
    
    Ok( total_value )
}

/// Return value of LPs in the discount vault
fn get_discounts_vault_value(
    querier: QuerierWrapper,
    config: Config,
    user: String,
) -> StdResult<Decimal>{

     //Get user info from the Gauge Vault
     let user = querier.query::<Discount_UserResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().discount_vault_contract.unwrap().to_string(),
        msg: to_binary(&Discount_QueryMsg::User {
            user,
            minimum_deposit_time: Some(config.minimum_time_in_network),
        })?,
    }))?;

    Ok( Decimal::from_ratio(user.discount_value, Uint128::one()) )

}

// Return value of staked MBRN & pending rewards
fn get_staked_MBRN_value(
    querier: QuerierWrapper,
    config: Config,
    user: String,
    mbrn_price: Decimal,
    credit_price: Decimal,
) -> StdResult<Decimal>{

    let mut user_stake = querier.query::<StakerResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::UserStake {
            staker: user.clone(),
        })?,
    }))?
    .total_staked;

    let rewards = querier.query::<RewardsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().staking_contract.to_string(),
        msg: to_binary(&Staking_QueryMsg::StakerRewards {
            staker: user.clone(),
        })?,
    }))?;

    //Add accrued interest to user_stake
    user_stake += rewards.accrued_interest;

    //Calculate staked value from reward.claimables
    let mut staked_value = Decimal::zero();
    
    for asset in rewards.claimables {

        let mut price = querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().oracle_contract.to_string(),
            msg: to_binary(&Oracle_QueryMsg::Price {
                asset_info: asset.info,
                twap_timeframe: 60,
                basket_id: None,
            })?,
        }))?
        .price;

        if price < credit_price { price = credit_price }

        let value = decimal_multiplication(price, Decimal::from_ratio(asset.amount, Uint128::one()))?;

        staked_value += value;
    }

    //Add MBRN value to staked_value
    let value = decimal_multiplication(mbrn_price, Decimal::from_ratio(user_stake, Uint128::one()))?;

    staked_value += value;
    
    Ok( staked_value )
}

/// Return user's total Stability Pool value from credit & MBRN incentives 
fn get_sp_value(
    querier: QuerierWrapper,
    config: Config,
    current_block_time: u64,
    user: String,
    mbrn_price: Decimal,
) -> StdResult<Decimal>{

    //Query Stability Pool to see if the user has funds
    let user_deposits = querier.query::<AssetPool>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().stability_pool_contract.to_string(),
        msg: to_binary(&SP_QueryMsg::AssetPool { 
            user: None, 
            start_after: None,
            deposit_limit: None 
        })?,
    }))?
    .deposits
        .into_iter()
        //Filter for user deposits deposited for a minimum_time_in_network
        .filter(|deposit| deposit.user.to_string() == user && current_block_time - deposit.deposit_time > (config.clone().minimum_time_in_network * SECONDS_PER_DAY))
        .collect::<Vec<Deposit>>();

    let total_user_deposit: Decimal = user_deposits
        .iter()
        .map(|user_deposit| user_deposit.amount)
        .collect::<Vec<Decimal>>()
        .into_iter()
        .sum();

    //Query for user accrued incentives
    let accrued_incentives = querier.query::<Uint128>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().stability_pool_contract.to_string(),
        msg: to_binary(&SP_QueryMsg::UnclaimedIncentives {
            user: user.clone(),
        })?,
    }))?;
    let incentive_value = decimal_multiplication(mbrn_price, Decimal::from_ratio(accrued_incentives, Uint128::one()))?;

    //Query for user claimable assets
    let res = querier.query::<ClaimsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().stability_pool_contract.to_string(),
        msg: to_binary(&SP_QueryMsg::UserClaims {
            user: user.clone(),
        })?,
    }))?;

    let mut claims_value = Decimal::zero();
    
    for asset in res.claims {

        let price = querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().oracle_contract.to_string(),
            msg: to_binary(&Oracle_QueryMsg::Price {
                asset_info: asset.info,
                twap_timeframe: 60,
                basket_id: None,
            })?,
        }))?
        .price;

        let value = decimal_multiplication(price, Decimal::from_ratio(asset.amount, Uint128::one()))?;

        claims_value += value;
    }

    //Return total_value
    Ok( total_user_deposit + incentive_value + claims_value)
}
