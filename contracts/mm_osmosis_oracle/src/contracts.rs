use std::str::FromStr;

use cosmwasm_std::{
    attr, entry_point, to_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Order, QuerierWrapper, Response, StdError, StdResult, Storage, Uint128
};
use cw2::set_contract_version;

use osmosis_std::types::osmosis::poolmanager::v1beta1::PoolmanagerQuerier;
use osmosis_std::types::osmosis::gamm::v1beta1::GammQuerier;
use pyth_sdk_cw::{PriceFeedResponse, query_price_feed, PriceIdentifier, PriceFeed};

use osmosis_std::types::osmosis::twap::v1beta1 as TWAP;

use membrane::math::{decimal_division, decimal_multiplication};
use membrane::mm_oracle::{Config, ExecuteMsg, InstantiateMsg, QueryMsg, MigrateMsg};
use membrane::oracle::PriceResponse;
use membrane::mars_vault_token::QueryMsg as Vault_QueryMsg;
use membrane::types::{AssetInfo, OsmosisOracleInfo, PoolInfo, PoolStateResponse, PriceInfo, VaultTokenInfo};

// Local AssetResponse type for this contract
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct AssetResponse {
    pub asset_info: AssetInfo,
    pub oracle_info: Vec<OsmosisOracleInfo>,
}

use crate::error::ContractError;
use crate::state::{ASSETS, CONFIG, OWNERSHIP_TRANSFER};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "mm_osmosis_oracle";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//  Static prices
const STATIC_USD_PRICE: Decimal = Decimal::one();
// Mainnet Pyth Price ID
// https://pyth.network/developers/price-feed-ids#cosmwasm-stable
const OSMO_USD_PRICE_ID: &str = "5867f5683c757393a0670ef0f701490950fe93fdb006d181c8265a831ac0c5c6"; 

#[cfg_attr(not(feature = "library"), entry_point)]
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
            owner: deps.api.addr_validate(&msg.clone().owner.unwrap())?,
            pyth_address: Some(deps.api.addr_validate(&"osmo1hpdzqku55lmfmptpyj6wdlugqs5etr6teqf7r4yqjjrxjznjhtuqqu5kdh")?), //mainnet: osmo13ge29x4e2s63a8ytz2px8gurtyznmue4a69n5275692v3qn3ks8q7cwck7
        };
    } else {
        config = Config {
            owner: info.sender,
            pyth_address: Some(deps.api.addr_validate(&"osmo1hpdzqku55lmfmptpyj6wdlugqs5etr6teqf7r4yqjjrxjznjhtuqqu5kdh")?), //mainnet: osmo13ge29x4e2s63a8ytz2px8gurtyznmue4a69n5275692v3qn3ks8q7cwck7
        };
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
            caller,
        } => add_asset(deps, env, info, asset_info, oracle_info, caller),
        ExecuteMsg::EditAsset {
            asset_info,
            oracle_info,
            caller,
            remove,
        } => edit_asset(deps, env, info, asset_info, oracle_info, caller, remove),
        ExecuteMsg::UpdateConfig {
            owner,
            pyth_address,
        } => update_config(deps, env, info, owner, pyth_address),
    }
}

/// Edit oracle info for an asset
/// or remove asset from the contract
fn edit_asset(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset_info: String,
    oracle_info: Option<OsmosisOracleInfo>,
    mut caller: String,
    remove: bool,
) -> Result<Response, ContractError> {

    let config = CONFIG.load(deps.storage)?;

    //Anyone can edit_assets
    //Only the owner can use caller's that are not themselves
    if info.sender != config.owner {
        caller = info.sender.to_string();
    }

    let mut attrs = vec![
        attr("action", "edit_asset"),
        attr("asset", asset_info.clone()),
        attr("removed", remove.to_string()),
    ];

    //Remove or edit 
    if remove {
        ASSETS.remove(deps.storage, (caller.clone(), asset_info.clone()));
    } else if oracle_info.is_some() {
        let oracle_info = oracle_info.unwrap();
        //Update Asset
        ASSETS.update(
            deps.storage,
            (caller.clone(), asset_info.clone()),
            |oracle: Option<OsmosisOracleInfo>| -> Result<OsmosisOracleInfo, ContractError> {
                //If oracle exists, replace it
                if oracle.is_some() {
                    Ok(oracle_info.clone())
                } else {
                    //Add as if new
                    Ok(oracle_info.clone())
                }
            },
        )?;

        attrs.push(attr("new_oracle_info", oracle_info.to_string()));

        //Test the new price source
        let price = get_asset_price(deps.storage, deps.querier, env, asset_info, info.sender.to_string(), 0, 0);
        attrs.push(attr("price", format!("{:?}", price)));
    }
        

    Ok(Response::new().add_attributes(attrs))
}

