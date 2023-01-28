use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use cosmwasm_std::{
    attr, entry_point, to_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, QuerierWrapper,
    Response, StdError, StdResult, Storage, Uint128, 
};
use cw2::set_contract_version;

use osmosis_std::types::osmosis::twap::v1beta1 as TWAP;

use membrane::math::{decimal_division, decimal_multiplication};
use membrane::oracle::{Config, AssetResponse, ExecuteMsg, InstantiateMsg, PriceResponse, QueryMsg};
use membrane::types::{AssetInfo, AssetOracleInfo, PriceInfo};

use crate::error::ContractError;
use crate::state::{ASSETS, CONFIG};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "oracle";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Time unit conversion rates
const MILLISECONDS_PER_MINUTE: i64 = 60_000i64;

pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let mut config: Config;
    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
            positions_contract: None,
        };
    } else {
        config = Config {
            owner: info.sender,
            osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
            positions_contract: None,
        };
    }

    if let Some(positions_contract) = msg.positions_contract {
        config.positions_contract = Some(deps.api.addr_validate(&positions_contract)?);
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AddAsset {
            asset_info,
            oracle_info,
        } => add_asset(deps, info, asset_info, oracle_info),
        ExecuteMsg::EditAsset {
            asset_info,
            oracle_info,
            remove,
        } => edit_asset(deps, info, asset_info, oracle_info, remove),
        ExecuteMsg::UpdateConfig {
            owner,
            osmosis_proxy,
            positions_contract,
        } => update_config(deps, env, info, owner, osmosis_proxy, positions_contract),
    }
}

