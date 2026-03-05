use cosmwasm_std::{
    attr, entry_point, to_json_binary, Addr, Binary, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, QueryRequest, Response, StdError, StdResult, Uint128, WasmMsg,
    WasmQuery, Order, Coin, BankMsg,
};
use cw2::set_contract_version;

use membrane::auction::{ExecuteMsg, InstantiateMsg, QueryMsg, Config, UpdateConfig, MigrateMsg};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};
use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::chain_proxy::ExecuteMsg as ChainProxyExecuteMsg;
use membrane::staking::ExecuteMsg as StakingExecuteMsg;
use membrane::cdp::{ExecuteMsg as CDPExecuteMsg, QueryMsg as CDPQueryMsg};
use membrane::types::{Asset, AssetInfo, RepayPosition, UserInfo, AuctionRecipient, Basket, DebtAuction, FeeAuction, MBRNSale};
use membrane::helpers::withdrawal_msg;
use membrane::revenue_distributor::ExecuteMsg as RevenueDistributorExecuteMsg;
use membrane::ltv_disco::ExecuteMsg as LtvDiscoExecuteMsg;

use crate::error::ContractError;
use crate::state::{CONFIG, DEBT_AUCTION, FEE_AUCTIONS, MBRN_SALE, OWNERSHIP_TRANSFER};

// Contract name and version used for migration. 
const CONTRACT_NAME: &str = "auctions";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const MAX_LIMIT: u64 = 31u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let mut config = Config {
        owner: info.sender,
        oracle_contract: deps.api.addr_validate(&msg.oracle_contract)?,
        osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
        mbrn_denom: msg.mbrn_denom,
        cdt_denom: String::new(),
        desired_asset: String::from("uosmo"),
        positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
        governance_contract: deps.api.addr_validate(&msg.governance_contract)?,
        staking_contract: deps.api.addr_validate(&msg.staking_contract)?,
        twap_timeframe: msg.twap_timeframe,
        initial_discount: msg.initial_discount,
        discount_increase_timeframe: msg.discount_increase_timeframe,
        discount_increase: msg.discount_increase,
        send_to_stakers: false,
        delay_window_minutes: msg.delay_window_minutes.unwrap_or(60u64),
        revenue_distributor_contract: msg.revenue_distributor_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
        ltv_disco_contract: msg.ltv_disco_contract
            .map(|addr| deps.api.addr_validate(&addr))
            .transpose()?,
    };

    if let Some(owner) = msg.owner {
        config.owner = deps.api.addr_validate(&owner)?
    }

    //Set CDT denom
    let basket: Basket = deps.querier.query_wasm_smart(
        config.clone().positions_contract, 
        &CDPQueryMsg::GetBasket{ })?;
        
    config.cdt_denom = basket.credit_asset.info.to_string();

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()    
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
    )
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::StartAuction {
            repayment_position_info,
            send_to,
            auction_asset,
            per_asset_distribution,
        } => start_auction(deps, env, info, repayment_position_info, send_to, auction_asset, per_asset_distribution),
        ExecuteMsg::SwapForMBRN { } => swap_for_mbrn(deps, info, env),
        ExecuteMsg::SwapForFee { auction_asset } => swap_with_the_contracts_desired_asset(deps, info, env, auction_asset),
        ExecuteMsg::RemoveAuction { } => remove_auction(deps, info),
        ExecuteMsg::StartMBRNSale { max_cdt } => start_mbrn_sale(deps, env, info, max_cdt),
        ExecuteMsg::BuySuppliedMBRN { } => buy_supplied_mbrn(deps, info, env),
        ExecuteMsg::UpdateConfig ( update)  => update_config( deps, info, update),
    }
}