/// Add an asset alongside its oracle info
fn add_asset(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    asset_info: String,
    oracle_info: OsmosisOracleInfo,
    mut caller: String,
) -> Result<Response, ContractError> {

    let config = CONFIG.load(deps.storage)?;

    let mut attrs = vec![
        attr("action", "add_asset"),
        attr("asset", asset_info.clone()),
    ];

    //Anyone can add_assets
    //Only the owner can use caller's that are not themselves
    if info.sender != config.owner {
        caller = info.sender.to_string();
    }



    //Save Oracle
    match ASSETS.load(deps.storage, (caller.clone(), asset_info.clone())) {
        Err(_err) => {
            //Save new oracle info
            ASSETS.save(deps.storage, (caller.clone(), asset_info.clone()), &oracle_info.clone())?;
            attrs.push(attr("added", "true"));
        }
        Ok(_oracles) => {
            //Update oracle info
            ASSETS.update(
                deps.storage,
                (caller.clone(), asset_info.clone()),
                |oracle| -> Result<OsmosisOracleInfo, ContractError> {
                    match oracle {
                        Some(_) => Ok(oracle_info.clone()),
                        None => Ok(oracle_info.clone()),
                    }
                },
            )?;

            attrs.push(attr("added", "true"));
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
    pyth_address: Option<String>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![attr("method", "update_config")];

    //Assert Authority or transfer ownership 
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }    
    } 
    
    if let Some(owner) = owner {
        let valid_addr = deps.api.addr_validate(&owner)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));  
    }
    if let Some(pyth_address) = pyth_address {
        config.pyth_address = Some(deps.api.addr_validate(&pyth_address)?);
    }

    CONFIG.save(deps.storage, &config)?;
    attrs.push(attr("updated_config", format!("{:?}", config)));

    Ok(Response::new().add_attributes(attrs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Prices {
            caller,
            asset_infos,
            twap_timeframe,
            oracle_time_limit,
        } => {
            
            to_binary(&get_asset_prices(
                deps.storage, 
                deps.querier,
                env,
                asset_infos,
                caller,
                twap_timeframe,
                oracle_time_limit,
            )?)
        },
        QueryMsg::Assets { asset_infos, caller } => to_binary(&get_assets(deps, asset_infos, caller)?),
    }
}

/// Get underlying asset price
/// Get underlying token amount
/// Calculate vault token price.
pub fn get_vault_token_price(
    querier: QuerierWrapper,
    vault_info: VaultTokenInfo,
    decimals: u64,
    underlying_price: PriceResponse,
) -> StdResult<PriceResponse>{    

    //Query underlying amount for 1 vault token (1_000_000_000_000)
    //Bc The vault token is using a minimum of 6 decimal place ASSETS, a single token will always be 1_000_000 * (10 ^ DECIMALS)
    let underlying_token_amount: Uint128 = querier.query_wasm_smart::<Uint128>(
        vault_info.clone().vault_contract,//Uint128::new(1_000_000_000_000)
        &Vault_QueryMsg::VaultTokenUnderlying { vault_token_amount: Uint128::new(1u128 * 10u128.pow(decimals as u32)) },
    )?;

    //Calculate value of Assets in 1 vault token
    let vault_token_value = underlying_price.get_value(underlying_token_amount)?;
        

    Ok(PriceResponse { 
        prices: vec![],
        price: vault_token_value,
        decimals,
    })
}

