use std::str::FromStr;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128, Addr, StdResult};


use crate::{types::{NeutronOracleInfo, PriceInfo, TWAPPoolInfo}, math::{decimal_multiplication, decimal_division, Decimal256, Uint256}};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Positions contract address
    pub positions_contract: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// Positions contract address
        positions_contract: Option<String>,
    },
    /// Add a new asset
    AddAsset {
        /// Asset info
        asset_info: String,
        /// Asset's oracle info
        oracle_info: NeutronOracleInfo,
    },
    /// Edit an existing asset
    EditAsset {
        /// Asset info
        asset_info: String,
        /// Asset's oracle info. Replaces existing oracle info.
        oracle_info: Option<NeutronOracleInfo>,
        /// Toggle to remove
        remove: bool,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    /// Returns twap price
    Price {
        /// Asset info
        asset_info: String,
        /// Timeframe in minutes
        twap_timeframe: u64,
        /// Pyth Oracle time limit in seconds
        oracle_time_limit: u64,
        /// To switch on oracle sources.
        /// None defaults to 1, which is assumed the USD basket.
        basket_id: Option<Uint128>,
    },
    /// Returns twap prices
    Prices {
        /// Asset infos
        asset_infos: Vec<String>,
        /// Timeframe in minutes
        twap_timeframe: u64,
        /// (Pyth) Oracle time limit in seconds
        oracle_time_limit: u64,
    },
    /// Return list of asset oracle info
    Assets {
        /// List of asset infos
        asset_infos: Vec<String> 
    },
}


#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Positions contract address
    pub positions_contract: Option<Addr>,
}

#[cw_serde]
pub struct PriceResponse {
    /// List of PriceInfo from different sources
    pub prices: Vec<PriceInfo>,
    /// Median price
    pub price: Decimal,
    /// Asset decimals
    pub decimals: u64,
}

impl PriceResponse {
    pub fn get_value(&self, amount: Uint128) -> StdResult<Decimal> {
        //Normalize Asset amounts to fiat decimal amounts (1_000_000 = 1)
        let exponent_difference = self.decimals;

        let decimal_asset_amount = {
            Decimal::from_ratio(amount, Uint128::from(10u64.pow(exponent_difference as u32) as u128))
        };
        
        decimal_multiplication(self.price, decimal_asset_amount)
    }

    pub fn get_amount(&self, value: Decimal) -> StdResult<Uint128> {
        //Normalize Asset amounts to fiat decimal amounts (1_000_000 = 1)
        let exponent_difference = self.decimals;

        let pre_scaled_amount = decimal_division(value, self.price)?;

        //Post scaled amount where we add the asset's decimals (1 = 1_000_000)
        let scale = Decimal::from_ratio(
            Uint128::from(10u64.pow(exponent_difference as u32) as u128),
            Uint128::one(),
        );
        let asset_amount = decimal_multiplication(pre_scaled_amount, scale)?;

        Ok(asset_amount.to_uint_floor())
    }

    pub fn to_decimal256(&self) -> StdResult<PriceResponse256>{
        let price = Decimal256::from_str(&self.price.to_string())?;
        Ok(PriceResponse256 {
            prices: self.clone().prices,
            price,
            decimals: self.decimals,
        })
    }
}

#[cw_serde]
pub struct PriceResponse256 {
    /// List of PriceInfo from different sources
    pub prices: Vec<PriceInfo>,
    /// Median price
    pub price: Decimal256,
    /// Asset decimals
    pub decimals: u64,
}

impl PriceResponse256 {
    pub fn get_value(&self, amount: Uint256) -> Decimal256 {
        //Normalize Asset amounts to fiat decimal amounts (1_000_000 = 1)
        let exponent_difference = self.decimals;

        let decimal_asset_amount = {
            Decimal256::from_ratio(amount, Uint256::from(10u64.pow(exponent_difference as u32) as u128))
        };

        self.price * decimal_asset_amount
    }

    pub fn get_amount(&self, value: Decimal256) -> Uint256 {
        //Normalize Asset amounts to fiat decimal amounts (1_000_000 = 1)
        let exponent_difference = self.decimals;

        let pre_scaled_amount = value / self.price;

        //Post scaled amount where we add the asset's decimals (1 = 1_000_000)
        let asset_amount = pre_scaled_amount
            * Uint256::from(10u64.pow(exponent_difference as u32) as u128);

        asset_amount
    }
}

