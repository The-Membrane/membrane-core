use cosmwasm_std::{
    entry_point, to_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo,
    QueryRequest, Response, StdResult, Uint128, WasmQuery, QuerierWrapper,
};
use cw2::set_contract_version;

use membrane::math::{decimal_multiplication, decimal_division};
use membrane::system_discounts::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::stability_pool::{QueryMsg as SP_QueryMsg, DepositResponse, ClaimsResponse};
use membrane::staking::{QueryMsg as Staking_QueryMsg, Config as Staking_Config, StakerResponse, RewardsResponse};
use membrane::lockdrop::{QueryMsg as Lockdrop_QueryMsg, UserResponse};
use membrane::discount_vault::{QueryMsg as Discount_QueryMsg, UserResponse as Discount_UserResponse};
use membrane::positions::{QueryMsg as CDP_QueryMsg, BasketResponse, PositionsResponse};
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::types::{AssetInfo, DebtTokenAsset, Position};

use crate::error::ContractError;
use crate::state::CONFIG;

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "system_discounts";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");


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
        contract_addr: staking_contract.to_string(),
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
        lockdrop_contract: deps.api.addr_validate(&msg.lockdrop_contract)?,
        discount_vault_contract: deps.api.addr_validate(&msg.discount_vault_contract)?,
    };
    

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("config", format!("{:?}", config)))
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
            basket_id,
            mbrn_denom,
            positions_contract,
            oracle_contract,
            staking_contract,
            stability_pool_contract,
            lockdrop_contract,
            discount_vault_contract,
        } => update_config(
            deps, 
            info, 
            owner, 
            mbrn_denom,
            basket_id,
            oracle_contract,
            positions_contract,
            staking_contract,
            stability_pool_contract,
            lockdrop_contract,
            discount_vault_contract,
        ),
    }
}

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    mbrn_denom: Option<String>,
    basket_id: Option<Uint128>,
    oracle_contract: Option<String>,
    positions_contract: Option<String>,
    staking_contract: Option<String>,
    stability_pool_contract: Option<String>,
    lockdrop_contract: Option<String>,
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
    if let Some(basket_id) = basket_id {
        config.basket_id = basket_id;
    }
    if let Some(mbrn_denom) = mbrn_denom {
        config.mbrn_denom = mbrn_denom;
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
    if let Some(addr) = lockdrop_contract {
        config.lockdrop_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = discount_vault_contract {
        config.discount_vault_contract = deps.api.addr_validate(&addr)?;
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("new_config", format!("{:?}", config)))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::UserDiscount { user } => to_binary(&get_discount(deps, user)?),
    }
}

//Returns % of interest that is discounted
//i.e. 90% of 1% interest is .1% interest
fn get_discount(
    deps: Deps,
    user: String, 
)-> StdResult<Decimal>{
  
    //Load Config
    let config = CONFIG.load(deps.storage)?;

    //Get the value of the user's capital in..
    //Stake, SP & Queriable LPs
    let user_value_in_network = get_user_value_in_network(deps.querier, config.clone(), user.clone())?;

    //Get User's outstanding debt
    let user_positions: Vec<Position> = deps.querier.query::<PositionsResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().positions_contract.to_string(),
        msg: to_binary(&CDP_QueryMsg::GetUserPositions {
            basket_id: Some(config.clone().basket_id),
            user: user.clone(),
            limit: None,
        })?,
    }))?
    .positions;

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
            decimal_division(user_value_in_network, user_outstanding_debt)
        }
    };

    Ok(percent_discount)
}

//Get the value of the user's capital in..
//Stake, SP & Queriable LPs
fn get_user_value_in_network(
    querier: QuerierWrapper,
    config: Config,
    user: String,
)-> StdResult<Decimal>{

    let basket: BasketResponse = querier.query::<BasketResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().positions_contract.to_string(),
        msg: to_binary(&CDP_QueryMsg::GetBasket {
            basket_id: config.clone().basket_id,
        })?,
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

    total_value += get_sp_value(querier, config.clone(), user.clone(), basket.clone().credit_asset.info, mbrn_price)?;
    total_value += get_staked_MBRN_value(querier, config.clone(), user.clone(), mbrn_price.clone())?;
    total_value += get_discounts_vault_value(querier, config.clone(), user.clone())?;
    total_value += get_lockdrop_value(querier, config.clone(), user.clone(), credit_price.clone(), mbrn_price.clone())?;
    
    Ok( total_value )
}

//Returns total_value of incentives + LP
fn get_lockdrop_value(
    querier: QuerierWrapper,
    config: Config,
    user: String,    
    credit_price: Decimal,
    mbrn_price: Decimal,
) -> StdResult<Decimal>{
    
    //Get user info from the Gauge Vault
    let user = querier.query::<UserResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().lockdrop_contract.to_string(),
        msg: to_binary(&Lockdrop_QueryMsg::User {
            user,
        })?,
    }))?;
    let debt_token: DebtTokenAsset = user.total_debt_token;
    let accrued_incentives = user.incentives.amount;

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
    let value = decimal_multiplication(
        mbrn_price, 
        Decimal::from_ratio(accrued_incentives, Uint128::one())
    );

    total_value += value;

    Ok( total_value )

}

fn get_discounts_vault_value(
    querier: QuerierWrapper,
    config: Config,
    user: String,
) -> StdResult<Decimal>{

     //Get user info from the Gauge Vault
     let user = querier.query::<Discount_UserResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().discount_vault_contract.to_string(),
        msg: to_binary(&Discount_QueryMsg::User {
            user,
        })?,
    }))?;

    Ok( user.premium_user_value )

}

//Calc value of staked MBRN & pending rewards
fn get_staked_MBRN_value(
    querier: QuerierWrapper,
    config: Config,
    user: String,
    mbrn_price: Decimal,
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

        let price = querier.query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
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
    let value = decimal_multiplication(mbrn_price, Decimal::from_ratio(user_stake, Uint128::one()));

    staked_value += value;
    
    Ok( staked_value )
}

//Gets user total Stability Pool funds
fn get_sp_value(
    querier: QuerierWrapper,
    config: Config,
    user: String,
    asset_info: AssetInfo,
    mbrn_price: Decimal,
) -> StdResult<Decimal>{

    //Query Stability Pool to see if the user has funds
    let user_deposits = querier.query::<DepositResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().stability_pool_contract.to_string(),
        msg: to_binary(&SP_QueryMsg::AssetDeposits {
            user: user.clone(),
            asset_info: asset_info.clone(),
        })?,
    }))?
    .deposits;

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
            asset_info: asset_info.clone(),
        })?,
    }))?;
    let incentive_value = decimal_multiplication(mbrn_price, Decimal::from_ratio(accrued_incentives, Uint128::one()));

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

        let value = decimal_multiplication(price, Decimal::from_ratio(asset.amount, Uint128::one()));

        claims_value += value;
    }

    //Return total_value
    Ok( total_user_deposit + incentive_value + claims_value)
}
