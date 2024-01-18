use cosmwasm_std::{Decimal, Uint128, Env, QuerierWrapper, Storage, StdResult, StdError, Addr};

use membrane::cdp::Config;
use membrane::stability_pool::QueryMsg as SP_QueryMsg;
use membrane::types::{Basket, Asset, cAsset, SupplyCap, AssetPool};
use membrane::helpers::{get_asset_liquidity, get_owner_liquidity_multiplier, get_stability_pool_liquidity};
use membrane::math::decimal_multiplication; 

use crate::state::{CONFIG, BASKET};
use crate::query::get_cAsset_ratios;
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
                    Err(_) => return Err(ContractError::CustomError {
                        val: format!(
                            "Removal amount ({}) is greater than current supply ({}) for {}", 
                            cAsset.asset.amount, cap.current_supply, cap.asset_info
                        ),
                    }),
                }; 
            }

            //Update
            basket.collateral_supply_caps[index] = cap.clone();
            basket.collateral_types[index].asset.amount = cap.current_supply;
        }
    
    }
    
    if !from_liquidation {
        let (new_basket_ratios, _) =
            get_cAsset_ratios(storage, env, querier, basket.clone().collateral_types, config)?;

        
        //Initialize in_position to check if the position has these assets
        let mut in_position = false;

        //Assert new ratios aren't above Collateral Supply Caps. If so, conditionally error.
        for (i, ratio) in new_basket_ratios.clone().into_iter().enumerate() {
            
            if add_to_cAsset {
                //Check if the depositing assets are part of this cap
                if let Some((_i, _cAsset)) = collateral_assets.clone().into_iter().enumerate().find(|(_i, cAsset)| cAsset.asset.info.equal(&basket.collateral_supply_caps[i].asset_info)){
                    in_position = true;
                }
            } else {
                //Check if the position has these assets if ur withdrawing
                //So if a withdrawal would push an asset over cap that isn't being withdrawn currently but is in the position, it errors
                if let Some((_i, _cAsset)) = full_positions_assets.clone().into_iter().enumerate().find(|(_i, cAsset)| cAsset.asset.info.equal(&basket.collateral_supply_caps[i].asset_info)){
                    in_position = true;
                }
                //If the position is withdrawing the asset, set to false.
                //User Flow: If a user fully withdraws an asset that is over cap BUT....
                //..doesn't completely pull it under cap, we don't want to block withdrawals
                if let Some((_i, _withdrawn_cAsset)) = collateral_assets.clone().into_iter().enumerate().find(|(_i, cAsset)| cAsset.asset.info.equal(&basket.collateral_supply_caps[i].asset_info)){
                    //Check if its being fully withdrawn from the position or if its the only asset in the position
                    if let Some((_i, _position_cAsset)) = full_positions_assets.clone().into_iter().enumerate().find(|(_i, cAsset)| cAsset.asset.info.equal(&basket.collateral_supply_caps[i].asset_info)){
                        //If the asset is still in the position, it must be the only remaining asset
                        if full_positions_assets.len() > 1 {
                            in_position = true;                     
                        } else {
                            //You can withdraw the only asset freely
                            in_position = false;
                        }
                    } else {
                        //This means the asset was fully withdrawn
                        in_position = false;
                    }
                    
                }
            }

            if basket.collateral_supply_caps != vec![] && ratio > basket.collateral_supply_caps[i].supply_cap_ratio && in_position {
                
                return Err(ContractError::CustomError {
                    val: format!(
                        "Supply cap ratio for {} is over the limit ({} > {})",
                        basket.collateral_supply_caps[i].asset_info,
                        ratio,
                        basket.collateral_supply_caps[i].supply_cap_ratio
                    ),
                });            
            }
        }

        //Assert for Multi-asset caps as well
        if basket.multi_asset_supply_caps != vec![]{
            for multi_asset_cap in basket.clone().multi_asset_supply_caps {

                //Initialize total_ratio
                let mut total_ratio = Decimal::zero();
                //Initialize in_position to check if the position has these assets
                let mut in_position = false;
                
                //Find & add ratio for each asset
                for asset in multi_asset_cap.clone().assets {
                    if let Some((i, _cap)) = basket.clone().collateral_supply_caps.into_iter().enumerate().find(|(_i, cap)| cap.asset_info.equal(&asset)){
                        total_ratio += new_basket_ratios[i];
                    }
                    if add_to_cAsset {
                        //Check if the depositing assets are part of this cap
                        if let Some((_i, _cAsset)) = collateral_assets.clone().into_iter().enumerate().find(|(_i, cAsset)| cAsset.asset.info.equal(&asset)){
                            in_position = true;
                        }
                    } else {
                        //Check if the position has these assets if ur withdrawing
                        //So if a withdrawal would push an asset over cap, it errors
                        if let Some((_i, _cAsset)) = full_positions_assets.clone().into_iter().enumerate().find(|(_i, cAsset)| cAsset.asset.info.equal(&asset)){
                            in_position = true;
                        }
                        //If the position is withdrawing the asset, set to false.
                        //User Flow: If a user fully withdraws an asset that is over cap BUT....
                        //..doesn't completely pull it under cap, we don't want to block withdrawals
                        if let Some((_i, _withdrawn_cAsset)) = collateral_assets.clone().into_iter().enumerate().find(|(_i, cAsset)| cAsset.asset.info.equal(&asset)){
                            //Check if its being fully withdrawn from the position or if its the only asset in the position
                            if let Some((_i, _position_cAsset)) = full_positions_assets.clone().into_iter().enumerate().find(|(_i, cAsset)| cAsset.asset.info.equal(&asset)){
                                //If the asset is still in the position, it must be the only remaining asset
                                if full_positions_assets.len() > 1 {
                                    in_position = true;                        
                                } else {
                                    //You can withdraw the only asset freely
                                    in_position = false;
                                }
                            } else {
                                //This means the asset was fully withdrawn
                                in_position = false;
                            }
                            
                        }
                    }
                }
                                
                //Error if over cap
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


/// Calculate the debt cap for each asset in the Basket using network liquidity 
pub fn get_basket_debt_caps(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    //These are Basket specific fields
    basket: &mut Basket,
) -> StdResult<Vec<Uint128>> {    
    let config: Config = CONFIG.load(storage)?;
    
    //Get the Basket's asset ratios
    let (cAsset_ratios, _) = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        basket.clone().collateral_types,
        config.clone(),
    )?;

    //Split the basket's total debt into each asset's SupplyCap.debt total
    for (i, cAsset) in basket.clone().collateral_types.into_iter().enumerate() {
        basket.collateral_supply_caps = basket.clone().collateral_supply_caps
            .into_iter()
            .map(|mut cap| {
                if cap.asset_info.equal(&cAsset.asset.info) {
                    cap.debt_total = decimal_multiplication(cAsset_ratios[i], Decimal::from_ratio(basket.credit_asset.amount, Uint128::one())).unwrap_or(Decimal::zero()).to_uint_floor();
                }

                cap
            })
            .collect::<Vec<SupplyCap>>();
    }
    
    //Get owner liquidity parameters from Osmosis Proxy
    let owner_params = match get_owner_liquidity_multiplier(
        querier, 
        env.contract.address.to_string(),
        config.clone().osmosis_proxy.unwrap_or_else(|| Addr::unchecked("")).to_string()
    ){
        Ok(params) => params,
        Err(err) => return Err(StdError::GenericErr { msg: String::from("Error at line 174")})
    };
    
    let liquidity = get_asset_liquidity(
        querier, 
        config.clone().liquidity_contract.unwrap_or_else(|| Addr::unchecked("")).to_string(),
        basket.clone().credit_asset.info
    )?;
    
    //Get the base debt cap
    let mut debt_cap = decimal_multiplication(Decimal::from_ratio(liquidity, Uint128::one()), owner_params.0)? * Uint128::one();


    //Get SP cap space the contract is allowed to use
    let sp_liquidity = match get_stability_pool_liquidity(querier, config.clone().stability_pool.unwrap_or_else(|| Addr::unchecked("")).to_string()){
        Ok(liquidity) => liquidity,
        Err(_) => //Query the SP regularly
        {
            let sp_pool: AssetPool = querier.query_wasm_smart::<AssetPool>(
                config.clone().stability_pool.unwrap_or_else(|| Addr::unchecked("")).to_string(), 
                &SP_QueryMsg::AssetPool {
                    user: None,
                    deposit_limit: Some(1),
                    start_after: None,
                }
            )?;

            sp_pool.credit_asset.amount
        }
    };
    let sp_liquidity = Decimal::from_ratio(sp_liquidity, Uint128::new(1));
    let sp_cap_space = decimal_multiplication(sp_liquidity, owner_params.1)?;

    //Add SP cap space to the cap
    debt_cap += sp_cap_space * Uint128::one();

    //If debt cap is less than the minimum, set it to the minimum
    if debt_cap < (config.base_debt_cap_multiplier * config.debt_minimum) {
        debt_cap = (config.base_debt_cap_multiplier * config.debt_minimum);
    }
    
     //Don't double count debt btwn Stability Pool based ratios and TVL based ratios
     for cap in basket.clone().collateral_supply_caps {
        //If the cap is based on sp_liquidity, subtract its value from the debt_cap
        if let Some(sp_ratio) = cap.stability_pool_ratio_for_debt_cap {
            debt_cap -= decimal_multiplication(sp_cap_space, sp_ratio)? * Uint128::one();
        }
    }

    //If the basket's proportion of it's debt cap is >= 1, negative rates are turned off
    //This protects against perpetual devaluing of the credit asset as Membrane is disincentivizing new debt w/ high rates
    //Note: This could be changed to "IF each asset's util is above desired"...
    //...but the current implementation turns them off faster, might as well be on the safe side
    if Decimal::from_ratio(basket.credit_asset.amount, debt_cap) >= Decimal::one() {
        basket.negative_rates = false;
    }

    let mut per_asset_debt_caps = vec![];

    //Calc per asset debt caps
    for (i, cAsset_ratio) in cAsset_ratios.clone().into_iter().enumerate() {
                
        if basket.clone().collateral_supply_caps != vec![] {
            // If supply cap is 0, then debt cap is 0
            if basket.clone().collateral_supply_caps[i]
                .supply_cap_ratio
                .is_zero()
            {
                per_asset_debt_caps.push(Uint128::zero());
            } else if let Some(sp_ratio) = basket.clone().collateral_supply_caps[i].stability_pool_ratio_for_debt_cap{
                //If cap is supposed to be based off of a ratio of SP liquidity, calculate
                per_asset_debt_caps.push(
                    decimal_multiplication(sp_liquidity, sp_ratio)? * Uint128::new(1)
                );
            } else {
                //TVL Ratio * Cap 
                per_asset_debt_caps.push(cAsset_ratio * debt_cap);
            }
        } else {
            //TVL Ratio * Cap 
            per_asset_debt_caps.push(cAsset_ratio * debt_cap);
        }
    }
    
    Ok(per_asset_debt_caps)
}