/// Update contract configuration
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![attr("method", "update_config")];

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if let Ok(new_owner) = OWNERSHIP_TRANSFER.load(deps.storage) {
            if info.sender == new_owner {
                config.owner = info.sender;
            } else {
                return Err(ContractError::Unauthorized {});
            }
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    //Save optionals
    if let Some(addr) = update.owner {
        let valid_addr = deps.api.addr_validate(&addr)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));        
    }
    if let Some(addr) = update.oracle_contract {
        config.oracle_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.osmosis_proxy {
        config.osmosis_proxy = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.positions_contract {
        config.positions_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.governance_contract {
        config.governance_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = update.staking_contract {
        config.staking_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(mbrn_denom) = update.mbrn_denom {
        config.mbrn_denom = mbrn_denom;
    }
    if let Some(cdt_denom) = update.cdt_denom {
        config.cdt_denom = cdt_denom;
    }
    //Ensure desired asset has an oracle price to save
    if let Some(asset) = update.desired_asset {
        ///Get desired_asset price
        if let Ok(_) = deps.querier.query_wasm_smart::<Vec<PriceResponse>>(
            config.clone().oracle_contract.to_string(),
            &OracleQueryMsg::Price {
                asset_info: AssetInfo::NativeToken {
                    denom: asset.clone(),
                },
                twap_timeframe: 0,
                oracle_time_limit: 0,
                basket_id: None,
            }) {
                //Set desired asset
                config.desired_asset = asset;
            };    
    }
    if let Some(twap_timeframe) = update.twap_timeframe {
        //Enforce 1 hr - 8 hr timeframe
        if twap_timeframe < 60 || twap_timeframe > 480 {
            return Err(ContractError::CustomError { val: String::from("Invalid TWAP timeframe") });
        }
        config.twap_timeframe = twap_timeframe;
    }
    if let Some(initial_discount) = update.initial_discount {
        //Enforce 1% - 10% discount
        if initial_discount < Decimal::percent(1) || initial_discount > Decimal::percent(10) {
            return Err(ContractError::CustomError { val: String::from("Invalid initial discount") });
        }
        config.initial_discount = initial_discount;
    }
    if let Some(discount_increase_timeframe) = update.discount_increase_timeframe {
        //Enforce 10 sec - 300 sec timeframe
        if discount_increase_timeframe < 10 || discount_increase_timeframe > 300 {
            return Err(ContractError::CustomError { val: String::from("Invalid discount increase timeframe") });
        }
        config.discount_increase_timeframe = discount_increase_timeframe;
    }
    if let Some(discount_increase) = update.discount_increase {
        //Enforce 1% - 5% discount
        if discount_increase < Decimal::percent(1) || discount_increase > Decimal::percent(5) {
            return Err(ContractError::CustomError { val: String::from("Invalid discount increase") });
        }
        config.discount_increase = discount_increase;
    }
    if let Some(send_to_stakers) = update.send_to_stakers {
        config.send_to_stakers = send_to_stakers;
    }
    if let Some(delay_window_minutes) = update.delay_window_minutes {
        config.delay_window_minutes = delay_window_minutes;
    }
    if let Some(addr) = update.revenue_distributor_contract {
        config.revenue_distributor_contract = Some(deps.api.addr_validate(&addr)?);
    }
    if let Some(addr) = update.ltv_disco_contract {
        config.ltv_disco_contract = Some(deps.api.addr_validate(&addr)?);
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    attrs.push(attr("updated_config", format!("{:?}", config)));

    Ok(Response::new().add_attributes(attrs))
}

/// Start or add to ongoing Auction.
/// Auctions have set recaptilization limits and can automatically repay for CDP Positions or send funds to an arbitrary address.
/// If non-CDT asset is sent, a burn auction is initiated.
fn start_auction(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user_info: Option<UserInfo>,
    send_to: Option<String>,
    mut auction_asset: Asset,
    per_asset_distribution: Option<Vec<Asset>>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Only positions contract or owner can start auctions
    if info.sender != config.owner && info.sender != config.positions_contract && info.sender != config.staking_contract {
        return Err(ContractError::Unauthorized {});
    }

    //Attributes
    let mut attrs = vec![
        attr("method", "start_auction"),
        attr("auction_asset", auction_asset.to_string()),
    ];
    
    //If not minting CDT, start FeeAuction
    if info.funds.len() > 0 {
        //Validate auction_asset
        if info.funds.len() == 1 {
            validate_asset(info.funds[0].clone(), auction_asset.info.to_string())?;
            auction_asset.amount = info.funds[0].clone().amount;
        } else { return Err(ContractError::CustomError { val: String::from("Must start only one auction & fees must be sent with intiation") }) }

        FEE_AUCTIONS.update(deps.storage, auction_asset.info.to_string(), |fee_auction| -> StdResult<FeeAuction> {
            match fee_auction {
                Some(mut auction) => {
                    //If Some, add to Auction asset amount
                    auction.auction_asset.amount += auction_asset.clone().amount;
                    // Merge per_asset_distribution if both exist
                    if let Some(new_dist) = per_asset_distribution.clone() {
                        if let Some(existing_dist) = &mut auction.per_asset_distribution {
                            // Merge: add amounts for matching assets, append new ones
                            for new_asset in new_dist {
                                if let Some(existing) = existing_dist.iter_mut()
                                    .find(|a| a.info == new_asset.info) {
                                    existing.amount += new_asset.amount;
                                } else {
                                    existing_dist.push(new_asset);
                                }
                            }
                        } else {
                            auction.per_asset_distribution = Some(new_dist);
                        }
                    }

                    Ok(auction)
                },
                None => {
                    //If None, create new auction               
                    Ok(FeeAuction {
                        auction_asset,
                        auction_start_time: env.block.time.seconds(),
                        per_asset_distribution: per_asset_distribution.clone(),
                    })
                }
            }
        })?;
    } //If CDT, start DebtAuction
    else if auction_asset.info.to_string() == config.clone().cdt_denom {

        //Both can't be Some
        if send_to.is_some() && user_info.is_some(){
            return Err(ContractError::CustomError { val: String::from("Delegate auction proceeds to one party at a time") })
        }

        //Set send_to Address
        let mut send_addr = Addr::unchecked("");
        if let Some(string) = send_to.clone() {
            send_addr = deps.api.addr_validate(&string)?;
        }

        //Update DebtAuctions
        match DEBT_AUCTION.load(deps.storage){
            //Add debt_amount and repayment info to the auction
            Ok(mut auction) => {

                auction.remaining_recapitalization += auction_asset.clone().amount;

                if send_to.is_some() {
                    auction.send_to.push(
                        AuctionRecipient {
                            amount: auction_asset.clone().amount,
                            recipient: send_addr,
                        });
                }

                if let Some(user_info) = user_info {                        
                    auction.repayment_positions.push(
                        RepayPosition {
                            repayment: auction_asset.clone().amount,
                            position_info: user_info,
                        });
                }

                attrs.push(attr("auction_status", "added_to"));

                //Save new DebtAuction
                DEBT_AUCTION.save(deps.storage, &auction)?;
            }
            //Add new auction
            Err(_) => {
                attrs.push(attr("auction_status", "started_anew"));

                let mut auction = DebtAuction {
                    remaining_recapitalization: auction_asset.clone().amount,
                    repayment_positions: vec![],
                    send_to: vec![],
                    auction_start_time: env.block.time.seconds(),
                };

                if send_to.is_some() {
                    auction.send_to.push(
                        AuctionRecipient {
                            amount: auction_asset.clone().amount,
                            recipient: send_addr,
                        });
                }

                if let Some(user_info) = user_info {                        
                    auction.repayment_positions.push(
                        RepayPosition {
                            repayment: auction_asset.clone().amount,
                            position_info: user_info,
                        });
                }

                //Save new DebtAuction
                DEBT_AUCTION.save(deps.storage, &auction)?;
            }
        };
    }


    Ok(Response::new().add_attributes(attrs))
}

/// Remove DebtAuction
fn remove_auction(
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Only positions contract or owner can remove auctions
    if info.sender != config.owner && info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }

    let attrs = vec![
        attr("method", "remove_auction"),
    ];

    //Update Auctions
    DEBT_AUCTION.remove(deps.storage);

    Ok(Response::new().add_attributes(attrs))
}

/// Validate asset and assert amount is > 0
fn validate_asset(
    coin: Coin,
    valid_denom: String
)-> StdResult<Coin>{
    if coin.denom != valid_denom {
        return Err(StdError::generic_err(format!("Invalid asset ({}) sent to fulfill auction. Must be {}", coin.denom, valid_denom)));
    }

    if coin.amount.is_zero() {
        return Err(StdError::generic_err("Amount must be greater than 0"));
    }
    
    Ok(coin)
}

/// Swap desired asset for Some(fee_asset) at a discount
/// Send desired asset to governance or stakers and send Some(fee_asset) to the sender.
fn swap_with_the_contracts_desired_asset(deps: DepsMut, info: MessageInfo, env: Env, auction_asset: AssetInfo) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let mut overpay = Uint128::zero();
    let successful_swap_amount;

    let mut msgs: Vec<CosmosMsg> = vec![];
    let mut attrs = vec![attr("method", "swap_with_contract_desired_asset")];
    
    //Validate MBRN send
    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err("Only one coin can be sent")));
    }
    let coin = validate_asset(info.funds[0].clone(), config.clone().desired_asset)?;

    //Get FeeAuction
    let mut auction = FEE_AUCTIONS.load(deps.storage, auction_asset.clone().to_string())?;

    // Check delay based on desired_asset: 0 for MBRN, configured delay for others
    let delay_window_seconds = if config.desired_asset == config.mbrn_denom {
        0u64
    } else {
        //Delay minutes to seconds
        config.delay_window_minutes * 60
    };
    let earliest_swap_time = auction.auction_start_time + delay_window_seconds;
    if env.block.time.seconds() < earliest_swap_time {
        return Err(ContractError::Std(StdError::generic_err(
            format!("Auction delay not passed. Can swap at timestamp: {}", earliest_swap_time)
        )));
    }

    //If the auction is active, i.e. there is still debt to be repaid & auction has started
    //...swap for auctioned asset
    if !auction.auction_asset.amount.is_zero() && auction.auction_start_time <= env.block.time.seconds() {

        //Get desired_asset price
        let desired_res: Vec<PriceResponse> = deps.querier.query_wasm_smart(
            config.clone().oracle_contract.to_string(), 
        &OracleQueryMsg::Price {
                asset_info: AssetInfo::NativeToken {
                    denom: config.clone().desired_asset,
                },
                twap_timeframe: config.clone().twap_timeframe,
                oracle_time_limit: 600,
                basket_id: None,
            })?;
            
        //Get value of sent desired asset
        let desired_asset_value = desired_res[0].get_value(coin.amount)?;
                
        //Get auction asset price
        let mut auction_res: Vec<PriceResponse> = deps.querier.query_wasm_smart(
            config.clone().oracle_contract.to_string(), 
            &OracleQueryMsg::Price {
                    asset_info: AssetInfo::NativeToken {
                        denom: auction.auction_asset.info.to_string(),
                    },
                    twap_timeframe: config.clone().twap_timeframe,
                    oracle_time_limit: 600,
                    basket_id: None,
                })?;      
        //Get value of auction asset
        let mut auction_asset_value = auction_res[0].get_value(auction.auction_asset.amount)?;
        
        //Get discount
        let discount_ratio = get_discount_ratio(env.clone(), auction.clone().auction_start_time, config.clone())?;
        
        //Incorporate discount to auction asset value
        auction_asset_value = decimal_multiplication(auction_asset_value, discount_ratio)?;

        //Get successful_swap_amount
        //If the value of the sent desired_Asset is greater than the value of the auction asset, set overpay amount
        //Zero auction asset amount
        if desired_asset_value > auction_asset_value {

            //Calc overpay amount in desired_asset
            overpay = desired_res[0].get_amount((desired_asset_value - auction_asset_value))?;

            successful_swap_amount = auction.auction_asset.amount;
            auction.auction_asset.amount = Uint128::zero();
            
            //Delete Auction
            FEE_AUCTIONS.remove(deps.storage, auction_asset.clone().to_string());

        } else if desired_asset_value < auction_asset_value {
            /////If the value of the sent desired_Asset is less than the value of the auction asset, set successful_swap_amount
            //Set auction_price to the discounted price
            let discounted_auction_price = decimal_multiplication(auction_res[0].price, discount_ratio)?;
            auction_res[0].price = discounted_auction_price;

            //Update auction asset amount
            successful_swap_amount = auction_res[0].get_amount(desired_asset_value)?;
            auction.auction_asset.amount = auction_res[0].get_amount(auction_asset_value - desired_asset_value)?;
            
            //Update Auction
            FEE_AUCTIONS.save(deps.storage, auction_asset.clone().to_string(), &auction)?;
        } else {
            successful_swap_amount = auction.auction_asset.amount;
            auction.auction_asset.amount = Uint128::zero();
            
            //Delete Auction
            FEE_AUCTIONS.remove(deps.storage, auction_asset.clone().to_string());
        }

        //Send desired asset based on type
        let proceeds_amount = coin.amount - overpay;
        if proceeds_amount > Uint128::zero() {
            if config.desired_asset == config.mbrn_denom {
                // MBRN proceeds go to LTV Disco with per_asset_distribution
                if let Some(ltv_disco) = config.ltv_disco_contract.clone() {
                    msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: ltv_disco.to_string(),
                        msg: to_json_binary(&LtvDiscoExecuteMsg::AddDepositTokenRevenue {
                            per_asset_distribution: auction.per_asset_distribution.clone().unwrap_or_default(),
                        })?,
                        funds: vec![Coin {
                            denom: config.mbrn_denom.clone(),
                            amount: proceeds_amount,
                        }],
                    }));
                }
            } else if config.desired_asset == config.cdt_denom {
                // CDT proceeds go to revenue distributor with per_asset_distribution
                if let Some(revenue_distributor) = config.revenue_distributor_contract.clone() {
                    msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: revenue_distributor.to_string(),
                        msg: to_json_binary(&RevenueDistributorExecuteMsg::SetPromises {
                            promises: vec![],
                            ltv_disco_distribution: auction.per_asset_distribution.clone(),
                        })?,
                        funds: vec![Coin {
                            denom: config.cdt_denom.clone(),
                            amount: proceeds_amount,
                        }],
                    }));
                }
            } else if config.send_to_stakers {
                //Staking DepositFee (fallback for other desired assets)
                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.clone().staking_contract.to_string(),
                    msg: to_json_binary(&StakingExecuteMsg::DepositFee { })?,
                    funds: vec![Coin {
                        denom: config.clone().desired_asset,
                        amount: proceeds_amount,
                    }],
                }));
            } else {
                //Governance (fallback)
                msgs.push(CosmosMsg::Bank(BankMsg::Send {
                    to_address: config.clone().governance_contract.to_string(),
                    amount: vec![Coin {
                        denom: config.clone().desired_asset,
                        amount: proceeds_amount,
                    }],
                }));
            }
        }

        //Send fee asset to the sender
        msgs.push(CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: auction.auction_asset.info.to_string(),
                amount: successful_swap_amount,
            }],
        }));

        //Update or Remove Auction
        if auction.auction_asset.amount <= Uint128::one() {
            FEE_AUCTIONS.remove(deps.storage, auction_asset.clone().to_string());
        } else {
            FEE_AUCTIONS.save(deps.storage, auction_asset.clone().to_string(), &auction)?;
        }

        //If there is overpay, send it back to the sender
        if !overpay.is_zero() {
            msgs.push(CosmosMsg::Bank(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![Coin {
                    denom: coin.denom,
                    amount: overpay,
                }],
            }));
        }

        attrs.push(attr("auction_asset", auction.auction_asset.to_string()));
    } else {
        return Err(ContractError::Std(StdError::generic_err("Auction isn't running now")));
    }

    Ok(Response::new().add_messages(msgs).add_attributes(attrs))
}


