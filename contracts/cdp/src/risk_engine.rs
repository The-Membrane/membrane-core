use cosmwasm_std::{Decimal, Uint128, Env, QuerierWrapper, Storage, to_binary, QueryRequest, WasmQuery, StdResult, StdError, Addr};

use membrane::cdp::Config;
use membrane::stability_pool::QueryMsg as SP_QueryMsg;
use membrane::types::{Basket, Asset, cAsset, SupplyCap, AssetPool};
use membrane::helpers::{get_asset_liquidity, get_owner_liquidity_multiplier};
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

    
        //Assert new ratios aren't above Collateral Supply Caps. If so, error.
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
    }

    Ok(())
}

/// Update the distribution of Basket debt per asset after Position collateral makeup changes
pub fn update_debt_per_asset_in_position(
    storage: &mut dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    mut basket: Basket,
    old_assets: Vec<cAsset>,
    mut old_ratios: Vec<Decimal>,
    new_assets: Vec<cAsset>,
    mut new_ratios: Vec<Decimal>,
    credit_amount: Decimal,
) -> Result<(), ContractError> {
    //Note: Vec lengths need to match, enforced in withdraw()
    if old_ratios.is_empty() {
        let (ratios, _) = get_cAsset_ratios(
            storage,
            env.clone(),
            querier,
            old_assets.clone(),
            config.clone(),
        )?;
        old_ratios = ratios;
    }
    if new_ratios.is_empty() {
        let (ratios, _) = get_cAsset_ratios(
            storage,
            env.clone(),
            querier,
            new_assets.clone(),
            config.clone(),
        )?;
        new_ratios = ratios;
    }

    let mut error: Option<StdError> = None;

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
                            if let Ok(debt_difference) = cap.debt_total.checked_sub( debt_difference * Uint128::new(1u128)) {
                                if cap.current_supply.is_zero() {
                                    //This removes rounding errors that would slowly increase resting interest rates
                                    //Doesn't effect checks for bad debt since its basket debt not position.credit_amount
                                    //its a .000001 error, so shouldn't effect overall calcs or be profitably spammable
                                    cap.debt_total = Uint128::zero();
                                } else {
                                    cap.debt_total = debt_difference;
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
                    .map(|mut cap| {
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

                            //Add to debt total
                            cap.debt_total += asset_debt;
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

    BASKET.save(storage, &basket)?;

    Ok(())
}

/// Update the distribution of Basket debt per asset after Position debt updates
pub fn update_basket_debt(
    storage: &dyn Storage,
    env: Env,
    querier: QuerierWrapper,
    config: Config,
    basket: &mut Basket,
    collateral_assets: Vec<cAsset>,
    credit_amount: Uint128,
    add_to_debt: bool,
    mut cAsset_ratios: Vec<Decimal>,
) -> Result<(), ContractError> {
    
    if cAsset_ratios.is_empty() {
        let (ratios, _) = get_cAsset_ratios(
            storage,
            env.clone(),
            querier,
            collateral_assets.clone(),
            config,
        )?;
        cAsset_ratios = ratios;
    }

    let mut asset_debt = vec![];
    //Save the debt distribution per asset to a list
    for asset in cAsset_ratios {
        let distro = decimal_multiplication(asset, Decimal::from_ratio(credit_amount, Uint128::one()))?;
        asset_debt.push(distro.to_uint_floor());
    }

    //Update basket debt tally
    if add_to_debt {
        basket.credit_asset.amount += credit_amount;  
    } else {
        basket.credit_asset.amount = match basket.credit_asset.amount.checked_sub(credit_amount){
            Ok(diff) => diff,
            //Basket debt amount should always equal outstanding debt
            Err(_err) => return Err(ContractError::FaultyCalc { msg: "Basket debt amount should always equal outstanding debt".to_string() })
        };
    }

    let mut err = None;
    //Update supply caps w/ new debt distribution
    for (index, cAsset) in collateral_assets.iter().enumerate() {
        basket.collateral_supply_caps = basket.clone().collateral_supply_caps
            .into_iter()
            .map(|mut cap| {
                //Add or subtract deposited amount to/from the correlated cAsset object
                if cap.asset_info.equal(&cAsset.asset.info) {
                    if add_to_debt {
                        //It can go over the cap bc the interest rate will increase to disincentivize
                        cap.debt_total += asset_debt[index];

                    } else if let Ok(difference) = cap.debt_total.checked_sub(asset_debt[index]) {
                        cap.debt_total = difference;
                    } else {
                        //If ratios change between updates the debt cap could be less than the debt total allotted to said asset
                        cap.debt_total = Uint128::zero();
                    }
                }

                cap
            })
            .collect::<Vec<SupplyCap>>();       

    }

    if let Some(error) = err {
        return error
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
                    deposit_limit: 0.into(),
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
        Err(err) => return Err(StdError::GenericErr { msg: format!( 
            "Error getting owner liquidity parameters, line 350: {}", err
         )})
    };
    
    let liquidity = get_asset_liquidity(
        querier, 
        config.clone().liquidity_contract.unwrap().to_string(),
        basket.clone().credit_asset.info
    )?;
    
    //Get the base debt cap
    let mut debt_cap = decimal_multiplication(Decimal::from_ratio(liquidity, Uint128::one()), owner_params.0)? * Uint128::one();


    //Get SP cap space the contract is allowed to use
    let sp_liquidity = match get_stability_pool_liquidity(querier, config.clone()){
        Ok(liquidity) => liquidity,
        Err(err) => return Err(StdError::GenericErr { msg: format!( 
            "Error getting stability pool liquidity, line 368: {}", err
         )})
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