#[cw_serde]
pub struct AssetResponse {
    /// Asset info
    pub asset_info: String,
    /// Asset's list of oracle info
    pub oracle_info: Vec<NeutronOracleInfo>,
}

#[cw_serde]
pub struct MigrateMsg {}

//FROM: https://github.com/mars-protocol/core-contracts/blob/6af9a00dd322f3a4ecc6ebbc5808669b0de00a89/contracts/oracle/wasm/src/slinky.rs#L152

use cosmwasm_std::{Deps, Env, Int128, QuerierWrapper, StdError};
use neutron_sdk::bindings::{
    marketmap::query::{MarketMapQuery, MarketResponse},
    oracle::{
        query::{GetAllCurrencyPairsResponse, GetPriceResponse, OracleQuery},
        types::CurrencyPair,
    },
    query::NeutronQuery,
};

/// Denom for USD, used in price source to get USD price in uusd
pub const USD_DENOM: &str = "usd";

/// We don't support any denom with more than 18 decimals
pub const MAX_DENOM_DECIMALS: u8 = 18;

pub const SLINKY_QUOTE_CURRENCY: &str = "USD";

/// Maximum number of blocks that the price can be old.
/// The value is checked when setting up the price source.
pub const SLINKY_MAX_BLOCKS_OLD: u8 = 5;

pub trait CurrencyPairExt {
    fn key(&self) -> String;
}

impl CurrencyPairExt for CurrencyPair {
    /// Market key is a combination of base and quote currency symbols separated by a slash (e.g. BTC/USD).
    fn key(&self) -> String {
        format!("{}/{}", self.base, self.quote)
    }
}

/// Assert Slinky configuration
pub fn assert_slinky(
    deps: &Deps,
    base_symbol: &str,
    denom_decimals: u8,
    max_blocks_old: u8,
) -> StdResult<()> {
    if denom_decimals > MAX_DENOM_DECIMALS {
        return Err(StdError::generic_err(format!(
            "denom_decimals must be <= {}", MAX_DENOM_DECIMALS
        )));
    }

    if max_blocks_old > SLINKY_MAX_BLOCKS_OLD {
        return Err(StdError::generic_err(format!(
            "max_blocks_old must be <= {}", SLINKY_MAX_BLOCKS_OLD
        )));
    }

    let currency_pair: CurrencyPair = CurrencyPair {
        base: base_symbol.to_string(),
        quote: SLINKY_QUOTE_CURRENCY.to_string(),
    };

    let ntrn_querier = QuerierWrapper::<NeutronQuery>::new(&*deps.querier);
    assert_currency_pair_in_oracle_module(&ntrn_querier, &currency_pair)?;
    assert_currency_pair_in_market_module(&ntrn_querier, &currency_pair)?;

    Ok(())
}

/// Assert that the currency pair exists in the x/oracle module
fn assert_currency_pair_in_oracle_module(
    querier: &QuerierWrapper<NeutronQuery>,
    currency_pair: &CurrencyPair,
) -> StdResult<()> {
    // fetch all supported currency pairs in x/oracle module
    let oracle_currency_pairs_query: OracleQuery = OracleQuery::GetAllCurrencyPairs {};
    let oracle_currency_pairs_response: GetAllCurrencyPairsResponse =
        querier.query(&oracle_currency_pairs_query.into())?;
    if !oracle_currency_pairs_response.currency_pairs.contains(currency_pair) {
        return Err(StdError::generic_err(format!(
                "Slinky Market {}/{} not found in x/oracle module",
                currency_pair.base, currency_pair.quote
            )
        ));
    }

    Ok(())
}

/// Assert that the currency pair exists in the x/marketmap module and is enabled
fn assert_currency_pair_in_market_module(
    querier: &QuerierWrapper<NeutronQuery>,
    currency_pair: &CurrencyPair,
) -> StdResult<()> {
    // fetch currency pair from x/marketmap module
    let market_currency_pair_query: MarketMapQuery = MarketMapQuery::Market {
        currency_pair: currency_pair.clone(),
    };
    let market_currency_pair_response: MarketResponse = querier
        .query(&market_currency_pair_query.into())
        .map_err(|_| StdError::generic_err(format!(
            "Slinky Market {}/{} not found in x/marketmap module",
            currency_pair.base, currency_pair.quote
        )))?;
    if !market_currency_pair_response.market.ticker.enabled {
        return Err(StdError::generic_err(format!(
            "Slinky Market {}/{} not enabled in x/marketmap module",
            currency_pair.base, currency_pair.quote
        )));
    }

    Ok(())
}

