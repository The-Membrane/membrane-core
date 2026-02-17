use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal, Env, QuerierWrapper, StdError, Uint128, Uint256, StdResult};

use crate::types::{NeutronOwner, TransmutationPairEntry, TransmutationPair, VaultInfo, VaultEntry};

#[cw_serde]
pub struct InstantiateMsg {
    /// Optional transmuter contract address for USDC<>CDT swaps
    pub transmuter_contract: Option<String>,
    /// Optional list of vaults for automatic exit on swaps
    pub vaults: Option<Vec<VaultInfo>>,
    /// Optional Astroport Factory address
    pub astroport_factory: Option<String>,
    /// Optional Astroport Router address
    pub astroport_router: Option<String>,
    /// Enable dynamic routing (query both DEXes and choose best)
    pub enable_dynamic_routing: Option<bool>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Create a new native token denom
    CreateDenom {
        /// Subdenom of the token
        subdenom: String,        
        /// Max supply of the token.
        /// Enforced by the contract, not Osmosis.
        max_supply: Option<Uint128>,
    },
    /// Change the admin of a denom
    ChangeAdmin {
        /// Native token denom
        denom: String,
        /// New admin address
        new_admin_address: String,
    },
    /// Edit owner
    EditOwner {
        /// Owner address
        owner: String,
        /// non-token contract auth
        non_token_contract_auth: bool,
        /// Remove
        remove: bool,
    },
    /// Mint tokens of a denom owned by the contract
    MintTokens {
        /// Native token denom
        denom: String,
        /// Amount to mint
        amount: Uint128,
        /// Mint to address
        mint_to_address: String,
    },
    /// Burn tokens
    BurnTokens {
        /// Native token denom
        denom: String,
        /// Amount to burn
        amount: Uint128,
        /// Burn from address
        burn_from_address: String,
    },
    /// Edit the max supply of a denom
    EditTokenMaxSupply {
        /// Native token denom
        denom: String,
        /// New max supply
        max_supply: Uint128,
    },
    /// Execute Swaps
    ExecuteSwaps {
        /// Token out
        token_out: String,
        /// Max slippage
        max_slippage: Decimal,
    },
    /// Exchange Osmosis tokens for Neutron tokens
    /// This is a necessity for the Debt Auction to have the ability to mint & the liquidity for MBRN to be sold
    TransmuteTokens { },
    /// Update contract config
    UpdateConfig {
        /// List of owners
        owners: Option<Vec<NeutronOwnerEntry>>,
        /// List of transmutation pairs
        /// If you want A <> B, you need to add A:B and B:A
        /// 
        transmutation_pairs: Option<Vec<TransmutationPairEntry>>,
        /// Debt auction contract address
        debt_auction: Option<String>,
        /// Transmuter contract for USDC<>CDT swaps
        transmuter_contract: Option<String>,
        /// List of vaults for automatic exit on swaps
        vaults: Option<Vec<VaultEntry>>,
        /// Astroport Factory address
        astroport_factory: Option<String>,
        /// Astroport Router address
        astroport_router: Option<String>,
        /// Enable dynamic routing
        enable_dynamic_routing: Option<bool>,
        /// Supply thresholds for transmuting (minimum supply required before transmuting is enabled)
        transmute_supply_thresholds: Option<Vec<TransmuteSupplyThresholdEntry>>,
        /// Vesting contract address
        vesting_contract: Option<String>,
        /// Vesting period for transmutations
        vesting_period: Option<crate::types::VestingPeriod>,
    },
    /// Create Astroport PCL (concentrated liquidity) pair
    // CreatePclPair {
    //     /// Asset infos for the pair (must be exactly 2)
    //     asset_infos: Vec<crate::types::AssetInfo>,
    //     /// PCL initialization parameters
    //     params: PclInitParams,
    // },
    /// Update swap route configuration for a pair
    UpdateSwapRoute {
        /// Route entry
        route: SwapRouteEntry,
    },
    /// Batch update swap route configurations
    UpdateSwapRoutes {
        /// Route entries
        routes: Vec<SwapRouteEntry>,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Return contract config
    Config {},
    /// Return GetDenomResponse
    GetDenom {
        /// Denom creator address
        creator_address: String,
        /// Subdenom of the token
        subdenom: String,
    },
    /// Return list of denoms owned by the contract
    GetContractDenoms {
        /// Response limit
        limit: Option<u32>,
    },
    /// Return TokenInfoResponse
    GetTokenInfo {
        /// Native token denom
        denom: String,
    },
    /// Return Owner
    GetOwner {
        /// Owner address
        owner: String
    },
    /// Return list of swap routes
    GetSwapRoutes { },
    /// Get swap route configuration for a pair
    GetSwapRouteConfig {
        /// Token in
        token_in: String,
        /// Token out
        token_out: String,
    },
    /// Simulate swap on both DEXes and return outputs
    SimulateSwap {
        /// Token in
        token_in: String,
        /// Token out
        token_out: String,
        /// Amount in
        amount_in: Uint128,
    },
    /// Get Astroport pair info
    AstroportPairInfo {
        /// Asset infos for the pair
        asset_infos: Vec<crate::types::AssetInfo>,
    },
}

#[cw_serde]
pub struct GetDenomResponse {
    /// Token full denom
    pub denom: String,
}

#[cw_serde]
pub struct OwnerResponse {
    /// Owner object
    pub owner: NeutronOwner,
    /// Liquidity multiplier for debt token token minting caps
    pub liquidity_multiplier: Decimal,
}

#[cw_serde]
pub struct TokenInfoResponse {
    /// Token full denom
    pub denom: String,
    /// Current supply
    pub current_supply: Uint128,
    /// Max supply
    pub max_supply: Uint128,
    /// Burned supply
    pub burned_supply: Uint128,
}

#[cw_serde]
pub struct ContractDenomsResponse {
    /// List of denoms owned by the contract
    pub denoms: Vec<String>,
}

#[cw_serde]
pub struct Config {
    /// List of owners
    pub owners: Vec<NeutronOwner>,
    /// Debt auction contract address
    pub debt_auction: Option<Addr>,
    /// List of valid transmutation pairs
    pub transmutation_pairs: Vec<TransmutationPair>,
    /// Transmuter contract for USDC<>CDT swaps
    pub transmuter_contract: Option<Addr>,
    /// CDT denom from transmuter
    pub cdt_denom: Option<String>,
    /// USDC denom from transmuter
    pub usdc_denom: Option<String>,
    /// List of vaults for automatic exit on swaps
    pub vaults: Vec<VaultInfo>,
    /// Astroport Factory address
    pub astroport_factory: Option<Addr>,
    /// Astroport Router address
    pub astroport_router: Option<Addr>,
    /// Enable dynamic routing (query both DEXes and choose best)
    pub enable_dynamic_routing: bool,
    /// Vesting contract address for post-threshold transmutations
    pub vesting_contract: Option<Addr>,
    /// Default vesting period (180 day cliff, 180 day linear)
    pub vesting_period: Option<crate::types::VestingPeriod>,
}


#[cw_serde]
pub struct NeutronOwnerEntry {
    /// Owner
    pub owner: NeutronOwner,
    /// Remove
    pub remove: bool,

}

#[cw_serde]
pub struct TransmuteSupplyThresholdEntry {
    /// Token denom
    pub denom: String,
    /// Supply threshold (None to remove)
    pub threshold: Option<Uint128>,
    /// Remove flag
    pub remove: bool,
}
//Taken from https://github.com/mars-protocol/core-contracts/blob/6af9a00dd322f3a4ecc6ebbc5808669b0de00a89/packages/types/src/swapper.rs#L8
#[cw_serde]
pub struct DualityRoute {
    /// Entry denom, in other words the asset we are selling
    pub from: String,
    /// Exit denom, in other words the asset we are buying
    pub to: String,
    /// The denoms to swap through. For example,
    /// For a single swap route e.g A:B, the swap_denoms are [A, B]
    /// For a multi-swap route e.g A:B, B:C, C:D, the swap_denoms are [A, B, C]
    pub swap_denoms: Vec<String>,
}


impl DualityRoute {
    pub fn validate(
        &self,
        _querier: &QuerierWrapper,
        denom_in: &str,
        denom_out: &str,
    ) -> StdResult<()> {
        let swap_denoms = &self.swap_denoms;

        // There must be at least two denoms in the route
        if swap_denoms.len() < 2 {
            return Err(StdError::generic_err(
                "the route must contain at least one pair".to_string(),
            ));
        }

        // Ensure the first denom in the route is the input denom
        if swap_denoms.first() != Some(&denom_in.to_string()) {
            return Err(StdError::generic_err(format!(
                "the route's first denom {} does not match the input denom {}",
                swap_denoms.first().unwrap_or(&"none".to_string()),
                denom_in
            )));
        }

        // Ensure the last denom in the route is the output denom
        if swap_denoms.last() != Some(&denom_out.to_string()) {
            return Err(StdError::generic_err(format!(
                "the route's last denom {} does not match the output denom {}",
                swap_denoms.last().unwrap_or(&"none".to_string()),
                denom_out
            )));
        }

        // Check for loops - each denom should only appear once in the route
        let mut seen_denoms = hashset(&[]);
        for denom in swap_denoms.iter() {
            if seen_denoms.contains(denom) {
                return Err(StdError::generic_err(format!(
                    "route contains a loop: denom {} seen twice",
                    denom
                )));
            }
            seen_denoms.insert(denom.to_string());
        }

        Ok(())
    }