/// Get swap discount based on time elapsed since auction start (after delay window)
fn get_discount_ratio(
    env: Env,
    auction_start_time: u64,
    config: Config,
) -> StdResult<Decimal> {
    // If desired_asset is MBRN, use 0 delay; otherwise use configured delay
    let delay_window_seconds = if config.desired_asset == config.mbrn_denom {
        0u64
    } else {
        //Delay minutes to seconds
        config.delay_window_minutes * 60
    };
    
    // Calculate time elapsed AFTER the delay window has passed
    // If still in delay window, time_elapsed_for_discount = 0
    let time_since_start = env.block.time.seconds().saturating_sub(auction_start_time);
    let time_elapsed = time_since_start.saturating_sub(delay_window_seconds);

    //Get discount based on elapsed time (after delay)
    let discount_multiplier = time_elapsed / config.discount_increase_timeframe;
    let current_discount_increase = decimal_multiplication(
        Decimal::from_ratio(
            Uint128::new(discount_multiplier.into()),
            Uint128::new(1u128),
        ),
        config.discount_increase,
    )?;

    //Ensure discount is not greater than 1
    let current_discount = if (current_discount_increase + config.initial_discount) > Decimal::one() {
        Decimal::one()
    } else {
        current_discount_increase + config.initial_discount
    };

    //Maximum discount of 99%
    let discount_ratio = decimal_subtraction(
        Decimal::one(),
        current_discount,
    )?.max(Decimal::percent(1));
    
    Ok(discount_ratio)
}

