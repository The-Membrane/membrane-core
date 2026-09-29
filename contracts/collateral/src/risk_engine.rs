#![allow(non_snake_case)]
use cosmwasm_std::{Decimal, DepsMut, Env, MessageInfo, Response, Uint128};

use crate::ContractError;
use crate::state::{CONFIG, BASKET, VOLATILITY};
use membrane::types::{cAsset, SupplyCap};
use membrane::math::decimal_multiplication;

/// Volatility list length required before volatility-based cap transforms apply.
const VOLATILITY_LIST_LIMIT: usize = 10;

/// Activate supply cap tracking when a position first takes on debt.
/// Only callable by Debt contract.
///
/// CDP parity: maps to the supply cap activation in cdp/risk_engine.rs::update_basket_tally()
pub fn activate_supply_caps(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if Some(info.sender.clone()) != config.debt_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;
    let mut basket = BASKET.load(deps.storage)?;

    // Add position's collateral to supply cap current_supply
    for ca in &position.collateral_assets {
        if let Some((idx, _cap)) = basket
            .collateral_supply_caps
            .iter()
            .enumerate()
            .find(|(_i, cap)| cap.asset_info.equal(&ca.asset.info))
        {
            basket.collateral_supply_caps[idx].current_supply += ca.asset.amount;
            basket.collateral_types[idx].asset.amount =
                basket.collateral_supply_caps[idx].current_supply;
        }
    }
    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new()
        .add_attribute("method", "activate_supply_caps")
        .add_attribute("position_id", position_id.to_string()))
}

/// Deactivate supply cap tracking when a position's debt reaches zero.
/// Only callable by Debt contract.
///
/// CDP parity: maps to the supply cap deactivation in cdp/risk_engine.rs::update_basket_tally()
pub fn deactivate_supply_caps(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if Some(info.sender.clone()) != config.debt_contract {
        return Err(ContractError::Unauthorized {});
    }

    let owner = deps.api.addr_validate(&position_owner)?;
    let (_index, position) =
        crate::state::get_target_position(deps.storage, owner.clone(), position_id)?;
    let mut basket = BASKET.load(deps.storage)?;

    // Remove position's collateral from supply cap current_supply
    for ca in &position.collateral_assets {
        if let Some((idx, _cap)) = basket
            .collateral_supply_caps
            .iter()
            .enumerate()
            .find(|(_i, cap)| cap.asset_info.equal(&ca.asset.info))
        {
            basket.collateral_supply_caps[idx].current_supply = basket.collateral_supply_caps[idx]
                .current_supply
                .checked_sub(ca.asset.amount)
                .unwrap_or(Uint128::zero());
            basket.collateral_types[idx].asset.amount =
                basket.collateral_supply_caps[idx].current_supply;
        }
    }
    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new()
        .add_attribute("method", "deactivate_supply_caps")
        .add_attribute("position_id", position_id.to_string()))
}

/// Assert that deposited assets are valid collateral in the basket.
///
/// CDP parity: maps to cdp/risk_engine.rs::assert_basket_assets()
pub fn assert_basket_assets(
    deps: &DepsMut,
    assets: &[cAsset],
) -> Result<(), ContractError> {
    let basket = BASKET.load(deps.storage)?;

    for asset in assets {
        let found = basket
            .collateral_types
            .iter()
            .any(|ct| ct.asset.info.equal(&asset.asset.info));
        if !found {
            return Err(ContractError::InvalidCollateral {});
        }
    }

    Ok(())
}

/// Transform supply caps based on per-asset volatility.
/// Higher volatility tightens the cap (multiplied by volatility index).
/// Does NOT alter multi-asset caps.
///
/// CDP parity: maps to cdp/rates.rs::transform_caps_based_on_volatility()
fn transform_caps_based_on_volatility(
    storage: &dyn cosmwasm_std::Storage,
    basket: &membrane::collateral::BasketCollateral,
) -> Result<Vec<SupplyCap>, ContractError> {
    let vol_caps: Vec<SupplyCap> = basket
        .collateral_supply_caps
        .iter()
        .map(|cap| {
            // Load volatility store for this asset
            if let Ok(vol_store) = VOLATILITY.load(storage, cap.asset_info.to_string()) {
                if vol_store.volatility_list.len() == VOLATILITY_LIST_LIMIT {
                    // Transform supply cap ratio based on asset volatility index
                    let new_supply_cap =
                        match decimal_multiplication(cap.supply_cap_ratio, vol_store.index) {
                            Ok(new_supply_cap) => new_supply_cap,
                            Err(_err) => cap.supply_cap_ratio,
                        };
                    Ok(SupplyCap {
                        supply_cap_ratio: std::cmp::min(new_supply_cap, Decimal::one()),
                        ..cap.clone()
                    })
                } else {
                    Ok(cap.clone())
                }
            } else {
                // No volatility data yet, use cap as-is
                Ok(cap.clone())
            }
        })
        .collect::<Result<Vec<SupplyCap>, ContractError>>()?;

    Ok(vol_caps)
}