    pub fn build_exact_in_swap_msg(
        &self,
        _querier: &QuerierWrapper,
        env: &Env,
        coin_in: &Coin,
        min_receive: Uint128,
    ) -> StdResult<CosmosMsg<NeutronMsgType>> {
        let swap_denoms = &self.swap_denoms;

        if swap_denoms.len() < 2 {
            return Err(StdError::generic_err(
                "the route must contain at least two denoms".to_string(),
            ));
        }

        // If we have more than two denoms, we need to do a multi-hop swap.
        let swap_msg: CosmosMsg<NeutronMsgType> = if swap_denoms.len() > 2 {
            // PrecDec (neutrons decimal implementation) uses fixed-point precision of 27 decimal places.
            let exponent = Uint256::from(10u128.pow(27));
            let min_receive_scaled = Uint256::from(min_receive).checked_mul(exponent)?;

            // Our limit sell price is the worst price we are willing to accept.
            // Note that MultiHopSwapRequest msg requires the raw integer string value of the price, not a decimal string.
            // This means that 1.0 will be 1^27 (1000000000000000000000000000)
            let exit_limit_price = min_receive_scaled.checked_div(coin_in.amount.into())?;
            msg_multi_hop_swap(MultiHopSwapRequest {
                sender: env.contract.address.to_string(),
                receiver: env.contract.address.to_string(),
                routes: vec![swap_denoms.clone()],
                amount_in: coin_in.amount.to_string(),
                exit_limit_price: exit_limit_price.to_string(),
                pick_best_route: true,
            })
        } else {
            // The PlaceLimitOrderRequest msg requires the decimal, not the integer value.
            let limit_sell_price = Decimal::from_ratio(min_receive, coin_in.amount).to_string();

            msg_place_limit_order(PlaceLimitOrderRequest {
                order_type: LimitOrderType::FillOrKill,
                sender: env.contract.address.to_string(),
                receiver: env.contract.address.to_string(),
                token_in: coin_in.denom.to_string(),
                token_out: self.to.to_string(),
                // tick_index_in_to_out is depreciated in favor of limit_sell_price
                tick_index_in_to_out: 0,
                limit_sell_price,
                amount_in: coin_in.amount.to_string(),
                expiration_time: None,
                max_amount_out: None,
            })?
        };

        Ok(swap_msg)
    }
}
////
/// 


#[cw_serde]
pub struct MigrateMsg {}

/// helpers
/// taken from https://github.com/mars-protocol/core-contracts/blob/master/contracts/swapper/duality/src/helpers.rs#L36
use std::{collections::HashSet, hash::Hash};

use cosmwasm_std::CosmosMsg;
use neutron_sdk::{
    bindings::msg::NeutronMsg as NeutronMsgType,
    proto_types::neutron::dex::MsgPlaceLimitOrder,
    stargate::{
        aux::create_stargate_msg,
        dex::{
            msg::msg_multi_hop_swap,
            types::{LimitOrderType, MultiHopSwapRequest, PlaceLimitOrderRequest},
        },
    },
};

// Precision of the decimal values used in the Neutron DEX
// They use 27 decimal places for their PrecDec type
const PREC_DEC_PRECISION: usize = 27;

// Fully qualified protobuf type URL for the Neutron DEX limit order message.
// This path is defined in the Neutron proto files, https://github.com/neutron-org/neutron/blob/main/proto/neutron/dex/tx.proto#L135.
const PLACE_LIMIT_ORDER_MSG_PATH: &str = "/neutron.dex.MsgPlaceLimitOrder";

/// Build a hashset from array data
pub(crate) fn hashset<T: Eq + Clone + Hash>(data: &[T]) -> HashSet<T> {
    data.iter().cloned().collect()
}

/// Creates a Cosmos message for placing a limit order on the Neutron DEX.
///
/// This function wraps the MsgPlaceLimitOrder in a CosmosMsg<NeutronMsg> that can be directly
/// returned from contract execution. It uses our custom serialization for PrecDec
/// values to ensure proper handling of decimal prices.
///
/// # Arguments
///
/// * `req` - The PlaceLimitOrderRequest containing all parameters for the limit order
///
/// # Returns
///
/// A CosmosMsg<NeutronMsgType> that can be included in the response of a contract execution
pub(crate) fn msg_place_limit_order(
    req: PlaceLimitOrderRequest,
) -> StdResult<CosmosMsg<NeutronMsgType>> {
    Ok(create_stargate_msg(PLACE_LIMIT_ORDER_MSG_PATH, from(req)?))
}

/// Converts a PlaceLimitOrderRequest into a MsgPlaceLimitOrder with proper price serialization.
///
/// This function exists primarily to intercept and fix the serialization of the limit_sell_price
/// field, ensuring that decimal values are properly formatted for the Neutron chain. Without this
/// conversion, prices with leading zeros (like "0.01") would fail to serialize correctly.
///
/// # Arguments
///
/// * `v` - The PlaceLimitOrderRequest to convert
///
/// # Returns
///
/// A properly formatted MsgPlaceLimitOrder with correct PrecDec serialization
fn from(v: PlaceLimitOrderRequest) -> StdResult<MsgPlaceLimitOrder> {
    let price = v.limit_sell_price.clone();
    let mut msg = MsgPlaceLimitOrder::from(v);
    msg.limit_sell_price = serialize_prec_dec(&price)?;
    Ok(msg)
}

/// Serializes a decimal string into the format expected by PrecDec in the Neutron SDK.
///
/// This custom implementation fixes a bug in the standard PrecDec serialization that
/// fails when handling decimal values with a zero integer part (e.g. "0.09999").
/// The issue occurs because the standard implementation incorrectly preserves leading zeros
/// after conversion, resulting in invalid strings like "09999..." that are rejected by big.Int.
///
/// This implementation properly handles leading zeros and maintains the required
/// fixed-point precision of 27 decimal places used by the PrecDec type.
///
/// # Arguments
///
/// * `decimal_str` - A decimal value as a string (e.g. "1.23", "0.01")
///
/// # Returns
///
/// A string representation of the decimal as a fixed-point integer with the leading
/// zeros properly removed, ready for PrecDec serialization.
fn serialize_prec_dec(decimal_str: &str) -> StdResult<String> {
    // Basic validation
    if decimal_str.is_empty() {
        return Err(StdError::generic_err("Empty input".to_string()));
    }

    // Split into parts
    let (integer_part, fractional_part) = match decimal_str.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (decimal_str, ""),
    };