/// Calculate MBRN amount for a given CDT amount using oracle pricing and discount.
/// Shared helper used by both `swap_for_mbrn` (debt auction) and `buy_supplied_mbrn` (MBRN sale).
///
/// Logic: MBRN oracle price → basket credit price → discount ratio → discounted price → MBRN amount
fn calculate_mbrn_for_cdt(
    deps: &DepsMut,
    env: &Env,
    config: &Config,
    cdt_amount: Uint128,
    auction_start_time: u64,
) -> Result<Uint128, ContractError> {
    // Get MBRN price (TWAP)
    let res: Vec<PriceResponse> = deps.querier.query_wasm_smart(
        config.oracle_contract.to_string(),
        &OracleQueryMsg::Price {
            asset_info: AssetInfo::NativeToken {
                denom: config.mbrn_denom.clone(),
            },
            twap_timeframe: config.twap_timeframe,
            oracle_time_limit: 600,
            basket_id: None,
        },
    )?;
    let mbrn_price = res[0].price;

    // Get credit price at peg to further incentivize recapitalization
    let basket = deps
        .querier
        .query::<Basket>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.positions_contract.to_string(),
            msg: to_json_binary(&CDPQueryMsg::GetBasket {})?,
        }))?;
    let basket_credit_price = basket.credit_price;

    // Get discount
    let discount_ratio = get_discount_ratio(env.clone(), auction_start_time, config.clone())?;

    // Calculate discounted MBRN price
    let discounted_mbrn_price = decimal_multiplication(mbrn_price, discount_ratio)?;
    if discounted_mbrn_price.is_zero() {
        return Err(ContractError::Std(StdError::generic_err("Discounted MBRN price is zero")));
    }

    let credit_value = basket_credit_price.get_value(cdt_amount)?;
    let mbrn_amount = decimal_division(credit_value, discounted_mbrn_price)?;

    Ok(mbrn_amount.to_uint_floor())
}