/// Compute basket collateral ratios from stored prices (read-only, no oracle queries).
///
/// Returns a Vec<Decimal> where each entry is the value-ratio of the corresponding
/// collateral_type relative to the total basket value.
///
/// CDP parity: maps to the ratio computation portion of cdp/query.rs::get_cAsset_ratios()
fn get_basket_ratios(
    storage: &dyn cosmwasm_std::Storage,
    basket: &membrane::collateral::BasketCollateral,
) -> Result<Vec<Decimal>, ContractError> {
    use crate::state::STORED_PRICES;

    let mut values: Vec<Decimal> = Vec::with_capacity(basket.collateral_types.len());
    let mut total_value = Decimal::zero();

    for ct in &basket.collateral_types {
        let stored = STORED_PRICES.may_load(storage, ct.asset.info.to_string())
            .map_err(ContractError::Std)?;

        let value = match stored {
            Some(sp) => {
                let amount_dec = Decimal::from_ratio(ct.asset.amount, 1u128);
                amount_dec * sp.price.price
            }
            None => Decimal::zero(),
        };
        values.push(value);
        total_value += value;
    }

    let ratios: Vec<Decimal> = if total_value.is_zero() {
        values.iter().map(|_| Decimal::zero()).collect()
    } else {
        values.iter().map(|v| *v / total_value).collect()
    };

    Ok(ratios)
}

