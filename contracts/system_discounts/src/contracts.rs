use std::str::FromStr;

use cosmwasm_std::{
    entry_point, to_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo,
    QueryRequest, Response, StdResult, Uint128, WasmQuery, QuerierWrapper, attr,
};
use cw2::set_contract_version;

use osmosis_std::shim::Duration;
use osmosis_std::types::osmosis::lockup::{LockupQuerier, AccountLockedLongerDurationDenomResponse};

use membrane::math::{decimal_multiplication, decimal_division};
use membrane::system_discounts::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig, UserDiscountResponse};
use membrane::stability_pool::{QueryMsg as SP_QueryMsg, ClaimsResponse};
use membrane::staking::{QueryMsg as Staking_QueryMsg, Config as Staking_Config, StakerResponse, RewardsResponse};
use membrane::discount_vault::{QueryMsg as Discount_QueryMsg, UserResponse as Discount_UserResponse, Config as DV_Config};
use membrane::cdp::{QueryMsg as CDP_QueryMsg, PositionResponse};
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::types::{AssetInfo, Basket, Deposit, AssetPool, LPPoolInfo};

use crate::error::ContractError;
use crate::state::{CONFIG, OWNERSHIP_TRANSFER};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "system_discounts";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_PER_DAY: u64 = 86_400u64;

#[cfg_attr(not(feature = "library"), entry_point)]
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
        discount_vault_contract: vec![],
        minimum_time_in_network: msg.minimum_time_in_network,
    };
    //Store optionals
    if let Some(lockdrop_contract) = msg.lockdrop_contract{
        config.lockdrop_contract = Some(deps.api.addr_validate(&lockdrop_contract)?);
    }
    if let Some(discount_vault_contract) = msg.discount_vault_contract{
        config.discount_vault_contract.push(deps.api.addr_validate(&discount_vault_contract)?);
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
    let mut attrs = vec![attr("method", "update_config")];

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    //Save optionals
    if let Some(addr) = update.owner {
        let valid_addr = deps.api.addr_validate(&addr)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));    
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
    if let Some((addr, add)) = update.discount_vault_contract {
        let addr = deps.api.addr_validate(&addr)?;
        //Add or remove address
        if add {
            config.discount_vault_contract.push(addr);
        } else {
            config.discount_vault_contract.retain(|x| x != &addr);
        }
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
)-> StdResult<UserDiscountResponse>{
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

    Ok(UserDiscountResponse {
        user,
        discount: percent_discount,
    })
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
    let credit_denom = basket.clone().credit_asset.info;

    let mbrn_price_res = match querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().oracle_contract.to_string(),
        msg: to_binary(&Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken { denom: config.clone().mbrn_denom },
            twap_timeframe: 60,
            oracle_time_limit: 600,
            basket_id: None,
        })?,
    })){
        Ok(price_res) => price_res,
        //Default to CDT price
        Err(_) => credit_price.clone()
    };

    //Initialize variables
    let mut total_value = Decimal::zero();
    let mut accepted_lps: Vec<LPPoolInfo> = vec![];
    let mut valid_denoms: Vec<AssetInfo> = vec![];

    total_value += get_sp_value(querier, config.clone(), env.clone().block.time.seconds(), user.clone(), mbrn_price_res.price)?;
    total_value += get_staked_MBRN_value(querier, config.clone(), user.clone(), mbrn_price_res.clone(), credit_price.clone().price)?;

            //Add to accepted LPs
            let res: DV_Config = querier.query_wasm_smart(discount_vault, &Discount_QueryMsg::Config {  })?;

            for lp in res.accepted_LPs {
                if let false = accepted_lps.contains(&lp) {
                    accepted_lps.push(lp);
                }
            }
        }
        valid_denoms = accepted_lps.clone().into_iter().map(|x| x.share_token).collect::<Vec<AssetInfo>>();    
    }   

    total_value += get_incentive_gauge_value(querier, valid_denoms, user.clone(), config.clone().minimum_time_in_network, credit_price.clone(), credit_denom.to_string())?;
    
    Ok( total_value )
}