/// Swap the debt asset in the ongoing auction for MBRN at a discount.
/// Handle Position repayments and arbitrary sends.
/// Excess swap amount is returned to the sender.
fn swap_for_mbrn(deps: DepsMut, info: MessageInfo, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let mut msgs: Vec<CosmosMsg> = vec![];
    let mut attrs = vec![attr("method", "swap_for_mbrn")];

    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err("Only one coin can be sent")));
    }
    let coin = validate_asset(info.funds[0].clone(), config.clone().cdt_denom)?;

    //Get DebtAuction
    let mut auction = DEBT_AUCTION.load(deps.storage)?;

    // Check delay based on desired_asset: 0 for MBRN, configured delay for others
    // For debt auctions, users pay CDT and get MBRN (minted), so desired_asset check applies
    let delay_window_seconds = if config.desired_asset == config.mbrn_denom {
        0u64
    } else {
        //Delay minutes to seconds
        config.delay_window_minutes * 60
    };
    let earliest_swap_time = auction.auction_start_time + delay_window_seconds;
    if env.block.time.seconds() < earliest_swap_time {
        return Err(ContractError::Std(StdError::generic_err(
            format!("Auction delay not passed. Can swap at timestamp: {}", earliest_swap_time)
        )));
    }

    //If the auction is active, i.e. there is still debt to be repaid, swap for MBRN
    if !auction.remaining_recapitalization.is_zero() {

        // Calculate MBRN amount using shared pricing helper
        let mbrn_mint_amount = calculate_mbrn_for_cdt(&deps, &env, &config, coin.amount, auction.auction_start_time)?;

        //Ensure MBRN mint amount is not zero
        if mbrn_mint_amount.is_zero() {
            return Err(ContractError::Std(StdError::generic_err("MBRN mint amount is zero")));
        }
        //Else
        let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().osmosis_proxy.to_string(),
            msg: to_json_binary(& ChainProxyExecuteMsg::MintTokens {
                denom: config.clone().mbrn_denom,
                amount: mbrn_mint_amount,
                mint_to_address: info.clone().sender.to_string(),
            })?,
            funds: vec![],
        });
        msgs.push(message);

        attrs.push(attr(
            "mbrn_minted",
            format!(
                "Swapped Asset: {}, MBRN Minted: {}",
                coin.denom, mbrn_mint_amount
            ),
        ));

        // Determine how much of the sent CDT fulfills pending recapitalization
        let fulfill_amount = if coin.amount >= auction.remaining_recapitalization {
            auction.remaining_recapitalization
        } else {
            coin.amount
        };

        // Send fulfilled CDT to the CDP to burn via FulfillBadDebt
        if !fulfill_amount.is_zero() {
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.clone().positions_contract.to_string(),
                msg: to_json_binary(&CDPExecuteMsg::FulfillBadDebt {})?,
                funds: vec![Coin { denom: coin.clone().denom, amount: fulfill_amount }],
            }));

            attrs.push(attr("fulfilled_bad_debt", fulfill_amount));
        }

        // Any remaining CDT is overpay and should be returned
        let overpay = match coin.amount.checked_sub(fulfill_amount) {
            Ok(val) => val,
            Err(_) => Uint128::zero(),
        };

        // Update remaining recapitalization
        auction.remaining_recapitalization = match auction.remaining_recapitalization.checked_sub(fulfill_amount) {
            Ok(val) => val,
            Err(_) => Uint128::zero(),
        };

        //Send back overpayment
        if !overpay.is_zero() {
            //Create msg
            msgs.push(withdrawal_msg(
                Asset {
                    info: AssetInfo::NativeToken {
                        denom: coin.clone().denom,
                    },
                    amount: overpay,
                },
                info.clone().sender,
            )?);
        }
    } else {
        return Err(ContractError::Std(StdError::generic_err("Auction ended")));
    }

    //Update or Remove DebtAuction
    if auction.remaining_recapitalization.is_zero() {
        DEBT_AUCTION.remove(deps.storage);
    } else {
        DEBT_AUCTION.save(deps.storage, &auction)?;
    }

    Ok(Response::new().add_messages(msgs))
}