/// Calculate LP share token value.
/// Calculate LP price.
pub fn get_lp_price(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    pool_info: PoolInfo,    
    twap_timeframe: u64, //in minutes
    oracle_time_limit: u64, //in seconds
    caller: String,
) -> StdResult<PriceResponse>{
    let mut asset_values: Vec<Decimal> = vec![];

    //Get asset prices
    let (asset_prices, oracle_sources) = {
        let res = get_asset_prices(
            storage,
            querier.clone(),
            env,
            pool_info.clone().asset_infos.iter().map(|asset_info| asset_info.info.clone().to_string()).collect(),
            caller.clone(),
            twap_timeframe,
            oracle_time_limit,
        )?;

        let mut price_infos = vec![];

        //Store price infos
        res.clone()
            .into_iter() 
            .for_each(|price| 
                {
                    price_infos.extend(price.clone().prices);
                });
        
        (res, price_infos)
    };

    //Calculate share value
    //Query share asset amount
    let share_asset_amounts = get_pool_state(
        querier.clone(),
        pool_info.pool_id,
    )?
        .shares_value(1_000_000_000_000_000_000u128); //1_000_000_000_000_000_000 = 1 pool share token

    //Calculate value of Assets in 1 share token
    for (i, price) in asset_prices.into_iter().enumerate() {
        //Assert we are pulling asset amount from the correct asset
        let asset_share =
            match share_asset_amounts.clone().into_iter().find(|coin| {
                AssetInfo::NativeToken {
                    denom: coin.denom.clone(),
                } == pool_info.clone().asset_infos[i].info
            }) {
                Some(coin) => coin,
                None => {
                    return Err(StdError::GenericErr {
                        msg: format!(
                            "Invalid asset denom: {}",
                            pool_info.clone().asset_infos[i].info
                        ),
                    })
                }
            };

        //Price * # of assets in 1 LP share token
        asset_values.push(price.get_value(Uint128::from_str(&asset_share.amount)?)?);
    }

    //Calculate LP price as the value of 1 share token
    let LP_price = {
        asset_values
            .clone()
            .into_iter()
            .sum::<Decimal>()
    };

    Ok(PriceResponse { 
        prices: oracle_sources,
        price: LP_price,
        decimals: 18u64,
    })
}

/// Returns PoolStateResponse for a specified pool id
fn get_pool_state(
    querier: QuerierWrapper,
    pool_id: u64,
) -> StdResult<PoolStateResponse> {
    let liquidity_res: osmosis_std::types::osmosis::poolmanager::v1beta1::TotalPoolLiquidityResponse = PoolmanagerQuerier::new(&querier).total_pool_liquidity(pool_id)?;
    let shares_res: osmosis_std::types::osmosis::gamm::v1beta1::QueryTotalSharesResponse = match GammQuerier::new(&querier).total_shares(pool_id){
        Ok(res) => res,
        //We return None as it'll error for CL pools but I'm pretty sure we need this query for GAMM pricing in the oracle
        Err(_) => osmosis_std::types::osmosis::gamm::v1beta1::QueryTotalSharesResponse { total_shares: None }
    };
        
    Ok(PoolStateResponse { 
        assets: liquidity_res.liquidity, 
        shares: shares_res.total_shares.unwrap_or_default(),
    })
    
}

/// Return list of queryable assets
fn get_assets(deps: Deps, asset_infos: Vec<AssetInfo>, caller: String) -> StdResult<Vec<AssetResponse>> {
    let mut resp = vec![];
    for asset_info in asset_infos {
        let asset_oracle = ASSETS.load(deps.storage, (caller.clone(), asset_info.to_string()))?;

        resp.push(AssetResponse {
            asset_info,
            oracle_info: vec![asset_oracle],
        });
    }

    Ok(resp)
}