/// Return value of LPs in Osmosis Incentive Lockups
fn get_incentive_gauge_value(
    querier: QuerierWrapper,
    valid_denoms: Vec<AssetInfo>,
    user: String,
    minimum_time_in_network: u64,
    debt_price: Decimal,
    debt_denom: String,
) -> StdResult<Decimal>{
    //Initialize total value
    let mut total_value = Decimal::zero();

    //Parse through all valid denoms
    for denom in valid_denoms {
        let res: AccountLockedLongerDurationDenomResponse = LockupQuerier::account_locked_longer_duration_denom(
            &LockupQuerier::new(&querier),
            user.clone(),
            Some(Duration { 
                seconds: (minimum_time_in_network * SECONDS_PER_DAY) as i64, 
                nanos: 0 }),
            denom.to_string(),
        )?;


        let mut user_locked_debt = vec![];
        //Parse through all locked coins
        for user_lock_period in res.locks.clone().into_iter(){
            user_locked_debt.extend(
                user_lock_period.coins
                    .into_iter()
                    .filter(|coin| coin.denom == debt_denom)
            )
        }

        //Get total locked debt
        let mut total_locked_debt = Uint128::zero();

        for coin in user_locked_debt {
            total_locked_debt += Uint128::from_str(&coin.amount)?;
        }
        
        //Get total locked debt value
        let total_debt_value = decimal_multiplication(
            Decimal::from_ratio(total_locked_debt, Uint128::one()),
            debt_price
        )?;
        
        //Multiply LP value by 2 to account for the non-debt side
        //Assumption of a 50:50 LP, meaning unbalanced stableswaps are boosted
        //This could be a "bug" but for now it's a feature to benefit LPs during distressed times
        total_value = decimal_multiplication(
            Decimal::percent(200),
            total_debt_value
        )?;

    }
    
    Ok(total_value)
}

/// Return value of LPs in the discount vault
fn get_discounts_vault_value(
    querier: QuerierWrapper,
    discount_vault: Addr,
    user: String,
    minimum_time_in_network: u64,
) -> StdResult<Decimal>{

    //Get user capital from the Gauge Vault
    let user = querier.query::<Discount_UserResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().discount_vault_contract.unwrap().to_string(),
        msg: to_binary(&Discount_QueryMsg::User {
            user,
            minimum_deposit_time: Some(minimum_time_in_network),
        })?,
    }))?;

    Ok( Decimal::from_ratio(user.discount_value, Uint128::one()) )

}

// Return value of staked MBRN & pending rewards
fn get_staked_MBRN_value(
    querier: QuerierWrapper,
    config: Config,
    user: String,
    mbrn_price_res: PriceResponse,
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
        msg: to_binary(&Staking_QueryMsg::UserRewards {
            user: user.clone(),
        })?,
    }))?;

    //Add accrued interest to user_stake
    user_stake += rewards.accrued_interest;

    //Calculate staked value from reward.claimables
    let mut staked_value = Decimal::zero();
    
    for asset in rewards.claimables {

        let mut price_res = querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().oracle_contract.to_string(),
            msg: to_binary(&Oracle_QueryMsg::Price {
                asset_info: asset.info,
                twap_timeframe: 60,
                oracle_time_limit: 600,
                basket_id: None,
            })?,
        }))?;

        if price_res.price < credit_price { price_res.price = credit_price }

        let value = price_res.get_value(asset.amount)?;

        staked_value += value;
    }

    //Add MBRN value to staked_value
    let value = mbrn_price_res.get_value(user_stake)?;

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

        let price_res = querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.clone().oracle_contract.to_string(),
            msg: to_binary(&Oracle_QueryMsg::Price {
                asset_info: AssetInfo::NativeToken { denom: asset.denom },
                twap_timeframe: 60,
                oracle_time_limit: 600,
                basket_id: None,
            })?,
        }))?;

        let value = price_res.get_value(asset.amount)?;

        claims_value += value;
    }

    //Return total_value
    Ok( total_user_deposit + incentive_value + claims_value)
}