/// Update basket supply cap tallies.
///
/// 1. Update SupplyCap.current_supply for each collateral asset (add or subtract).
/// 2. Update basket.collateral_types[i].asset.amount to match.
/// 3. If NOT from liquidation: compute basket ratios and check supply cap limits.
/// 4. Check both individual and multi-asset supply caps.
/// 5. Error if any cap is exceeded (with position-aware logic from CDP).
///
/// CDP parity: maps to cdp/risk_engine.rs::update_basket_tally()
pub fn update_basket_tally(
    deps: &mut DepsMut,
    _env: Env,
    collateral_assets: Vec<cAsset>,
    full_positions_assets: Vec<cAsset>,
    add_to_cAsset: bool,
    from_liquidation: bool,
) -> Result<(), ContractError> {
    let mut basket = BASKET.load(deps.storage)?;

    // Step 1 & 2: Update SupplyCap objects and sync collateral_types amounts
    for cAsset in collateral_assets.clone() {
        if let Some((index, mut cap)) = basket
            .clone()
            .collateral_supply_caps
            .into_iter()
            .enumerate()
            .find(|(_x, cap)| cap.asset_info.equal(&cAsset.asset.info))
        {
            if add_to_cAsset {
                cap.current_supply += cAsset.asset.amount;
            } else {
                cap.current_supply = match cap.current_supply.checked_sub(cAsset.asset.amount) {
                    Ok(diff) => diff,
                    Err(_) => Uint128::zero(),
                };
            }

            // Update
            basket.collateral_supply_caps[index] = cap.clone();
            basket.collateral_types[index].asset.amount = cap.current_supply;
        }
    }

    // Transform supply caps based on asset volatility
    // This doesn't alter multi-asset caps
    let supply_caps = match transform_caps_based_on_volatility(deps.storage, &basket) {
        Ok(supply_caps) => supply_caps,
        Err(_err) => basket.collateral_supply_caps.clone(),
    };

    // Step 3-5: Check supply cap ratios (skip for liquidations)
    if !from_liquidation {
        let new_basket_ratios = get_basket_ratios(deps.storage, &basket)?;

        // Assert new ratios aren't above Collateral Supply Caps. If so, conditionally error.
        for (i, ratio) in new_basket_ratios.clone().into_iter().enumerate() {
            // Initialize in_position to check if the position has these assets
            let mut in_position = false;

            if add_to_cAsset {
                // Check if the depositing assets are part of this cap
                if collateral_assets
                    .iter()
                    .any(|ca| ca.asset.info.equal(&supply_caps[i].asset_info))
                {
                    in_position = true;
                }
            } else {
                // Check if the position has these assets if withdrawing
                // So if a withdrawal would push an asset over cap that isn't being withdrawn
                // currently but is in the position, it errors
                if full_positions_assets
                    .iter()
                    .any(|ca| ca.asset.info.equal(&supply_caps[i].asset_info))
                {
                    in_position = true;
                }
                // If the position is withdrawing the asset, set to false.
                // User Flow: If a user fully withdraws an asset that is over cap BUT
                // doesn't completely pull it under cap, we don't want to block withdrawals
                if collateral_assets
                    .iter()
                    .any(|ca| ca.asset.info.equal(&supply_caps[i].asset_info))
                {
                    // Check if its being fully withdrawn from the position or if its the only asset in the position
                    if full_positions_assets
                        .iter()
                        .any(|ca| ca.asset.info.equal(&supply_caps[i].asset_info))
                    {
                        // If the asset is still in the position, it must be the only remaining asset
                        if full_positions_assets.len() > 1 {
                            in_position = true;
                        } else {
                            // You can withdraw the only asset freely
                            in_position = false;
                        }
                    } else {
                        // This means the asset was fully withdrawn
                        in_position = false;
                    }
                }
            }

            // Check if deposits are blocked because Disco returned 0 for this asset
            let disco_blocked = crate::state::DISCO_BLOCKED
                .may_load(deps.storage, basket.collateral_supply_caps[i].asset_info.to_string())?
                .unwrap_or(false);

            // We skip the check if the supply cap is zero bc those are expunged assets.
            // disco_blocked uses the same logic so blocked assets behave identically to a supply cap breach.
            if !basket.collateral_supply_caps.is_empty()
                && (ratio > supply_caps[i].supply_cap_ratio || disco_blocked)
                && in_position
                && !supply_caps[i].supply_cap_ratio.is_zero()
            {
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

        // Assert for Multi-asset caps as well
        if !basket.multi_asset_supply_caps.is_empty() {
            for multi_asset_cap in basket.clone().multi_asset_supply_caps {
                // Initialize total_ratio
                let mut total_ratio = Decimal::zero();
                // Initialize in_position to check if the position has these assets
                let mut in_position = false;

                // Find & add ratio for each asset
                for asset in multi_asset_cap.clone().assets {
                    if let Some((i, _cap)) = basket
                        .clone()
                        .collateral_supply_caps
                        .into_iter()
                        .enumerate()
                        .find(|(_i, cap)| cap.asset_info.equal(&asset))
                    {
                        total_ratio += new_basket_ratios[i];
                    }
                    if add_to_cAsset {
                        // Check if the depositing assets are part of this cap
                        if collateral_assets
                            .iter()
                            .any(|ca| ca.asset.info.equal(&asset))
                        {
                            in_position = true;
                        }
                    } else {
                        // Check if the position has these assets if withdrawing
                        // So if a withdrawal would push an asset over cap, it errors
                        if full_positions_assets
                            .iter()
                            .any(|ca| ca.asset.info.equal(&asset))
                        {
                            in_position = true;
                        }
                        // If the position is withdrawing the asset, set to false.
                        // User Flow: If a user fully withdraws an asset that is over cap BUT
                        // doesn't completely pull it under cap, we don't want to block withdrawals
                        if collateral_assets
                            .iter()
                            .any(|ca| ca.asset.info.equal(&asset))
                        {
                            // Check if its being fully withdrawn from the position or if its the only asset
                            if full_positions_assets
                                .iter()
                                .any(|ca| ca.asset.info.equal(&asset))
                            {
                                // If the asset is still in the position, it must be the only remaining asset
                                if full_positions_assets.len() > 1 {
                                    in_position = true;
                                } else {
                                    // You can withdraw the only asset freely
                                    in_position = false;
                                }
                            } else {
                                // This means the asset was fully withdrawn
                                in_position = false;
                            }
                        }
                    }
                }

                // Error if over cap
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

    // Save updated basket
    BASKET.save(deps.storage, &basket)?;

    Ok(())
}