/// Return Asset price info as a PriceResponse
fn get_asset_price(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    asset_info: String,
    caller_address: String,
    twap_timeframe: u64, //in minutes
    oracle_time_limit: u64, //in seconds
) -> StdResult<PriceResponse> { //Return Asset Price
    //Load state
    let config: Config = CONFIG.load(storage)?;
    let asset_oracle_info = ASSETS.load(storage, (caller_address.clone(), asset_info.clone()))?;
    let oracle_info = asset_oracle_info;

    let mut oracle_prices = vec![];
    let mut pyth_feed_errored = false;

    //Use Pyth USD-quoted price feeds first if available
    if let Some(pyth_address) = config.clone().pyth_address {
        if let Some(feed_id) = oracle_info.clone().pyth_price_feed_id {
            //Query USD price from Pyth
            let price_feed_response: PriceFeedResponse = match query_price_feed(
                &querier, 
                pyth_address,
                PriceIdentifier::from_hex(&feed_id).map_err(|err| StdError::GenericErr { msg: err.to_string() })?,
            ){
                    Ok(res) => res,
                    Err(_) => {
                        pyth_feed_errored = true;
                        //If Pyth fails, skip to USD-par pricing
                        PriceFeedResponse {
                            price_feed: PriceFeed::default(),
                        }
                    }
                };
            
            //Query unscaled price
            let price_feed = price_feed_response.price_feed;
            let price = price_feed
                .get_ema_price_no_older_than(env.block.time.seconds() as i64, oracle_time_limit);

            //If price was queried && within the time limit, scale it & use it
            //If not, skip to Osmosis TWAP pricing
            let mut pyth_price: Decimal = Decimal::zero();
            match price {
                Some(price) => {
                    //Scale price using given exponent
                    match price.expo > 0 {
                        true => {
                            pyth_price = decimal_multiplication(
                                Decimal::from_str(&price.price.to_string())?, 
                                Decimal::from_ratio(Uint128::new(10), Uint128::one()).checked_pow(price.expo as u32)?
                            )?;
                        },
                        //If the exponent is negative we divide, it should be for most if not all
                        false => {
                            pyth_price = decimal_division(
                                Decimal::from_str(&price.price.to_string())?, 
                                Decimal::from_ratio(Uint128::new(10), Uint128::one()).checked_pow((price.expo*-1) as u32)?
                            )?;
                        }
                    };                   
                    

                    //Push Pyth USD price
                    oracle_prices.push(PriceInfo {
                        source: String::from("pyth"),
                        price: pyth_price,
                    });
                },
                None => {
                    pyth_feed_errored = true;
                }
            }

            //Return Pyth only price if it was queried successfully
            if !pyth_feed_errored {
                return Ok(PriceResponse {
                    prices: oracle_prices,
                    price: pyth_price,
                    decimals: oracle_info.decimals,
                });
            }
        }
    }
    /// If there is no return above, query starting from Osmosis TWAPs

    
    //twap_timeframe = MINUTES * SECONDS_PER_MINUTE
    let twap_timeframe: u64 = twap_timeframe * 60;
    let start_time: u64 = env.block.time.seconds() - twap_timeframe;

    let mut asset_price_in_lp_steps = vec![];


    //Query prices from the TWAP sources
    //This can use multiple pools to calculate our price
    for pool in oracle_info.clone().pools_for_osmo_twap {

        let res: TWAP::GeometricTwapToNowResponse = TWAP::TwapQuerier::new(&querier).geometric_twap_to_now(
            pool.clone().pool_id, 
            pool.clone().base_asset_denom, 
            pool.clone().quote_asset_denom, 
            Some(osmosis_std::shim::Timestamp {
                seconds:  start_time as i64,
                nanos: 0,
            }),
        )?;

        //Push TWAP
        asset_price_in_lp_steps.push(Decimal::from_str(&res.geometric_twap)?);
    }

    //Multiply prices to denominate in USDC
    let mut asset_price_in_usdc = {
        let mut final_price = Decimal::one();
        //If no prices were queried & there is no vault info, return error
        if asset_price_in_lp_steps.len() == 0 && oracle_info.clone().vault_info.is_none() {
            return Err(StdError::GenericErr {
                msg: String::from("No TWAP prices found"),
            });
        }
        //if there is vault info, we can assume its a USDC vault bc non-USDC will have TWAP pools

        //Find asset price in USDC
        //Multiply prices to get the desired Quote
        for price in asset_price_in_lp_steps {
            final_price = decimal_multiplication(final_price, price)?;
        } 
        //Results in slight error: (https://medium.com/reflexer-labs/analysis-of-the-rai-twap-oracle-20a01af2e49d)

        final_price
    };

    // Correct for decimal differences between collateral and USDC.
    // This logic mirrors the oracle contract's decimal adjustment.
    // We assume the final quote asset is Noble USDC, which has 6 decimals.
    let collateral_decimals = oracle_info.decimals;
    const USDC_DECIMALS: u64 = 6;

    if collateral_decimals > USDC_DECIMALS {
        let power = collateral_decimals - USDC_DECIMALS;
        asset_price_in_usdc = decimal_multiplication(
            asset_price_in_usdc, 
            Decimal::from_ratio(Uint128::new(10).pow(power as u32), Uint128::one()),
        )?;
    } else if collateral_decimals < USDC_DECIMALS {
        let power = USDC_DECIMALS - collateral_decimals;
        asset_price_in_usdc = decimal_division(
            asset_price_in_usdc,
            Decimal::from_ratio(Uint128::new(10).pow(power as u32), Uint128::one()),
        )?;
    }
    // If decimals are equal, no adjustment is needed.
    //Results in slight error: (https://medium.com/reflexer-labs/analysis-of-the-rai-twap-oracle-20a01af2e49d)

    Ok(PriceResponse { 
        prices: vec![], 
        price: asset_price_in_usdc, 
        decimals: oracle_info.decimals.clone() 
    })
}

