use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128, Addr, StdResult};

use pyth_sdk_cw::PriceIdentifier;

use crate::{types::{AssetInfo, AssetOracleInfo, PriceInfo, TWAPPoolInfo}, math::{decimal_multiplication, decimal_division}};

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner, defaults to info.sender
    pub owner: Option<String>,
    /// Positions contract address
    pub positions_contract: Option<String>,
    /// Osmosis Proxy contract address
    pub osmosis_proxy_contract: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Update contract config
    UpdateConfig {
        /// Contract owner
        owner: Option<String>,
        /// Positions contract address
        positions_contract: Option<String>,
        /// Osmosis Proxy contract address
        osmosis_proxy_contract: Option<String>,
        /// OSMO/USD Pyth price feed id
        osmo_usd_pyth_feed_id: Option<PriceIdentifier>,
        /// Pyth Osmosis address
        pyth_osmosis_address: Option<String>,
        /// Osmosis pools for OSMO/USD-par TWAP.
        /// Replaces saved state.
        pools_for_usd_par_twap: Option<Vec<TWAPPoolInfo>>,
    },
    /// Add a new asset
    AddAsset {
        /// Asset info
        asset_info: AssetInfo,
        /// Asset's oracle info
        oracle_info: AssetOracleInfo,
    },
    /// Edit an existing asset
    EditAsset {
        /// Asset info
        asset_info: AssetInfo,
        /// Asset's oracle info. Replaces existing oracle info.
        oracle_info: Option<AssetOracleInfo>,
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
        asset_info: AssetInfo,
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
        asset_infos: Vec<AssetInfo>,
        /// Timeframe in minutes
        twap_timeframe: u64,
        /// Pyth Oracle time limit in seconds
        oracle_time_limit: u64,
    },
    /// Return list of asset oracle info
    Assets {
        /// List of asset infos
        asset_infos: Vec<AssetInfo> 
    },
}


#[cw_serde]
pub struct Config {
    /// Contract owner
    pub owner: Addr,
    /// Positions contract address
    /// Can edit asset & config
    pub positions_contract: Option<Addr>,
    /// Osmosis Proxy contract address
    /// Used to check for removed assets in Positions Owners
    pub osmosis_proxy_contract: Option<Addr>,
    /// OSMO/USD Pyth price feed id
    pub osmo_usd_pyth_feed_id: PriceIdentifier,
    /// Pyth Osmosis address
    pub pyth_osmosis_address: Option<Addr>,
    /// Osmosis pools for OSMO/USD-par TWAP.
    /// This list of pools will be used separately and medianized.
    pub pools_for_usd_par_twap: Vec<TWAPPoolInfo>,
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
        //Normalize Asset amounts to native token decimal amounts (6 places: 1 = 1_000_000)
        let exponent_difference = self
            .decimals
            .checked_sub(6u64)
            .unwrap();
        let asset_amount = amount
            / Uint128::new(10u64.pow(exponent_difference as u32) as u128);
        let decimal_asset_amount =
            Decimal::from_ratio(asset_amount, Uint128::new(1u128));

        decimal_multiplication(self.price, decimal_asset_amount)
    }

    pub fn get_amount(&self, value: Decimal) -> StdResult<Uint128> {
        //Normalize Asset amounts to native token decimal amounts (6 places: 1 = 1_000_000)
        let exponent_difference = self
            .decimals
            .checked_sub(6u64)
            .unwrap();

        //This gives us the amount if the asset had 6 decimals
        let pre_scaled_amount = decimal_division(value, self.price)?;

        let asset_amount = pre_scaled_amount
            * Uint128::new(10u64.pow(exponent_difference as u32) as u128);

        Ok(asset_amount)

    }
}

#[cw_serde]
pub struct AssetResponse {
    /// Asset info
    pub asset_info: AssetInfo,
    /// Asset's list of oracle info
    pub oracle_info: Vec<AssetOracleInfo>,
}