/// Start an MBRN sale for bad debt coverage.
/// Called by ltv_disco (no funds sent). Auction pulls MBRN from disco on demand.
fn start_mbrn_sale(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    max_cdt: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only ltv_disco can start MBRN sales
    if let Some(ltv_disco) = &config.ltv_disco_contract {
        if info.sender != *ltv_disco {
            return Err(ContractError::Unauthorized {});
        }
    } else {
        return Err(ContractError::CustomError {
            val: String::from("ltv_disco_contract not configured"),
        });
    }

    if max_cdt.is_zero() {
        return Err(ContractError::CustomError {
            val: String::from("max_cdt must be greater than zero"),
        });
    }

    // No funds expected (pull model — disco retains MBRN)
    if !info.funds.is_empty() {
        return Err(ContractError::CustomError {
            val: String::from("No funds should be sent; MBRN is pulled on demand"),
        });
    }

    // Create or add to existing MBRN sale
    match MBRN_SALE.load(deps.storage) {
        Ok(mut sale) => {
            sale.max_cdt += max_cdt;
            // Keep existing auction_start_time to maintain discount progression
            MBRN_SALE.save(deps.storage, &sale)?;
        }
        Err(_) => {
            let sale = MBRNSale {
                max_cdt,
                cdt_fulfilled: Uint128::zero(),
                auction_start_time: env.block.time.seconds(),
                supplier: info.sender.clone(),
            };
            MBRN_SALE.save(deps.storage, &sale)?;
        }
    }

    Ok(Response::new()
        .add_attribute("method", "start_mbrn_sale")
        .add_attribute("max_cdt", max_cdt)
        .add_attribute("supplier", info.sender))
}

