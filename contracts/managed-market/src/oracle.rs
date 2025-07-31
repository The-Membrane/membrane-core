use std::str::FromStr;

use cosmwasm_std::{to_binary, to_json_binary, Addr, Decimal, Env, QuerierWrapper, QueryRequest, StdResult, Storage, Uint128, WasmQuery};
use membrane::managed_market::{Config, MarketParams};
use membrane::oracle::PriceResponse;
use crate::error::ContractError;
use crate::positions::{NOBLE_USDC_DENOM};
use crate::state::CONFIG;
use membrane::types::VaultTokenInfo;
use membrane::math::{decimal_multiplication, decimal_division};
use osmosis_std::types::osmosis::twap::v1beta1 as TWAP;
use membrane::mars_vault_token::QueryMsg as Vault_QueryMsg;
use membrane::mm_oracle::QueryMsg as OracleQueryMsg;

const ORACLE_TIME_LIMIT: u64 = 60 * 10; //10 minutes

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


// pub fn get_collateral_price(
//     _storage: &dyn Storage,
//     querier: QuerierWrapper,
//     env: Env,
//     market: MarketParams,
//     twap: bool,
// ) -> Result<PriceResponse, ContractError> {
//     //Load state
//     // let config: Config = CONFIG.load(storage)?;
//     let asset_oracle_info = market.pool_for_oracle_and_liquidations;

//     //twap_timeframe = MINUTES * SECONDS_PER_MINUTE
//     let twap_timeframe: u64 = twap.then(|| (60 * 60)).unwrap_or(0);
//     let start_time: u64 = env.block.time.seconds() - twap_timeframe;

//     let mut asset_price_in_lp_steps = vec![];


//     //Query prices from the TWAP sources
//     //This can use multiple pools to calculate our price
//     for pool in asset_oracle_info.pools_for_osmo_twap.clone() {

//         let res: TWAP::GeometricTwapToNowResponse = TWAP::TwapQuerier::new(&querier).geometric_twap_to_now(
//             pool.clone().pool_id, 
//             pool.clone().base_asset_denom, 
//             pool.clone().quote_asset_denom, 
//             Some(osmosis_std::shim::Timestamp {
//                 seconds:  start_time as i64,
//                 nanos: 0,
//             }),
//         )?;

//         //Push TWAP
//         asset_price_in_lp_steps.push(Decimal::from_str(&res.geometric_twap)?);
//     }

//     //Multiply prices to denominate in USDC
//     let mut asset_price_in_usdc = {
//         let mut final_price = Decimal::one();
//         //If no prices were queried & there is no vault info, return error
//         if asset_price_in_lp_steps.len() == 0 && asset_oracle_info.vault_info.is_none() {
//             return Err(ContractError::CustomError {
//                 val: String::from("No TWAP prices found"),
//             });
//         }
//         //if there is vault info, we can assume its a USDC vault bc non-USDC will have TWAP pools

//         //Find asset price in USDC
//         //Multiply prices to get the desired Quote
//         for price in asset_price_in_lp_steps {
//             final_price = decimal_multiplication(final_price, price)?;
//         } 
//         //Results in slight error: (https://medium.com/reflexer-labs/analysis-of-the-rai-twap-oracle-20a01af2e49d)

//         final_price
//     };

//     // Correct for decimal differences between collateral and USDC.
//     // This logic mirrors the oracle contract's decimal adjustment.
//     // We assume the final quote asset is Noble USDC, which has 6 decimals.
//     let collateral_decimals = asset_oracle_info.decimals;
//     const USDC_DECIMALS: u64 = 6;

//     if collateral_decimals > USDC_DECIMALS {
//         let power = collateral_decimals - USDC_DECIMALS;
//         asset_price_in_usdc = decimal_multiplication(
//             asset_price_in_usdc, 
//             Decimal::from_ratio(Uint128::new(10).pow(power as u32), Uint128::one()),
//         )?;
//     } else if collateral_decimals < USDC_DECIMALS {
//         let power = USDC_DECIMALS - collateral_decimals;
//         asset_price_in_usdc = decimal_division(
//             asset_price_in_usdc,
//             Decimal::from_ratio(Uint128::new(10).pow(power as u32), Uint128::one()),
//         )?;
//     }
//     // If decimals are equal, no adjustment is needed.
//     //Results in slight error: (https://medium.com/reflexer-labs/analysis-of-the-rai-twap-oracle-20a01af2e49d)



//     //Set asset price. 
//     //This is either the collateral price or the underlying price of the vault token asset
//     let asset_price = PriceResponse { 
//         prices: vec![], 
//         price: asset_price_in_usdc, 
//         decimals: asset_oracle_info.decimals.clone() 
//     };


//     if let Some(vault_info) = asset_oracle_info.clone().vault_info {
//         Ok(get_vault_token_price(
//             querier.clone(),
//             vault_info,
//             asset_oracle_info.decimals,
//             asset_price
//         )?)

//     } else {
//         return Ok(asset_price)
//     }
// }

pub fn get_asset_prices(
    querier: QuerierWrapper,
    config: Config,
    contract_address: String,
    twap: bool,
    asset_infos: Vec<String>,
) -> Result<Vec<PriceResponse>, ContractError> {

    //twap_timeframe = MINUTES * SECONDS_PER_MINUTE
    let twap_timeframe: u64 = twap.then(|| (60 * 60)).unwrap_or(0);

    let prices = match querier.query::<Vec<PriceResponse>>(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.clone().oracle_contract.unwrap_or_else(|| Addr::unchecked("")).to_string(),
        msg: to_json_binary(&OracleQueryMsg::Prices {
            caller: contract_address.clone(),
            asset_infos,
            twap_timeframe,
            oracle_time_limit: ORACLE_TIME_LIMIT,
        })?,
    })) {
        Ok(res) => res,
        Err(err) => {
            //if the oracle is down, error
            return Err(ContractError::CustomError { val: format!("Failed to get asset prices: {:?}", err) })
        }
    };

    return Ok(prices)

    // todo!();
    //////MOVE TO EXTERNAL CONTRACT/////
    // let start_time: u64 = env.block.time.seconds() - twap_timeframe;

    // //Query CDT/USDC price
    // let res: TWAP::GeometricTwapToNowResponse = TWAP::TwapQuerier::new(&querier).geometric_twap_to_now(
    //     1268, 
    //     CDT_DENOM.to_string(), 
    //     NOBLE_USDC_DENOM.to_string(), 
    //     Some(osmosis_std::shim::Timestamp {
    //         seconds:  start_time as i64,
    //         nanos: 0,
    //     }),
    // )?;

    // //Price in USDC
    // let asset_price_in_usdc = Decimal::from_str(&res.geometric_twap)?;
    // ////////

    // Ok(PriceResponse { 
    //     prices: vec![], 
    //     price: asset_price_in_usdc, 
    //     decimals: 6 })
}
