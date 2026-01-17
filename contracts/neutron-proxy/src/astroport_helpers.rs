use cosmwasm_std::{
    to_json_binary, Addr, Binary, Coin, CosmosMsg, Decimal, QuerierWrapper, Uint128, WasmMsg,
};
use membrane::neutron_proxy::{DexChoice, PclInitParams};
use membrane::types::AssetInfo;
use astroport::asset::Asset;
use astroport::pair::ExecuteMsg as PairExecuteMsg;
use astroport::factory::QueryMsg as FactoryQueryMsg;
use astroport::pair::QueryMsg as PairQueryMsg;
use astroport::pair::SimulationResponse;

use crate::error::TokenFactoryError;
use membrane::neutron_proxy::NeutronMsg;

/// Query expected output from Astroport pair
pub fn query_astroport_swap_output(
    querier: &QuerierWrapper,
    pair_addr: &Addr,
    coin_in: &Coin,
    asset_out: &AssetInfo,
) -> Result<Uint128, TokenFactoryError> {
    let asset_in = AssetInfo::NativeToken {
        denom: coin_in.denom.clone(),
    };

    let simulation: SimulationResponse = querier.query_wasm_smart(
        pair_addr,
        &PairQueryMsg::Simulation {
            offer_asset: Asset {
                info: asset_info_to_astroport(asset_in),
                amount: coin_in.amount,
            },
            ask_asset_info: Some(asset_info_to_astroport(asset_out.clone())),
        },
    )?;

    // Extract amount from return asset
    // return_amount is a Uint128 directly
    Ok(simulation.return_amount)
}

/// Resolve Astroport pair address via Factory
pub fn resolve_astroport_pair(
    querier: &QuerierWrapper,
    factory: &Addr,
    asset_infos: &[AssetInfo],
) -> Result<Addr, TokenFactoryError> {
    if asset_infos.len() != 2 {
        return Err(TokenFactoryError::InvalidRouteConfig {
            reason: "Pair must have exactly 2 assets".to_string(),
        });
    }

    // Canonicalize asset infos
    let mut canonical_assets = asset_infos.to_vec();
    canonicalize_asset_infos(&mut canonical_assets)?;

    // Convert to astroport AssetInfo
    let astroport_assets: Vec<astroport::asset::AssetInfo> = canonical_assets
        .iter()
        .map(|a| asset_info_to_astroport(a.clone()))
        .collect();

    // Query factory for pair - use the response type that's actually public
    #[derive(serde::Deserialize)]
    struct PairInfoResponse {
        pub contract_addr: Addr,
    }
    
    let pair_info: PairInfoResponse = querier.query_wasm_smart(
        factory,
        &FactoryQueryMsg::Pair {
            asset_infos: astroport_assets,
        },
    )?;

    Ok(pair_info.contract_addr)
}

/// Build Astroport swap message (native tokens only)
pub fn build_astroport_swap_msg(
    pair_addr: &Addr,
    coin_in: &Coin,
    asset_out: &AssetInfo,
    min_receive: Uint128,
    max_spread: Option<Decimal>,
    to: Option<Addr>,
) -> Result<CosmosMsg<NeutronMsg>, TokenFactoryError> {
    let asset_in = AssetInfo::NativeToken {
        denom: coin_in.denom.clone(),
    };

    let swap_msg = PairExecuteMsg::Swap {
        offer_asset: Asset {
            info: asset_info_to_astroport(asset_in),
            amount: coin_in.amount,
        },
        ask_asset_info: Some(asset_info_to_astroport(asset_out.clone())),
        belief_price: None,
        max_spread: max_spread,
        to: to.map(|a| a.to_string()),
    };

    Ok(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: pair_addr.to_string(),
        msg: to_json_binary(&swap_msg)?,
        funds: vec![coin_in.clone()],
    }))
}

/// Choose best DEX by comparing outputs
pub fn choose_best_dex(
    duality_output: Option<Uint128>,
    astroport_output: Option<Uint128>,
) -> Result<DexChoice, TokenFactoryError> {
    match (duality_output, astroport_output) {
        (Some(duality), Some(astroport)) => {
            if astroport > duality {
                Ok(DexChoice::Astroport)
            } else {
                Ok(DexChoice::Duality)
            }
        }
        (Some(_), None) => Ok(DexChoice::Duality),
        (None, Some(_)) => Ok(DexChoice::Astroport),
        (None, None) => Err(TokenFactoryError::NoLiquidityAvailable {}),
    }
}