/// Buy MBRN from the disco supply sale by sending CDT.
/// CDT goes to CDP.FulfillBadDebt, buyer receives MBRN at discount.
/// Auction pulls MBRN from disco via SendMBRNForSale.
fn buy_supplied_mbrn(deps: DepsMut, info: MessageInfo, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut msgs: Vec<CosmosMsg> = vec![];
    let mut attrs = vec![attr("method", "buy_supplied_mbrn")];

    // Validate CDT sent
    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err("Only one coin can be sent")));
    }
    let coin = validate_asset(info.funds[0].clone(), config.cdt_denom.clone())?;

    // Load MBRN sale
    let mut sale = MBRN_SALE.load(deps.storage)?;

    let remaining_cdt_needed = sale.max_cdt.checked_sub(sale.cdt_fulfilled)
        .unwrap_or(Uint128::zero());
    if remaining_cdt_needed.is_zero() {
        return Err(ContractError::Std(StdError::generic_err("MBRN sale is complete")));
    }

    // Query disco's available MBRN balance
    let disco_mbrn_balance: Coin = deps.querier.query_balance(
        sale.supplier.clone(),
        config.mbrn_denom.clone(),
    )?;
    if disco_mbrn_balance.amount.is_zero() {
        return Err(ContractError::Std(StdError::generic_err("No MBRN available in disco")));
    }

    // Cap CDT at remaining needed
    let effective_cdt = std::cmp::min(coin.amount, remaining_cdt_needed);

    // Calculate MBRN amount using shared pricing helper
    let mbrn_amount = calculate_mbrn_for_cdt(&deps, &env, &config, effective_cdt, sale.auction_start_time)?;
    if mbrn_amount.is_zero() {
        return Err(ContractError::Std(StdError::generic_err("MBRN amount is zero")));
    }

    // Cap MBRN at what disco actually has
    let actual_mbrn = std::cmp::min(mbrn_amount, disco_mbrn_balance.amount);

    // If MBRN was capped, proportionally reduce CDT taken
    let actual_cdt = if actual_mbrn < mbrn_amount {
        effective_cdt.multiply_ratio(actual_mbrn, mbrn_amount)
    } else {
        effective_cdt
    };
    let total_refund = coin.amount.checked_sub(actual_cdt).unwrap_or(Uint128::zero());

    // Update sale state
    sale.cdt_fulfilled += actual_cdt;

    // Pull MBRN from disco → send to buyer
    msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: sale.supplier.to_string(),
        msg: to_json_binary(&LtvDiscoExecuteMsg::SendMBRNForSale {
            amount: actual_mbrn,
            recipient: info.sender.to_string(),
        })?,
        funds: vec![],
    }));

    // Send CDT to CDP FulfillBadDebt
    if !actual_cdt.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.positions_contract.to_string(),
            msg: to_json_binary(&CDPExecuteMsg::FulfillBadDebt {})?,
            funds: vec![Coin { denom: config.cdt_denom.clone(), amount: actual_cdt }],
        }));
    }

    // Refund unused CDT to buyer
    if !total_refund.is_zero() {
        msgs.push(withdrawal_msg(
            Asset {
                info: AssetInfo::NativeToken {
                    denom: config.cdt_denom.clone(),
                },
                amount: total_refund,
            },
            info.sender.clone(),
        )?);
    }

    attrs.push(attr("cdt_received", actual_cdt));
    attrs.push(attr("mbrn_sold", actual_mbrn));
    attrs.push(attr("buyer", info.sender.clone()));

    // Check if sale is complete
    if sale.cdt_fulfilled >= sale.max_cdt {
        MBRN_SALE.remove(deps.storage);
        attrs.push(attr("sale_status", "completed"));
    } else {
        MBRN_SALE.save(deps.storage, &sale)?;
    }

    Ok(Response::new().add_messages(msgs).add_attributes(attrs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::DebtAuction {} => to_json_binary(&DEBT_AUCTION.load(deps.storage)?),
        QueryMsg::MBRNSale {} => to_json_binary(&MBRN_SALE.may_load(deps.storage)?),
        QueryMsg::OngoingFeeAuctions { auction_asset, limit, start_after } => {
            to_json_binary(&get_ongoing_fee_auctions(
                deps,
                auction_asset,
                limit,
                start_after,
            )?)
        }
    }
}