/// Return list of asset price info as list of PriceResponse
fn get_asset_prices(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    asset_infos: Vec<String>,
    caller_address: String,
    twap_timeframe: u64, //in minutes
    oracle_time_limit: u64, //in seconds
) -> StdResult<Vec<PriceResponse>> {

    //Enforce Vec max size
    if asset_infos.len() > 50 {
        return Err(StdError::GenericErr {
            msg: String::from("Max asset_infos length is 50"),
        });
    }

    let mut price_responses = vec![];

    for asset in asset_infos {
        let asset_oracle_info = ASSETS.load(storage, (caller_address.clone(), asset.clone()))?;
        
        //Get the oracle info for the asset
        let oracle_info = asset_oracle_info;
        
        //Switch based on if asset is an LP 
        match oracle_info.clone().lp_pool_info {
            Some(pool_info) => {
                //If asset is an LP, get the LP price
                price_responses.push(get_lp_price(
                    storage,
                    querier.clone(),
                    env.clone(),
                    pool_info,
                    twap_timeframe,
                    oracle_time_limit,
                    caller_address.clone(),
                )?);
            },
            None => {
                //If its a vault token get the vault token price
                if let Some(vault_info) = oracle_info.clone().vault_info {
                    let underlying_price = get_asset_price(
                        storage,
                        querier.clone(),
                        env.clone(),
                        asset.clone(),
                        caller_address.clone(),
                        twap_timeframe,
                        oracle_time_limit,
                    )?;
                    
                    price_responses.push(get_vault_token_price(
                        querier.clone(),
                        vault_info,
                        oracle_info.decimals,
                        underlying_price,
                    )?);
                } else {
                    //Get the asset price directly
                    let price = get_asset_price(
                        storage,
                        querier.clone(),
                        env.clone(),
                        asset.clone(),
                        caller_address.clone(),
                        twap_timeframe,
                        oracle_time_limit,
                    )?;
                    price_responses.push(price);
                }
            }
        }
    }

    Ok(price_responses)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    //Get state keys for oracle info
    let keys = ASSETS
        .range(deps.storage, None, None, Order::Ascending)
        .map(|asset| {
            let ((caller, asset_key), _asset_info) = asset?;

            Ok((caller, asset_key))

        })
        .collect::<StdResult<Vec<(String, String)>>>()?;
    
    //Set underlying token field for all stored Assets
    for (caller, asset_key) in keys {
        let mut asset_info = ASSETS.load(deps.storage, (caller.clone(), asset_key.clone()))?;

        asset_info.vault_info = None;

        ASSETS.save(deps.storage, (caller, asset_key), &asset_info)?;
    }
    Ok(Response::default())
}