/// Canonicalize asset infos for Factory queries
pub fn canonicalize_asset_infos(
    asset_infos: &mut [AssetInfo],
) -> Result<(), TokenFactoryError> {
    if asset_infos.len() != 2 {
        return Err(TokenFactoryError::InvalidRouteConfig {
            reason: "Pair must have exactly 2 assets".to_string(),
        });
    }

    // Check for duplicates
    if asset_infos[0].equal(&asset_infos[1]) {
        return Err(TokenFactoryError::DuplicateAssets {});
    }

    // Sort by string representation for canonical ordering
    let asset_str_0 = format!("{:?}", asset_infos[0]);
    let asset_str_1 = format!("{:?}", asset_infos[1]);

    if asset_str_0 > asset_str_1 {
        asset_infos.swap(0, 1);
    }

    Ok(())
}

/// Encode PCL init params to JSON binary
pub fn encode_pcl_init_params(
    params: &PclInitParams,
) -> Result<Binary, TokenFactoryError> {
    // Build init params as a JSON object
    use std::collections::HashMap;
    let mut init_params = HashMap::new();
    init_params.insert("amp".to_string(), params.amp.clone());
    init_params.insert("gamma".to_string(), params.gamma.clone());
    init_params.insert("mid_fee".to_string(), params.mid_fee.clone());
    init_params.insert("out_fee".to_string(), params.out_fee.clone());
    init_params.insert("fee_gamma".to_string(), params.fee_gamma.clone());
    init_params.insert("repeg_profit_threshold".to_string(), params.repeg_profit_threshold.clone());
    init_params.insert("min_price_scale_delta".to_string(), params.min_price_scale_delta.clone());
    init_params.insert("initial_price_scale".to_string(), params.initial_price_scale.clone());
    init_params.insert("ma_half_time".to_string(), params.ma_half_time.to_string());
    init_params.insert("owner".to_string(), params.owner.clone());

    to_json_binary(&init_params).map_err(|e| TokenFactoryError::Std(e.into()))
}

/// Validate PCL params
pub fn validate_pcl_params(
    params: &PclInitParams,
) -> Result<(), TokenFactoryError> {
    // Validate amp > 0
    let amp: Decimal = params.amp.parse().map_err(|_| {
        TokenFactoryError::InvalidPclParam {
            field: "amp".to_string(),
            message: "Invalid decimal format".to_string(),
        }
    })?;
    if amp <= Decimal::zero() {
        return Err(TokenFactoryError::InvalidPclParam {
            field: "amp".to_string(),
            message: "Must be greater than 0".to_string(),
        });
    }

    // Validate fees are between 0 and 1
    let mid_fee: Decimal = params.mid_fee.parse().map_err(|_| {
        TokenFactoryError::InvalidPclParam {
            field: "mid_fee".to_string(),
            message: "Invalid decimal format".to_string(),
        }
    })?;
    if mid_fee < Decimal::zero() || mid_fee >= Decimal::one() {
        return Err(TokenFactoryError::InvalidPclParam {
            field: "mid_fee".to_string(),
            message: "Must be between 0 and 1".to_string(),
        });
    }

    let out_fee: Decimal = params.out_fee.parse().map_err(|_| {
        TokenFactoryError::InvalidPclParam {
            field: "out_fee".to_string(),
            message: "Invalid decimal format".to_string(),
        }
    })?;
    if out_fee < Decimal::zero() || out_fee >= Decimal::one() {
        return Err(TokenFactoryError::InvalidPclParam {
            field: "out_fee".to_string(),
            message: "Must be between 0 and 1".to_string(),
        });
    }

    Ok(())
}

/// Convert membrane AssetInfo to astroport AssetInfo
pub fn asset_info_to_astroport(info: AssetInfo) -> astroport::asset::AssetInfo {
    match info {
        AssetInfo::NativeToken { denom } => astroport::asset::AssetInfo::NativeToken { denom },
        AssetInfo::Token { address } => astroport::asset::AssetInfo::Token {
            contract_addr: address,
        },
    }
}

