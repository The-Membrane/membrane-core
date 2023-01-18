
use cosmwasm_std::{Decimal, Uint128, Env, QuerierWrapper, Storage, to_binary, QueryRequest, WasmQuery, StdResult, StdError};

use membrane::cdp::Config;
use membrane::stability_pool::QueryMsg as SP_QueryMsg;
use membrane::types::{Basket, Asset, cAsset, SupplyCap, AssetPool};
use membrane::helpers::get_asset_liquidity;
use membrane::math::decimal_multiplication; 

use crate::state::{CONFIG, BASKET};
use crate::query::get_cAsset_ratios;
use crate::error::ContractError;

/// Asserts that the assets provided are valid collateral assets in the basket
pub fn assert_basket_assets(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    assets: Vec<Asset>,
    add_to_cAsset: bool,
) -> Result<Vec<cAsset>, ContractError> {
    let mut basket: Basket = BASKET.load(storage)?;

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
        BASKET.save(storage, &basket)?;
    }

    Ok(collateral_assets)
}

/// Update SupplyCap objects in Basket 
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
            .find(|(_x, cap)| cap.asset_info.equal(&cAsset.asset.info))
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
    
    let (new_basket_ratios, _) =
        get_cAsset_ratios(storage, env, querier, basket.clone().collateral_types, config)?;

 
    //Assert new ratios aren't above Collateral Supply Caps. If so, error.
    //Only for deposits
    for (i, ratio) in new_basket_ratios.clone().into_iter().enumerate() {
        if basket.collateral_supply_caps != vec![] && ratio > basket.collateral_supply_caps[i].supply_cap_ratio && add_to_cAsset{

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

            
            //Find & add ratio for each asset
            for asset in multi_asset_cap.clone().assets{
                if let Some((i, _cap)) = basket.clone().collateral_supply_caps.into_iter().enumerate().find(|(_i, cap)| cap.asset_info.equal(&asset)){
                    total_ratio += new_basket_ratios[i];
                }
            }

            //Error if over cap
            if total_ratio > multi_asset_cap.supply_cap_ratio {
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

    Ok(())
}

/// Update the distribution of Basket debt per asset after Position collateral makeup changes
pub fn update_debt_per_asset_in_position(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    old_assets: Vec<cAsset>,
    new_assets: Vec<cAsset>,
    credit_amount: Decimal,
) -> Result<(), ContractError> {
    let mut basket: Basket = BASKET.load(storage)?;

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
    let mut error: Option<StdError> = None;

    //Calculate debt per asset caps
    let cAsset_caps = get_basket_debt_caps(storage, querier, env, &mut basket)?;

    for i in 0..old_ratios.len() {
        match old_ratios[i].atomics().checked_sub(new_ratios[i].atomics()) {
            Ok(difference) => {
                //Old ratio was > than New
                basket.collateral_supply_caps = basket.clone().collateral_supply_caps
                    .into_iter() 
                    .map(|mut cap| {
                        if cap.asset_info.equal(&old_assets[i].asset.info) {
                            let debt_difference = match decimal_multiplication(Decimal::new(difference), credit_amount){
                                Ok(debt_difference) => {
                                    debt_difference
                                },
                                Err(e) => {
                                    error = Some(e);                                    
                                    Decimal::zero()
                                }
                            };
                            //So we subtract the % difference in debt from said asset
                            if let Ok(difference) = cap.debt_total.checked_sub( debt_difference * Uint128::new(1u128)) {
                                if cap.current_supply.is_zero() {
                                    //This removes rounding errors that would slowly increase resting interest rates
                                    //Doesn't effect checks for bad debt since its basket debt not position.credit_amount
                                    //its a .000001 error, so shouldn't effect overall calcs or be profitably spammable
                                    cap.debt_total = Uint128::zero();
                                } else {
                                    cap.debt_total = difference;
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

                basket.collateral_supply_caps = basket.clone().collateral_supply_caps
                    .into_iter()
                    .enumerate()
                    .map(|(index, mut cap)| {
                        if cap.asset_info.equal(&old_assets[i].asset.info) {
                            let debt_difference = match decimal_multiplication(difference, credit_amount){
                                Ok(debt_difference) => {
                                    debt_difference
                                },
                                Err(e) => {
                                    error = Some(e);
                                    Decimal::zero()
                                }
                            };
                            let asset_debt = debt_difference * Uint128::new(1u128);

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

    if let Some(error) = error{
        return Err(ContractError::Std(error));
    }

    if over_cap {
        return Err(ContractError::CustomError {
            val: format!("Assets over debt cap: {:?}", assets_over_cap),
        });
    }
    BASKET.save(storage, &basket)?;

    Ok(())
}

/// Update the distribution of Basket debt per asset after Position debt increases
pub fn update_basket_debt(
    storage: &dyn Storage,
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
    //Save the debt distribution per asset to a list
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
        basket.collateral_supply_caps = basket.clone().collateral_supply_caps
            .into_iter()
            .enumerate()
            .map(|(i, mut cap)| {
                //Add or subtract deposited amount to/from the correlated cAsset object
                if cap.asset_info.equal(&cAsset.asset.info) {
                    if add_to_debt {
                        //Assert its not over the cap
                        //IF the debt is adding to interest then we allow it to exceed the cap
                        if (cap.debt_total + asset_debt[index]) <= cAsset_caps[i] || interest_accrual {
                            cap.debt_total += asset_debt[index];
                        } else {
                            over_cap = true;
                            assets_over_cap.push(cap.asset_info.to_string());
                        }
                    } else if let Ok(difference) = cap.debt_total.checked_sub(asset_debt[index]) {
                        cap.debt_total = difference;
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

/// Get total amount of debt token in the Stability Pool
pub fn get_stability_pool_liquidity(
    querier: QuerierWrapper,
    config: Config,
) -> StdResult<Uint128> {
    if let Some(sp_addr) = config.stability_pool {
        //Query the SP Asset Pool
        Ok(querier
            .query::<AssetPool>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: sp_addr.to_string(),
                msg: to_binary(&SP_QueryMsg::AssetPool { 
                    user: None,
                    start_after: None,
                    deposit_limit: 1.into(),
                })?,
            }))?
            .credit_asset
            .amount)
    } else {
        Ok(Uint128::zero())
    }
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
        env,
        querier,
        basket.clone().collateral_types,
        config.clone(),
    )?;
    
    //Get the base debt cap
    let mut debt_cap =
        get_asset_liquidity(querier, config.clone().liquidity_contract.unwrap().to_string(), basket.clone().credit_asset.info)?
            * basket.liquidity_multiplier;

    //Get SP liquidity
    let sp_liquidity = get_stability_pool_liquidity(querier, config.clone())?;

    //Add SP liquidity to the cap
    debt_cap += Decimal::from_ratio(sp_liquidity, Uint128::new(1)) * Uint128::new(1);


    //If debt cap is less than the minimum, set it to the minimum
    if debt_cap < (config.base_debt_cap_multiplier * config.debt_minimum) {
        debt_cap = (config.base_debt_cap_multiplier * config.debt_minimum);
    }

     //Don't double count debt btwn Stability Pool based ratios and TVL based ratios
     for cap in basket.clone().collateral_supply_caps {
        //If the cap is based on sp_liquidity, subtract its value from the debt_cap
        if let Some(sp_ratio) = cap.stability_pool_ratio_for_debt_cap {
            debt_cap -= decimal_multiplication(Decimal::from_ratio(sp_liquidity, Uint128::new(1)), sp_ratio)? * Uint128::new(1);
        }
    }

    //Calc total basket debt
    let total_debt: Uint128 = basket.clone().collateral_supply_caps
        .into_iter()
        .map(|cap| cap.debt_total)
        .collect::<Vec<Uint128>>()
        .into_iter()
        .sum();

    //If the basket's proportion of it's debt cap is >= 1, negative rates are turned off
    //This protects against perpetual devaluing of the credit asset as Membrane is disincentivizing new debt w/ high rates
    //Note: This could be changed to "IF each asset's util is above desired"...
    //...but the current implementation turns them off faster, might as well be on the safe side
    if Decimal::from_ratio(total_debt, debt_cap) >= Decimal::one() {
        basket.negative_rates = false;
    }

    let mut per_asset_debt_caps = vec![];

    //Calc per asset debt caps
    for (i, cAsset_ratio) in cAsset_ratios.into_iter().enumerate() {
                
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
                    decimal_multiplication(Decimal::from_ratio(sp_liquidity, Uint128::new(1)), sp_ratio)? * Uint128::new(1)
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

