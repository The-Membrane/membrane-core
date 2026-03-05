use cosmwasm_std::{Decimal, Uint128, Env, QuerierWrapper, Storage, StdResult, StdError, Addr};

use membrane::cdp::Config;
use membrane::stability_pool::QueryMsg as SP_QueryMsg;
use membrane::types::{Basket, Asset, cAsset, SupplyCap, AssetPool};
use membrane::helpers::{get_asset_liquidity, get_owner_liquidity_multiplier, get_stability_pool_liquidity};
use membrane::math::decimal_multiplication; 

use crate::rates::transform_caps_based_on_volatility;
use crate::state::{CONFIG, BASKET};
use crate::query::{get_cAsset_ratios, get_cAsset_ratios_imut};
use crate::error::ContractError;

/// Asserts that the assets provided are valid collateral assets in the basket
pub fn assert_basket_assets(
    storage: &mut dyn Storage,
    _querier: QuerierWrapper,
    _env: Env,
    assets: Vec<Asset>,
) -> Result<Vec<cAsset>, ContractError> {
    let basket: Basket = BASKET.load(storage)?;

    //Checking if Assets for the position are available collateral assets in the basket
    let collateral_assets = assets
        .into_iter()
        .map(|asset| {
            let cAsset = basket
                .collateral_types
                .iter()
                .find(|cAsset| cAsset.asset.info.equal(&asset.info))
                .ok_or(ContractError::InvalidCollateral {})?;
            Ok(cAsset {
                asset: asset.clone(),
                ..cAsset.clone()
            })
        })
        .collect::<Result<Vec<cAsset>, ContractError>>()?;   

    Ok(collateral_assets)
}