pub fn query_slinky_price(
    deps: &Deps,
    env: &Env,
    base_symbol: &str,
    denom_decimals: u8,
    max_blocks_old: u8,
    usd_price: Decimal,
) -> StdResult<Decimal> {
    let ntrn_querier = QuerierWrapper::<NeutronQuery>::new(&*deps.querier);

    let currency_pair: CurrencyPair = CurrencyPair {
        base: base_symbol.to_string(),
        quote: SLINKY_QUOTE_CURRENCY.to_string(),
    };

    assert_currency_pair_in_market_module(&ntrn_querier, &currency_pair)?;

    // fetch price for currency_pair from x/oracle module
    let oracle_price_query: OracleQuery = OracleQuery::GetPrice {
        currency_pair: currency_pair.clone(),
    };
    let oracle_price_response: GetPriceResponse = ntrn_querier.query(&oracle_price_query.into())?;

    assert_oracle_price_validity(
        env.block.height,
        max_blocks_old,
        &currency_pair,
        &oracle_price_response,
    )?;

    // Use current price source for USD to check how much 1 USD is worth in base_denom
    // let usd_price = price_sources
    //     .load(deps.storage, USD_DENOM)
    //     .map_err(|_| StdError::generic_err("Price source not found for denom 'usd'"))?
    //     .query_price(deps, env, USD_DENOM, config, price_sources, kind.clone())?;

    let scaled_price = scale_slinky_price(
        oracle_price_response.price.price,
        oracle_price_response.decimals,
        denom_decimals,
        usd_price,
    )?;

    Ok(scaled_price)
}


// pub fn query_slinky_price<P: PriceSourceChecked<Empty>>(
//     deps: &Deps,
//     env: &Env,
//     config: &Config,
//     price_sources: &Map<&str, P>,
//     kind: ActionKind,
//     base_symbol: &str,
//     denom_decimals: u8,
//     max_blocks_old: u8,
// ) -> StdResult<Decimal> {
//     let ntrn_querier = QuerierWrapper::<NeutronQuery>::new(&*deps.querier);

//     let currency_pair: CurrencyPair = CurrencyPair {
//         base: base_symbol.to_string(),
//         quote: SLINKY_QUOTE_CURRENCY.to_string(),
//     };

//     assert_currency_pair_in_market_module(&ntrn_querier, &currency_pair)?;

//     // fetch price for currency_pair from x/oracle module
//     let oracle_price_query: OracleQuery = OracleQuery::GetPrice {
//         currency_pair: currency_pair.clone(),
//     };
//     let oracle_price_response: GetPriceResponse = ntrn_querier.query(&oracle_price_query.into())?;

//     assert_oracle_price_validity(
//         env.block.height,
//         max_blocks_old,
//         &currency_pair,
//         &oracle_price_response,
//     )?;

//     // Use current price source for USD to check how much 1 USD is worth in base_denom
//     let usd_price = price_sources
//         .load(deps.storage, USD_DENOM)
//         .map_err(|_| StdError::generic_err("Price source not found for denom 'usd'"))?
//         .query_price(deps, env, USD_DENOM, config, price_sources, kind.clone())?;

//     let scaled_price = scale_slinky_price(
//         oracle_price_response.price.price,
//         oracle_price_response.decimals,
//         denom_decimals,
//         usd_price,
//     )?;

//     Ok(scaled_price)
// }

/// Assert validity of the price from x/oracle module
fn assert_oracle_price_validity(
    current_block_height: u64,
    max_blocks_old: u8,
    currency_pair: &CurrencyPair,
    price_response: &GetPriceResponse,
) -> StdResult<()> {
    // check if block_height is not too old
    let price_block_height = price_response.price.block_height.ok_or_else(|| {
        StdError::generic_err("block_height is not available in Slinky OracleQuery response")
    })?;
    
    if (current_block_height - price_block_height) > max_blocks_old as u64 {
        return Err(StdError::generic_err(format!(
            "Slinky Market {}/{} price is older than {} blocks",
            currency_pair.base, currency_pair.quote, max_blocks_old
        )));
    }

    // make sure the price value is not None (i.e. has not been initialized)
    if price_response.nonce == 0 {
        return Err(StdError::generic_err(format!(
            "Slinky Market {}/{} price is nil",
            currency_pair.base, currency_pair.quote
        )));
    }

    Ok(())
}