/// Return FeeAuction info
fn get_ongoing_fee_auctions(
    deps: Deps,
    auction_asset: Option<AssetInfo>,
    limit: Option<u64>,
    start_after: Option<u64>,
) -> StdResult<Vec<FeeAuction>> {
    //If querying a specific auction
    if let Some(auction_asset) = auction_asset {
        if let Ok(auction) = FEE_AUCTIONS.load(deps.storage, auction_asset.to_string()) {
            //Zeroed auctions are removed ahead of time
            Ok(vec![auction.clone()])
            
        } else {
            Err(StdError::generic_err(format!("Auction asset: {}, doesn't have an ongoing auction", auction_asset)))
        }
    } else {
        let limit: u64 = limit.unwrap_or(MAX_LIMIT);

        let mut resp = vec![];

        for asset in FEE_AUCTIONS.keys(deps.storage, None, None, Order::Ascending) {
            let asset = asset?;

            //Load auction
            if let Ok(auction) = FEE_AUCTIONS.load(deps.storage, asset.to_string()) {
                //Add Response
                //Zeroed auctions are removed ahead of time
                resp.push( auction.clone() );
                
            } else {
                return Err(StdError::generic_err(format!("Invalid auction swap asset: {}", asset)));
            }
        }
        match start_after {
            Some(index) => {                
                let _ = resp.split_off(index as usize);
            },
            None => {},
        };

        let resp = resp.into_iter().take(limit as usize).collect();

        Ok(resp)
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}