    // Remove leading zeros from integer part, keep at least one "0"
    let integer_clean = integer_part.trim_start_matches('0');
    let integer_clean = if integer_clean.is_empty() {
        "0"
    } else {
        integer_clean
    };

    // Remove trailing zeros from fractional part
    let fractional_clean = fractional_part.trim_end_matches('0');

    // Handle fractional part that's too long
    let fractional_to_use = if fractional_clean.len() > PREC_DEC_PRECISION {
        &fractional_clean[..PREC_DEC_PRECISION]
    } else {
        fractional_clean
    };

    // Build result efficiently
    let mut result = String::with_capacity(integer_clean.len() + PREC_DEC_PRECISION);

    // Special case for zero
    if integer_clean == "0" && fractional_to_use.is_empty() {
        result.push('0');
        result.push_str(&"0".repeat(PREC_DEC_PRECISION));
        return Ok(result);
    }

    // Build result
    result.push_str(integer_clean);
    result.push_str(fractional_to_use);

    // Add missing zeros
    let zeros_to_add = PREC_DEC_PRECISION.saturating_sub(fractional_to_use.len());
    result.push_str(&"0".repeat(zeros_to_add));

    // Remove leading zeros from result (keep at least one)
    let final_result = result.trim_start_matches('0');
    Ok(if final_result.is_empty() {
        "0".to_string()
    } else {
        final_result.to_string()
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hashset() {
        let data = vec![1, 2, 3, 4, 5];
        let set = hashset(&data);
        assert_eq!(set.len(), 5);
        assert!(set.contains(&1));
        assert!(set.contains(&2));
        assert!(set.contains(&3));
        assert!(set.contains(&4));
        assert!(set.contains(&5));
    }

}

// Re-export NeutronMsg for use in contracts
pub use neutron_sdk::bindings::msg::NeutronMsg;

/// Route configuration types
#[cw_serde]
pub enum DexPreference {
    /// Always use Neutron DEX (Duality)
    Duality,
    /// Always use Astroport
    Astroport,
    /// Query both and choose best output
    Best,
    /// Multi-hop with per-hop DEX selection
    MultiHop(Vec<HopConfig>),
}

#[cw_serde]
pub struct HopConfig {
    /// Intermediate token for this hop
    pub intermediate_token: String,
    /// DEX choice for this hop
    pub dex: DexChoice,
}

#[cw_serde]
pub enum DexChoice {
    /// Use Duality DEX
    Duality,
    /// Use Astroport DEX
    Astroport,
}

#[cw_serde]
pub struct SwapRouteEntry {
    /// Token in
    pub token_in: String,
    /// Token out
    pub token_out: String,
    /// Route preference
    pub preference: DexPreference,
    /// Remove this route configuration
    pub remove: bool,
}

/// PCL initialization parameters
#[cw_serde]
pub struct PclInitParams {
    /// Amplification parameter
    pub amp: String,
    /// Gamma parameter
    pub gamma: String,
    /// Mid fee
    pub mid_fee: String,
    /// Out fee
    pub out_fee: String,
    /// Fee gamma
    pub fee_gamma: String,
    /// Repeg profit threshold
    pub repeg_profit_threshold: String,
    /// Min price scale delta
    pub min_price_scale_delta: String,
    /// Initial price scale
    pub initial_price_scale: String,
    /// Moving average half time
    pub ma_half_time: u64,
    /// Owner address
    pub owner: String,
}

/// Query response types
#[cw_serde]
pub struct SwapRouteConfigResponse {
    /// Route preference (None if not configured)
    pub preference: Option<DexPreference>,
}

#[cw_serde]
pub struct SimulateSwapResponse {
    /// Expected output from Duality (None if not available)
    pub duality_output: Option<Uint128>,
    /// Expected output from Astroport (None if not available)
    pub astroport_output: Option<Uint128>,
    /// Best DEX choice based on outputs
    pub best_dex: Option<DexChoice>,
    /// Expected output for multi-hop route (if configured)
    pub multihop_output: Option<Uint128>,
    /// Per-hop breakdown (if multi-hop)
    pub hop_breakdown: Option<Vec<HopSimulation>>,
}

#[cw_serde]
pub struct HopSimulation {
    /// DEX used for this hop
    pub dex: DexChoice,
    /// Input token for this hop
    pub token_in: String,
    /// Output token for this hop
    pub token_out: String,
    /// Input amount for this hop
    pub amount_in: Uint128,
    /// Expected output amount for this hop
    pub amount_out: Uint128,
}

#[cw_serde]
pub struct AstroportPairInfoResponse {
    /// Pair contract address
    pub pair_addr: String,
    /// LP token address
    pub lp_token: String,
    /// Pair type
    pub pair_type: String,
}