/// We have to represent the price for utoken in base_denom.
/// Slinky price should be normalized with token decimals.
///
/// Let's try to convert BTC/USD reported by Slinky to ubtc/base_denom:
/// - base_denom = uusd
/// - price source set for usd (e.g. FIXED price source where 1 usd = 1000000 uusd = 10^6 uusd)
/// - denom_decimals (BTC) = 8
///
/// 1 BTC = 10^8 ubtc
///
/// 1 BTC = price * 10^(-slinky_decimals) USD
/// 10^8 ubtc = price * 10^(-slinky_decimals) * 10^6 uusd
/// ubtc = price * 10^(-slinky_decimals) * 10^6 / 10^8 uusd
/// ubtc = price * 10^(-slinky_decimals) * 10^6 * 10^(-8) uusd
/// ubtc/uusd = 6470160093122 * 10^(-8) * 10^6 * 10^(-8)
/// ubtc/uusd = 6470160093122 * 10^(-10) = 647.0160093122
///
/// Generalized formula:
/// utoken/uusd = price * 10^(-slinky_decimals) * usd_price_in_base_denom * 10^(-denom_decimals)
pub fn scale_slinky_price(
    slinky_value: Int128,
    slinky_decimals: u64,
    denom_decimals: u8,
    usd_price: Decimal,
) -> StdResult<Decimal> {
    // Slinky price should be above 0
    if slinky_value <= Int128::zero() {
        return Err(StdError::generic_err(
            "Slinky price should be greater than 0"
        ));
    }
    let value = slinky_value.unsigned_abs();

    // Slinky decimals should be 8 in most cases (see doc for `GetPriceResponse`).
    // This check is to prevent overflow in the calculation.
    let slinky_decimals =     if slinky_decimals > u8::MAX as u64 {
        return Err(StdError::generic_err(format!(
            "Slinky decimals {} too big (should be <= {})",
            slinky_decimals,
            u8::MAX
        )));
    } else {
        slinky_decimals as u8
    };

    // USD price is expected to be represented as: 10^decimals (it is validated when setting usd price source).
    // Example:
    // 1 USD = 10^6 uusd = 1000000 uusd
    // We subtract 1 from the length of the string representation of the price to get the number of decimals.
    let usd_decimals = usd_price.to_string().len() as u8 - 1;

    let decimal_places = usd_decimals as i32 - denom_decimals as i32 - slinky_decimals as i32;
    let price = if decimal_places <= 0 {
        Decimal::from_atomics(value, decimal_places.unsigned_abs())
            .map_err(|_| StdError::generic_err("Failed to create decimal from atomics"))?
    } else {
        // Impossible for current Slinky and USD price setup:
        // - Slinky price is 8 decimals
        // - USD price is 6 decimals
        // denom_decimals should be < -2 to get here but it is not possible.
        let target_expo = Uint128::from(10u8).checked_pow(decimal_places.unsigned_abs())
            .map_err(|_| StdError::generic_err("checked_pow overflow"))?;
        let res = value.checked_mul(target_expo)
            .map_err(|_| StdError::generic_err("checked_mul overflow"))?;
        Decimal::from_ratio(res, 1u128)
    };

    if price.is_zero() {
        return Err(StdError::generic_err(
            "price is zero"
        ));
    }

    Ok(price)
}