/// Edit oracle info for an asset
/// or remove asset from the contract
fn edit_asset(
    deps: DepsMut,
    info: MessageInfo,
    asset_info: AssetInfo,
    oracle_info: Option<AssetOracleInfo>,
    remove: bool,
) -> Result<Response, ContractError> {

    let config = CONFIG.load(deps.storage)?;

    //Owner or Positions contract can Add_assets
    if info.sender != config.owner {
        if config.positions_contract.is_some() {
            if info.sender != config.positions_contract.unwrap() {
                return Err(ContractError::Unauthorized {});
            }
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    let mut attrs = vec![
        attr("action", "edit_asset"),
        attr("asset", asset_info.to_string()),
        attr("removed", remove.to_string()),
    ];

    //Remove or edit 
    if remove {
        ASSETS.remove(deps.storage, asset_info.to_string());
    } else if oracle_info.is_some() {
        let oracle_info = oracle_info.unwrap();
        //Update Asset
        ASSETS.update(
            deps.storage,
            asset_info.to_string(),
            |oracle| -> Result<Vec<AssetOracleInfo>, ContractError> {
                //If oracle list
                if let Some(mut oracle_list) = oracle {
                    //Find oracle
                    if let Some((i, _oracle)) = oracle_list
                        .clone()
                        .into_iter()
                        .enumerate()
                        .find(|(_index, info)| info.basket_id == oracle_info.basket_id)
                    {
                        oracle_list[i] = oracle_info.clone();
                    }

                    Ok(oracle_list)
                } else {
                    //Add as if new
                    Ok(vec![oracle_info.clone()])
                }
            },
        )?;

        attrs.push(attr("new_oracle_info", oracle_info.to_string()));
    }

    Ok(Response::new().add_attributes(attrs))
}

/// Add an asset alongside its oracle info
fn add_asset(
    deps: DepsMut,
    info: MessageInfo,
    asset_info: AssetInfo,
    oracle_info: AssetOracleInfo,
) -> Result<Response, ContractError> {

    let config = CONFIG.load(deps.storage)?;

    let mut attrs = vec![
        attr("action", "add_asset"),
        attr("asset", asset_info.to_string()),
    ];

    //Owner or Positions contract can Add_assets
    if info.sender != config.owner {
        if config.positions_contract.is_some() {
            if info.sender != config.positions_contract.unwrap() {
                return Err(ContractError::Unauthorized {});
            }
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    match asset_info.clone() {
        AssetInfo::Token { address } => {
            //Validate address
            deps.api.addr_validate(address.as_ref())?;
        }
        AssetInfo::NativeToken { denom: _ } => {}
    };

    //Save Oracle
    match ASSETS.load(deps.storage, asset_info.to_string()) {
        Err(_err) => {
            //Save new list to asset if its list is empty
            ASSETS.save(deps.storage, asset_info.to_string(), &vec![oracle_info])?;
            attrs.push(attr("added", "true"));
        }
        Ok(oracles) => {
            //Save oracle to asset, no duplicates
            if !oracles
                .into_iter().any(|oracle| oracle.basket_id == oracle_info.basket_id)
            {
                ASSETS.update(
                    deps.storage,
                    asset_info.to_string(),
                    |oracle| -> Result<Vec<AssetOracleInfo>, ContractError> {
                        match oracle {
                            Some(mut oracle_list) => {
                                oracle_list.push(oracle_info);
                                Ok(oracle_list)
                            }
                            None => Ok(vec![oracle_info]),
                        }
                    },
                )?;

                attrs.push(attr("added", "true"));
            } else {
                attrs.push(attr("added", "false"));
            }
        }
    }

    Ok(Response::new().add_attributes(attrs))
}

/// Update contract configuration
pub fn update_config(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    owner: Option<String>,
    osmosis_proxy: Option<String>,
    positions_contract: Option<String>,
) -> Result<Response, ContractError> {
    
    let mut config = CONFIG.load(deps.storage)?;

    //Owner or Positions contract can Add_assets
    if info.sender != config.owner {
        if config.positions_contract.is_some() {
            if info.sender != config.clone().positions_contract.unwrap() {
                return Err(ContractError::Unauthorized {});
            }
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    if let Some(owner) = owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }
    if let Some(osmosis_proxy) = osmosis_proxy {
        config.osmosis_proxy = deps.api.addr_validate(&osmosis_proxy)?;
    }
    if let Some(positions_contract) = positions_contract {
        config.positions_contract = Some(deps.api.addr_validate(&positions_contract)?);
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Price {
            asset_info,
            twap_timeframe,
            basket_id,
        } => to_binary(&get_asset_price(
            deps.storage,
            deps.querier,
            asset_info,
            twap_timeframe,
            basket_id,
        )?),
        QueryMsg::Prices {
            asset_infos,
            twap_timeframe,
        } => to_binary(&get_asset_prices(deps, asset_infos, twap_timeframe)?),
        QueryMsg::Assets { asset_infos } => to_binary(&get_assets(deps, asset_infos)?),
    }
}

/// Return list of queryable assets
fn get_assets(deps: Deps, asset_infos: Vec<AssetInfo>) -> StdResult<Vec<AssetResponse>> {
    let mut resp = vec![];
    for asset_info in asset_infos {
        let asset_oracle = ASSETS.load(deps.storage, asset_info.to_string())?;

        resp.push(AssetResponse {
            asset_info,
            oracle_info: asset_oracle,
        });
    }

    Ok(resp)
}

/// Return Asset price info as a PriceResponse
fn get_asset_price(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    asset_info: AssetInfo,
    twap_timeframe: u64, //in minutes
    basket_id_field: Option<Uint128>,
) -> StdResult<PriceResponse> {
    //Load Asset Info
    let asset_oracle_info = ASSETS.load(storage, asset_info.to_string())?;

    let mut basket_id = Uint128::new(1u128); //Defaults to first basket assuming thats the USD basket
    if let Some(id) = basket_id_field {
        basket_id = id;
    };

    //Find OracleInfo for the basket_id
    let oracle_info = if let Some(oracle_info) = asset_oracle_info
        .into_iter()
        .find(|oracle| oracle.basket_id == basket_id)
    {
        oracle_info
    } else {
        return Err(StdError::GenericErr {
            msg: String::from("Invalid basket_id"),
        });
    };

    let current_unix_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as i64,
        Err(_) => {
            return Err(StdError::GenericErr {
                msg: String::from("SystemTime before UNIX EPOCH!"),
            })
        }
    };

    //twap_timeframe = MINUTES * MILLISECONDS_PER_MINUTE
    let twap_timeframe: i64 = (twap_timeframe as i64 * MILLISECONDS_PER_MINUTE);
    let start_time: i64 = current_unix_time - twap_timeframe;

    let mut oracle_prices = vec![];

    //Set static price if some
    if oracle_info.static_price.clone().is_some() {
        oracle_prices.push(PriceInfo {
            source: String::from("static_price"),
            price: oracle_info.static_price.unwrap(),
        });
    } else {
        let mut price_steps = vec![];

        //Query price from the TWAP sources
        //This is if we need to use multiple pools to calculate our price
        for pool in oracle_info.osmosis_pools_for_twap {

            let res: TWAP::ArithmeticTwapToNowResponse = TWAP::TwapQuerier::new(&querier).arithmetic_twap_to_now(
                pool.clone().pool_id, 
                pool.clone().base_asset_denom, 
                pool.clone().quote_asset_denom, 
                Some(osmosis_std::shim::Timestamp {
                    seconds:  start_time,
                    nanos: 0,
                }),
            )?;

            //Push TWAP
            price_steps.push(Decimal::from_str(&res.arithmetic_twap).unwrap());
        }

        //Multiply prices
        let price = {
            let mut final_price = Decimal::one();
            for price in price_steps {
                final_price = decimal_multiplication(final_price, price)?;
            }

            final_price
        };
        //Results in slight error: (https://medium.com/reflexer-labs/analysis-of-the-rai-twap-oracle-20a01af2e49d)

        //Push TWAP
        oracle_prices.push(PriceInfo {
            source: String::from("osmosis"),
            price,
        });
    }

    //////If AssetOracleInfo gets more fields we can just push those prices here////

    //Get Median price
    let price = if oracle_prices.len() % 2 == 0 {
        let median_index = oracle_prices.len() / 2;

        decimal_division(oracle_prices[median_index].price + oracle_prices[median_index+1].price, Decimal::percent(2_00))?
        
    } else {
        let median_index = oracle_prices.len() / 2;
        oracle_prices[median_index + 1].price
    };


    Ok(PriceResponse {
        prices: oracle_prices,
        price,
    })
}

/// Return list of asset price info as list of PriceResponse
fn get_asset_prices(
    deps: Deps,
    asset_infos: Vec<AssetInfo>,
    twap_timeframe: u64,
) -> StdResult<Vec<PriceResponse>> {
    let mut price_responses = vec![];

    for asset in asset_infos {
        price_responses.push(get_asset_price(
            deps.storage,
            deps.querier,
            asset,
            twap_timeframe,
            None,
        )?);
    }

    Ok(price_responses)
}