/// Update SupplyCap objects in Basket
pub fn update_basket_tally(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    collateral_assets: Vec<cAsset>,
    full_positions_assets: Vec<cAsset>,
    add_to_cAsset: bool,
    config: Config,
    from_liquidation: bool,
) -> Result<(), ContractError> {

    // WITHDRAWALS: Check pre-tally-update ratios to enforce over-cap withdrawal rules
    if !add_to_cAsset && !from_liquidation {
        let pre_supply_caps = match transform_caps_based_on_volatility(storage, basket.clone()){
            Ok(supply_caps) => supply_caps,
            Err(_err) => basket.clone().collateral_supply_caps
        };
        let (pre_ratios, _) =
            get_cAsset_ratios(storage, env.clone(), querier, basket.clone().collateral_types, config.clone(), Some(basket.clone()))?;

        // Collect over-cap assets that are still in the position after withdrawal
        let mut overcap_remaining: Vec<String> = vec![];
        for (i, ratio) in pre_ratios.iter().enumerate() {
            if !pre_supply_caps[i].supply_cap_ratio.is_zero()
                && *ratio > pre_supply_caps[i].supply_cap_ratio
                && full_positions_assets.iter().any(|a| a.asset.info.equal(&pre_supply_caps[i].asset_info))
            {
                overcap_remaining.push(pre_supply_caps[i].asset_info.to_string());
            }
        }

        // If over-cap assets remain AND user is withdrawing non-over-cap assets, error.
        // Withdrawing only over-cap assets (even partially) is always allowed.
        if !overcap_remaining.is_empty() {
            let withdrawing_non_overcap = collateral_assets.iter().any(|withdrawn| {
                // Check if this withdrawn asset is NOT an over-cap asset
                !pre_ratios.iter().enumerate().any(|(i, ratio)| {
                    pre_supply_caps[i].asset_info.equal(&withdrawn.asset.info)
                        && !pre_supply_caps[i].supply_cap_ratio.is_zero()
                        && *ratio > pre_supply_caps[i].supply_cap_ratio
                })
            });
            if withdrawing_non_overcap {
                return Err(ContractError::CustomError {
                    val: format!(
                        "Assets [{}] are over supply cap and must be fully withdrawn before other withdrawals",
                        overcap_remaining.join(", ")
                    ),
                });
            }
        }

        // Multi-asset caps: same logic
        if basket.multi_asset_supply_caps != vec![] {
            for multi_asset_cap in basket.clone().multi_asset_supply_caps {
                let mut total_ratio = Decimal::zero();
                for asset in &multi_asset_cap.assets {
                    if let Some((i, _cap)) = basket.collateral_supply_caps.iter().enumerate().find(|(_, cap)| cap.asset_info.equal(asset)) {
                        total_ratio += pre_ratios[i];
                    }
                }
                if total_ratio > multi_asset_cap.supply_cap_ratio {
                    // Check if any grouped assets remain in position after withdrawal
                    let remaining_grouped: Vec<String> = multi_asset_cap.assets.iter()
                        .filter(|asset| full_positions_assets.iter().any(|a| a.asset.info.equal(asset)))
                        .map(|asset| asset.to_string())
                        .collect();
                    if !remaining_grouped.is_empty() {
                        let withdrawing_non_grouped = collateral_assets.iter().any(|withdrawn| {
                            !multi_asset_cap.assets.iter().any(|asset| asset.equal(&withdrawn.asset.info))
                        });
                        if withdrawing_non_grouped {
                            return Err(ContractError::CustomError {
                                val: format!(
                                    "Multi-asset supply cap for [{}] is over the limit ({} > {}) - grouped assets must be fully withdrawn before other withdrawals",
                                    remaining_grouped.join(", "),
                                    total_ratio,
                                    multi_asset_cap.supply_cap_ratio,
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    //Update SupplyCap objects
    for cAsset in collateral_assets.clone() {
        if let Some((index, mut cap)) = basket.clone().collateral_supply_caps
            .into_iter()
            .enumerate()
            .find(|(_x, cap)| cap.asset_info.equal(&cAsset.asset.info))
        {
            if add_to_cAsset {
                cap.current_supply += cAsset.asset.amount;
            } else {
                cap.current_supply = match cap.current_supply.checked_sub(cAsset.asset.amount){
                    Ok(diff) => diff,
                    Err(_) => Uint128::zero(),
                };
            }

            //Update
            basket.collateral_supply_caps[index] = cap.clone();
            basket.collateral_types[index].asset.amount = cap.current_supply;
        }
    }

    // DEPOSITS/MINTS: Check post-tally-update ratios
    if add_to_cAsset && !from_liquidation {
        let supply_caps = match transform_caps_based_on_volatility(storage, basket.clone()){
            Ok(supply_caps) => supply_caps,
            Err(_err) => basket.clone().collateral_supply_caps
        };
        let (new_basket_ratios, _) =
            get_cAsset_ratios(storage, env, querier, basket.clone().collateral_types, config, Some(basket.clone()))?;

        //Assert new ratios aren't above Collateral Supply Caps
        for (i, ratio) in new_basket_ratios.clone().into_iter().enumerate() {
            //Check if the depositing/minting assets are part of this cap
            let in_position = collateral_assets.iter().any(|c| c.asset.info.equal(&supply_caps[i].asset_info));

            //We skip the check if the supply cap is zero bc those are expunged assets.
            if basket.collateral_supply_caps != vec![] && ratio > supply_caps[i].supply_cap_ratio && in_position && !supply_caps[i].supply_cap_ratio.is_zero(){
                return Err(ContractError::CustomError {
                    val: format!(
                        "Supply cap ratio for {} is over the limit ({} > {})",
                        basket.collateral_supply_caps[i].asset_info,
                        ratio,
                        supply_caps[i].supply_cap_ratio
                    ),
                });
            }
        }

        //Assert for Multi-asset caps as well
        if basket.multi_asset_supply_caps != vec![]{
            for multi_asset_cap in basket.clone().multi_asset_supply_caps {
                let mut total_ratio = Decimal::zero();
                let mut in_position = false;

                for asset in multi_asset_cap.clone().assets {
                    if let Some((i, _cap)) = basket.clone().collateral_supply_caps.into_iter().enumerate().find(|(_i, cap)| cap.asset_info.equal(&asset)){
                        total_ratio += new_basket_ratios[i];
                    }
                    if collateral_assets.iter().any(|c| c.asset.info.equal(&asset)){
                        in_position = true;
                    }
                }

                if total_ratio > multi_asset_cap.supply_cap_ratio && in_position {
                    return Err(ContractError::CustomError {
                        val: format!(
                            "Multi-Asset supply cap ratio for {:?} is over the limit ({} > {})",
                            multi_asset_cap.assets,
                            total_ratio,
                            multi_asset_cap.supply_cap_ratio,
                        ),
                    });
                }
            }
        }
    }

    Ok(())
}