// #[cfg(test)]
// mod tests {
    // use std::str::FromStr;

    // use super::*;

    // #[test]
    // fn return_error_if_slinky_price_less_than_or_equal_to_zero() {
    //     let usd_price = Decimal::from_str("1000000").unwrap();

    //     // slinky price is 0
    //     let price_err = scale_slinky_price(Int128::zero(), 8, 6, usd_price).unwrap_err();
    //     assert_eq!(
    //         price_err,
    //         ContractError::InvalidPrice {
    //             reason: "Slinky price should be greater than 0".to_string(),
    //         }
    //     );

    //     // slinky price below 0
    //     let price_err = scale_slinky_price(Int128::from(-1), 8, 6, usd_price).unwrap_err();
    //     assert_eq!(
    //         price_err,
    //         ContractError::InvalidPrice {
    //             reason: "Slinky price should be greater than 0".to_string(),
    //         }
    //     );
    // }

    // #[test]
    // fn return_error_if_slinky_decimals_too_big() {
    //     let price_err =
    //         scale_slinky_price(Int128::one(), 256, 6, Decimal::from_str("1000000").unwrap())
    //             .unwrap_err();
    //     assert_eq!(
    //         price_err,
    //         ContractError::InvalidPrice {
    //             reason: "Slinky decimals 256 too big (should be <= 255)".to_string(),
    //         }
    //     );
    // }

    // #[test]
    // fn return_error_if_scaled_price_is_zero() {
    //     let price_err =
    //         scale_slinky_price(Int128::from(1), 18, 18, Decimal::from_str("1000000").unwrap())
    //             .unwrap_err();
    //     assert_eq!(
    //         price_err,
    //         ContractError::InvalidPrice {
    //             reason: "price is zero".to_string()
    //         }
    //     );
    // }

    // #[test]
    // fn scale_slinky_price_if_decimal_places_less_than_or_equal_to_zero() {
    //     let usd_price = Decimal::from_str("1000000").unwrap();

    //     // slinky ETH price with 6 decimals
    //     let ueth_price_in_uusd =
    //         scale_slinky_price(Int128::from(3486881068i128), 6, 18, usd_price).unwrap();
    //     let exptected_price = Decimal::from_atomics(3486881068u128, 18u32).unwrap();
    //     assert_eq!(ueth_price_in_uusd, exptected_price);
    //     assert_eq!(ueth_price_in_uusd.to_string(), "0.000000003486881068".to_string());

    //     // slinky ETH price with 8 decimals
    //     let ueth_price_in_uusd =
    //         scale_slinky_price(Int128::from(348688106812i128), 8, 18, usd_price).unwrap();
    //     let exptected_price = Decimal::from_atomics(348688106812u128, 20u32).unwrap();
    //     assert_eq!(ueth_price_in_uusd, exptected_price);
    //     assert_eq!(ueth_price_in_uusd.to_string(), "0.000000003486881068".to_string()); // lost 2 digits precision

    //     // slinky bigger ETH price with 8 decimals
    //     let ueth_price_in_uusd =
    //         scale_slinky_price(Int128::from(1248688106812i128), 8, 18, usd_price).unwrap();
    //     let exptected_price = Decimal::from_atomics(1248688106812u128, 20u32).unwrap();
    //     assert_eq!(ueth_price_in_uusd, exptected_price);
    //     assert_eq!(ueth_price_in_uusd.to_string(), "0.000000012486881068".to_string()); // lost 2 digits precision

    //     // slinky TIA price with 8 decimals
    //     let utia_price_in_uusd =
    //         scale_slinky_price(Int128::from(652586790i128), 8, 6, usd_price).unwrap();
    //     let exptected_price = Decimal::from_atomics(652586790u128, 8u32).unwrap();
    //     assert_eq!(utia_price_in_uusd, exptected_price);
    //     assert_eq!(utia_price_in_uusd.to_string(), "6.5258679".to_string());

    //     // slinky DYDX price with 8 decimals
    //     let udydx_price_in_uusd =
    //         scale_slinky_price(Int128::from(142437588i128), 8, 18, usd_price).unwrap();
    //     let exptected_price = Decimal::from_atomics(142437588u128, 20u32).unwrap();
    //     assert_eq!(udydx_price_in_uusd, exptected_price);
    //     assert_eq!(udydx_price_in_uusd.to_string(), "0.000000000001424375".to_string());
    //     // lost 2 digits precision
    // }

    // #[test]
    // fn scale_slinky_price_if_decimal_places_more_than_zero() {
    //     let usd_price = Decimal::from_str("10000000000").unwrap();

    //     let price_in_uusd = scale_slinky_price(Int128::from(15612i128), 2, 6, usd_price).unwrap();
    //     let exptected_price = Decimal::from_atomics(1561200u128, 0u32).unwrap();
    //     assert_eq!(price_in_uusd, exptected_price);
    //     assert_eq!(price_in_uusd.to_string(), "1561200".to_string());
    // }
// }