use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal, Env, QuerierWrapper, StdError, Uint128, Uint256, StdResult};

use crate::types::{NeutronOwner, TransmutationPairEntry, TransmutationPair};

#[cw_serde]
pub struct InstantiateMsg {}

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
}


#[cw_serde]
pub struct NeutronOwnerEntry {
    /// Owner
    pub owner: NeutronOwner,
    /// Remove
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
            return Err(StdError::GenericErr {
                msg: "the route must contain at least one pair".to_string(),
            });
        }

        // Ensure the first denom in the route is the input denom
        if swap_denoms.first() != Some(&denom_in.to_string()) {
            return Err(StdError::GenericErr {
                msg: format!(
                    "the route's first denom {} does not match the input denom {}",
                    swap_denoms.first().unwrap_or(&"none".to_string()),
                    denom_in
                ),
            });
        }

        // Ensure the last denom in the route is the output denom
        if swap_denoms.last() != Some(&denom_out.to_string()) {
            return Err(StdError::GenericErr {
                msg: format!(
                    "the route's last denom {} does not match the output denom {}",
                    swap_denoms.last().unwrap_or(&"none".to_string()),
                    denom_out
                ),
            });
        }

        // Check for loops - each denom should only appear once in the route
        let mut seen_denoms = hashset(&[]);
        for denom in swap_denoms.iter() {
            if seen_denoms.contains(denom) {
                return Err(StdError::GenericErr {
                    msg: format!("route contains a loop: denom {} seen twice", denom),
                });
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
            return Err(StdError::GenericErr {
                msg: "the route must contain at least two denoms".to_string(),
            });
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
                amount_in: coin_in.amount.to_string(),
                expiration_time: None,
                max_amount_out: None,
                limit_sell_price,
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
    stargate::{aux::create_stargate_msg, dex::{msg::msg_multi_hop_swap, types::{LimitOrderType, MultiHopSwapRequest, PlaceLimitOrderRequest}}},
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
        return Err(StdError::GenericErr {
            msg: "Empty input".to_string(),
        });
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

    #[test]
    fn test_serialize_prec_dec() {
        // Standard case with both integer and decimal parts
        assert_eq!(serialize_prec_dec("1.23").unwrap(), "1230000000000000000000000000");

        // Case with leading zero in integer part (the buggy case)
        assert_eq!(serialize_prec_dec("0.01").unwrap(), "10000000000000000000000000");

        // Case with only integer part
        assert_eq!(serialize_prec_dec("42").unwrap(), "42000000000000000000000000000");

        // Zero case
        assert_eq!(serialize_prec_dec("0.0").unwrap(), "0000000000000000000000000000");

        // Case with trailing zeros in fractional part
        assert_eq!(serialize_prec_dec("1.2300").unwrap(), "1230000000000000000000000000");

        // Case with long fractional part
        assert_eq!(serialize_prec_dec("0.000123").unwrap(), "123000000000000000000000");

        // Edge case: exactly 27 digits in fractional part
        assert_eq!(
            serialize_prec_dec("0.123456789012345678901234567").unwrap(),
            "123456789012345678901234567"
        );
    }
}

// Re-export NeutronMsg for use in contracts
pub use neutron_sdk::bindings::msg::NeutronMsg;