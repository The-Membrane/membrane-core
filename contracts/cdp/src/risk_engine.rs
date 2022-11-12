use std::cmp::min;

use cosmwasm_std::{Decimal, Uint128, StdResult, Env, QuerierWrapper, Storage};
use membrane::math::{decimal_multiplication, decimal_division}; 
use membrane::positions::Config;
use membrane::types::{Basket, PoolInfo, Asset, cAsset, SupplyCap, AssetInfo};

use crate::state::{CREDIT_MULTI, CONFIG, BASKETS};
use crate::positions::{get_stability_pool_liquidity, get_asset_liquidity, get_cAsset_ratios};
use crate::query::{get_cAsset_ratios_imut, get_asset_values_imut};
use crate::error::ContractError;
use crate::rates::get_credit_asset_multiplier;



pub fn assert_basket_assets(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket_id: Uint128,
    assets: Vec<Asset>,
    add_to_cAsset: bool,
) -> Result<Vec<cAsset>, ContractError> {

    let mut basket: Basket = match BASKETS.load(storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    //Checking if Assets for the position are available collateral assets in the basket
    let mut valid = false;
    let mut collateral_assets: Vec<cAsset> = Vec::new();

    for asset in assets {
        for cAsset in basket.clone().collateral_types {
            match (asset.clone().info, cAsset.asset.info) {
                (
                    AssetInfo::Token { address },
                    AssetInfo::Token {
                        address: cAsset_address,
                    },
                ) => {
                    if address == cAsset_address {
                        valid = true;
                        collateral_assets.push(cAsset {
                            asset: asset.clone(),
                            ..cAsset
                        });
                    }
                }
                (
                    AssetInfo::NativeToken { denom },
                    AssetInfo::NativeToken {
                        denom: cAsset_denom,
                    },
                ) => {
                    if denom == cAsset_denom {
                        valid = true;
                        collateral_assets.push(cAsset {
                            asset: asset.clone(),
                            ..cAsset
                        });
                    }
                }
                (_, _) => continue,
            }
        }

        //Error if invalid collateral, meaning it wasn't found in the list of cAssets
        if !valid {
            return Err(ContractError::InvalidCollateral {});
        }
        valid = false;
    }

    //Add valid asset amounts to running basket total
    //This is done before deposit() so if that errors this will revert as well
    //////We don't want this to trigger for withdrawals bc debt needs to accrue on the previous basket state
    //////For deposit's its fine bc it'll error when invalid and doesn't accrue debt
    if add_to_cAsset {
        update_basket_tally(
            storage,
            querier,
            env,
            &mut basket,
            collateral_assets.clone(),
            add_to_cAsset,
        )?;
        BASKETS.save(storage, basket_id.to_string(), &basket)?;
    }

    Ok(collateral_assets)
}


pub fn update_basket_tally(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
    collateral_assets: Vec<cAsset>,
    add_to_cAsset: bool,
) -> Result<(), ContractError> {

    let config = CONFIG.load(storage)?;

    //Update SupplyCap objects 
    for cAsset in collateral_assets.iter() {

        if let Some((index, mut cap)) = basket
            .clone()
            .collateral_supply_caps
            .into_iter()
            .enumerate()
            .find(|(x, cap)| cap.asset_info.equal(&cAsset.asset.info))
        {
            if add_to_cAsset {
                cap.current_supply += cAsset.asset.amount;
            } else {
                cap.current_supply -= cAsset.asset.amount;
            }

            //Update
            basket.collateral_supply_caps[index] = cap.clone();
            basket.collateral_types[index].asset.amount = cap.current_supply;
        }
    
    }
    
    let (mut new_basket_ratios, _) =
        get_cAsset_ratios(storage, env, querier, basket.clone().collateral_types, config.clone())?;

 
    //Assert new ratios aren't above Collateral Supply Caps. If so, error.
    //Only for deposits
    for (i, ratio) in new_basket_ratios.into_iter().enumerate() {
        if basket.collateral_supply_caps != vec![] {
            if ratio > basket.collateral_supply_caps[i].supply_cap_ratio && add_to_cAsset {

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
    }

    Ok(())
}


pub fn get_basket_debt_caps(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    basket: &mut Basket,
) -> Result<Vec<Uint128>, ContractError> {
    let config: Config = CONFIG.load(storage)?;

    //Get the Basket's asset ratios
    let (cAsset_ratios, _) = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        basket.clone().collateral_types,
        config.clone(),
    )?;
    
    //Get credit_asset's liquidity_multiplier
    let credit_asset_multiplier = get_credit_asset_multiplier(
        storage,
        querier,
        env.clone(),
        config.clone(),
        basket.clone(),
    )?;
    
    //Get the base debt cap
    let mut debt_cap =
        get_asset_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?
            * credit_asset_multiplier;
            

    //Get SP liquidity
    let sp_liquidity = get_stability_pool_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?;

    //Add basket's ratio of SP liquidity to the cap
    //Ratio is based off of its ratio of the total credit_asset_multiplier
    debt_cap += decimal_multiplication(
        Decimal::from_ratio(sp_liquidity, Uint128::new(1)), 
        decimal_division(credit_asset_multiplier, CREDIT_MULTI.load(storage, basket.clone().credit_asset.info.to_string())?) 
    ) * Uint128::new(1);
    

    //If debt cap is less than the minimum, set it to the minimum
    if debt_cap < (config.base_debt_cap_multiplier * config.debt_minimum) {
        debt_cap = (config.base_debt_cap_multiplier * config.debt_minimum);
    }
    
    
    //Don't double count debt btwn Stability Pool based caps and TVL based caps
    for cap in basket.clone().collateral_supply_caps {
        //If the cap is based on sp_liquidity, subtract its value from the debt_cap
        if let Some(sp_ratio) = cap.stability_pool_ratio_for_debt_cap {
            debt_cap -= decimal_multiplication(Decimal::from_ratio(sp_liquidity, Uint128::new(1)), sp_ratio) * Uint128::new(1);
        }
    }

    //Calc total basket debt
    let total_debt: Uint128 = basket.clone().collateral_supply_caps
        .into_iter()
        .map(|cap| cap.debt_total)
        .collect::<Vec<Uint128>>()
        .into_iter()
        .sum();

    //If the basket's proportion of it's debt cap is >= the desired debt util, negative rates are turned off
    //This protects against perpetual devaluing of the credit asset as Membrane is disincentivizing new debt w/ high rates
    //Note: This could be changed to "IF each asset's util is above desired"...
    //...but the current implementation turns them off faster, might as well be on the safe side
    if Decimal::from_ratio(total_debt, debt_cap) >= basket.clone().desired_debt_cap_util {
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
                    decimal_multiplication(Decimal::from_ratio(sp_liquidity, Uint128::new(1)), sp_ratio) * Uint128::new(1)
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


pub fn update_debt_per_asset_in_position(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    basket_id: Uint128,
    old_assets: Vec<cAsset>,
    new_assets: Vec<cAsset>,
    credit_amount: Decimal,
) -> Result<(), ContractError> {
    //Load Basket
    let mut basket: Basket = match BASKETS.load(storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    //Note: Vec lengths need to match
    let (old_ratios, _) = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        old_assets.clone(),
        config.clone(),
    )?;
    let (new_ratios, _) = get_cAsset_ratios(storage, env.clone(), querier, new_assets, config)?;

    let mut over_cap = false;
    let mut assets_over_cap = vec![];

    //Calculate debt per asset caps
    let cAsset_caps = get_basket_debt_caps(storage, querier, env, &mut basket)?;

    for i in 0..old_ratios.len() {
        match old_ratios[i].atomics().checked_sub(new_ratios[i].atomics()) {
            Ok(difference) => {
                //Old was > than New
                //So we subtract the % difference in debt from said asset

                basket.collateral_supply_caps = basket
                    .clone()
                    .collateral_supply_caps
                    .into_iter() //We don't take LP supply caps when calculating debt
                    .map(|mut cap| {
                        if cap.asset_info.equal(&old_assets[i].asset.info) {
                            match cap.debt_total.checked_sub(
                                decimal_multiplication(Decimal::new(difference), credit_amount)
                                    * Uint128::new(1u128),
                            ) {
                                Ok(difference) => {
                                    if cap.current_supply.is_zero() {
                                        //This removes rounding errors that would slowly increase resting interest rates
                                        //Doesn't effect checks for bad debt since its basket debt not position.credit_amount
                                        //its a .000001 error, so shouldn't effect overall calcs and shouldn't be profitably spammable
                                        cap.debt_total = Uint128::zero();
                                    } else {
                                        cap.debt_total = difference;
                                    }
                                }
                                Err(_) => {
                                    //Can't return an Error here without inferring the map return type
                                }
                            };
                        }

                        cap
                    })
                    .collect::<Vec<SupplyCap>>();
            }
            Err(_) => {
                //Old was < than New
                //So we add the % difference in debt to said asset
                let difference = new_ratios[i] - old_ratios[i];

                basket.collateral_supply_caps = basket
                    .clone()
                    .collateral_supply_caps
                    .into_iter()
                    .enumerate() //We don't take LP supply caps when calculating debt
                    .map(|(index, mut cap)| {
                        if cap.asset_info.equal(&old_assets[i].asset.info) {
                            let asset_debt = decimal_multiplication(difference, credit_amount)
                                * Uint128::new(1u128);

                            //Assert its not over the cap
                            if (cap.debt_total + asset_debt) <= cAsset_caps[index] {
                                cap.debt_total += asset_debt;
                            } else {
                                over_cap = true;
                                assets_over_cap.push(cap.asset_info.to_string());
                            }
                        }

                        cap
                    })
                    .collect::<Vec<SupplyCap>>();
            }
        }
    }

    if over_cap {
        return Err(ContractError::CustomError {
            val: format!("Assets over debt cap: {:?}", assets_over_cap),
        });
    }

    BASKETS.save(storage, basket_id.to_string(), &basket)?;

    Ok(())
}

pub fn update_basket_debt(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    basket: &mut Basket,
    collateral_assets: Vec<cAsset>,
    credit_amount: Uint128,
    add_to_debt: bool,
    interest_accrual: bool,
) -> Result<(), ContractError> {
    
    let (cAsset_ratios, _) = get_cAsset_ratios(
        storage,
        env.clone(),
        querier,
        collateral_assets.clone(),
        config,
    )?;

    let mut asset_debt = vec![];

    //Save the debt distribution per asset to a Vec
    for asset in cAsset_ratios {
        asset_debt.push(asset * credit_amount);
    }
    
    let mut over_cap = false;
    let mut assets_over_cap = vec![];

    //Calculate debt per asset caps
    let cAsset_caps = get_basket_debt_caps(storage, querier, env, basket)?;

    //Update basket debt tally
    if add_to_debt {
        basket.credit_asset.amount += credit_amount;
    } else {
        basket.credit_asset.amount = match basket.credit_asset.amount.checked_sub(credit_amount){
            Ok(diff) => diff,
            Err(_err) => return Err(ContractError::FaultyCalc {  })
        };
    }

    //Update supply caps w/ new debt distribution
    for (index, cAsset) in collateral_assets.iter().enumerate() {
        basket.collateral_supply_caps = basket
            .clone()
            .collateral_supply_caps
            .into_iter()
            .enumerate()
            .map(|(i, mut cap)| {
                //Add or subtract deposited amount to/from the correlated cAsset object
                if cap.asset_info.equal(&cAsset.asset.info) {
                    if add_to_debt {
                        //Assert its not over the cap
                        //IF the debt is adding to interest then we allow it to exceed the cap
                        if (cap.debt_total + asset_debt[index]) <= cAsset_caps[i]
                            || interest_accrual
                        {
                            cap.debt_total += asset_debt[index];
                        } else {
                            over_cap = true;
                            assets_over_cap.push(cap.asset_info.to_string());
                        }
                    } else {
                        match cap.debt_total.checked_sub(asset_debt[index]) {
                            Ok(difference) => {
                                cap.debt_total = difference;
                            }
                            Err(_) => {
                                //Don't subtract bc it'll end up being an invalid repayment error anyway
                                //Can't return an Error here without inferring the map return type
                            }
                        };
                    }
                }

                cap
            })
            .collect::<Vec<SupplyCap>>();       

    }

    //Error if over the asset cap
    if over_cap {
        return Err(ContractError::CustomError {
            val: format!(
                "This increase of debt sets [ {:?} ] assets above the protocol debt cap",
                assets_over_cap
            ),
        });
    }

    Ok(())
}

////////////Immutable fns for Queries/////

fn get_credit_asset_multiplier_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    basket: Basket,
) -> StdResult<Decimal> {
    //Find Baskets with similar credit_asset
    let mut baskets: Vec<Basket> = vec![];

    //Has to be done ugly due to an immutable borrow
    //Uint128 to int
    let range: i32 = config.current_basket_id.to_string().parse().unwrap();

    for basket_id in 1..range {
        let stored_basket = BASKETS.load(storage, basket_id.to_string())?;

        if stored_basket
            .credit_asset
            .info
            .equal(&basket.credit_asset.info)
        {
            baskets.push(stored_basket);
        }
    }

    //Calc collateral_type totals
    let mut collateral_totals: Vec<(Asset, Option<PoolInfo>)> = vec![];

    for basket in baskets.clone() {
        //Find collateral's corresponding total in list
        for collateral in basket.collateral_types {
            if let Some((index, _total)) = collateral_totals
                .clone()
                .into_iter()
                .enumerate()
                .find(|(_i, (asset, _pool))| asset.info.equal(&collateral.asset.info))
            {
                //Add to collateral total
                collateral_totals[index].0.amount += collateral.asset.amount;
            } else {
                //Add collateral type to list
                collateral_totals.push((Asset {
                    info: collateral.asset.info,
                    amount: collateral.asset.amount,
                },
                    collateral.pool_info,
                ));
            }            
        }
    }

    //Get total_collateral_value
    let temp_cAssets: Vec<cAsset> = collateral_totals
        .clone()
        .into_iter()
        .map(|(asset, pool_info)| cAsset {
            asset,
            max_borrow_LTV: Decimal::zero(),
            max_LTV: Decimal::zero(),
            pool_info,
            rate_index: Decimal::one(),
        })
        .collect::<Vec<cAsset>>();
        
        let total_collateral_value: Decimal = get_asset_values_imut(
            storage,
            env.clone(),
            querier,
            temp_cAssets,
            config.clone(),
            None,
        )?
        .0
        .into_iter()
        .sum();
    
        //Get basket_collateral_value   
        let basket_collateral_value: Decimal = get_asset_values_imut(
            storage,
            env.clone(),
            querier,
            basket.clone().collateral_types,
            config.clone(),
            None,
        )?
        .0
        .into_iter()
        .sum();
    
        //Find Basket's ratio of total collateral
        let basket_tvl_ratio: Decimal = {
            if !basket_collateral_value.is_zero() {
                decimal_division(basket_collateral_value, total_collateral_value )
            } else {
                Decimal::zero()
            }
        };    
        
        //Get credit_asset's liquidity multiplier
        let credit_asset_liquidity_multiplier =
            CREDIT_MULTI.load(storage, basket.clone().credit_asset.info.to_string())?;
    
        //Get Minimum between (ratio * credit_asset's multiplier) and basket's liquidity_multiplier
        let multiplier = min(
            decimal_multiplication(basket_tvl_ratio, credit_asset_liquidity_multiplier),
            basket.liquidity_multiplier,
        );
        
        Ok(multiplier)
}


pub fn get_basket_debt_caps_imut(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    //These are Basket specific fields
    basket: Basket,
) -> StdResult<Vec<Uint128>> {
    
    let config: Config = CONFIG.load(storage)?;
    
    //Get the Basket's asset ratios
    let cAsset_ratios = get_cAsset_ratios_imut(
        storage,
        env.clone(),
        querier,
        basket.clone().collateral_types,
        config.clone(),
    )?;

    //Get credit_asset's liquidity_multiplier
    let credit_asset_multiplier = get_credit_asset_multiplier_imut(
        storage,
        querier,
        env.clone(),
        config.clone(),
        basket.clone(),
    )?;
    
    //Get the base debt cap
    let mut debt_cap =
        get_asset_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?
            * credit_asset_multiplier;

    //Get SP liquidity
    let sp_liquidity = get_stability_pool_liquidity(querier, config.clone(), basket.clone().credit_asset.info)?;

    //Add basket's ratio of SP liquidity to the cap
    //Ratio is based off of its ratio of the total credit_asset_multiplier
    debt_cap += decimal_multiplication(
        Decimal::from_ratio(sp_liquidity, Uint128::new(1)), 
        decimal_division(credit_asset_multiplier, CREDIT_MULTI.load(storage, basket.clone().credit_asset.info.to_string())?) 
    ) * Uint128::new(1);


    //If debt cap is less than the minimum, set it to the minimum
    if debt_cap < (config.base_debt_cap_multiplier * config.debt_minimum) {
        debt_cap = (config.base_debt_cap_multiplier * config.debt_minimum);
    }

     //Don't double count debt btwn Stability Pool based ratios and TVL based ratios
     for cap in basket.clone().collateral_supply_caps {
        //If the cap is based on sp_liquidity, subtract its value from the debt_cap
        if let Some(sp_ratio) = cap.stability_pool_ratio_for_debt_cap {
            debt_cap -= decimal_multiplication(Decimal::from_ratio(sp_liquidity, Uint128::new(1)), sp_ratio) * Uint128::new(1);
        }
    }

    let mut per_asset_debt_caps = vec![];
    
    for (i, cAsset_ratio) in cAsset_ratios.clone().into_iter().enumerate() {
        // If supply cap is 0, then debt cap is 0
        if basket.clone().collateral_supply_caps != vec![] {
            if basket.clone().collateral_supply_caps[i]
                .supply_cap_ratio
                .is_zero()
            {
                per_asset_debt_caps.push(Uint128::zero());

            } else if let Some(sp_ratio) = basket.clone().collateral_supply_caps[i].stability_pool_ratio_for_debt_cap{
                //If cap is supposed to be based off of a ratio of SP liquidity, calculate                                
                per_asset_debt_caps.push(
                    decimal_multiplication(Decimal::from_ratio(sp_liquidity, Uint128::new(1)), sp_ratio) * Uint128::new(1)
                );
            } else {
                per_asset_debt_caps.push(cAsset_ratio * debt_cap);
            }
        } else {
            per_asset_debt_caps.push(cAsset_ratio * debt_cap);
        }
    }

    Ok(per_asset_debt_caps)
}

