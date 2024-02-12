use cosmwasm_std::{
    attr, coins, entry_point, to_binary, Addr, Binary, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, QueryRequest, Response, StdError, StdResult, Uint128, WasmMsg,
    WasmQuery, Order, Coin, BankMsg,
};
use cw2::set_contract_version;

use membrane::auction::{ExecuteMsg, InstantiateMsg, QueryMsg, Config, UpdateConfig};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};
use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::staking::ExecuteMsg as StakingExecuteMsg;
use membrane::cdp::{ExecuteMsg as CDPExecuteMsg, QueryMsg as CDPQueryMsg};
use membrane::types::{Asset, AssetInfo, RepayPosition, UserInfo, AuctionRecipient, Basket, DebtAuction, FeeAuction};
use membrane::helpers::withdrawal_msg;

use crate::error::ContractError;
use crate::state::{CONFIG, DEBT_AUCTION, FEE_AUCTIONS, OWNERSHIP_TRANSFER};

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
        } => start_auction(deps, env, info, repayment_position_info, send_to, auction_asset),
        ExecuteMsg::SwapForMBRN { } => swap_for_mbrn(deps, info, env),
        ExecuteMsg::SwapForFee { auction_asset } => swap_with_the_contracts_desired_asset(deps, info, env, auction_asset),
        ExecuteMsg::RemoveAuction { } => remove_auction(deps, info),
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
        if let Ok(_) = deps.querier.query_wasm_smart::<PriceResponse>(
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

                    Ok(auction)
                },
                None => {
                    //If None, create new auction               
                    Ok(FeeAuction {
                        auction_asset,
                        auction_start_time: env.block.time.seconds(),
                    })
                }
            }
        })?;
    } //If CDT, start DebtAuction
    else if auction_asset.info.to_string() == config.clone().cdt_denom{

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
        return Err(StdError::GenericErr{msg: format!("Invalid asset ({}) sent to fulfill auction. Must be {}", coin.denom, valid_denom)});
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
        return Err(ContractError::Std(StdError::GenericErr { msg: String::from("Only one coin can be sent") }));
    }
    let coin = validate_asset(info.funds[0].clone(), config.clone().desired_asset)?;

    //Get FeeAuction
    let mut auction = FEE_AUCTIONS.load(deps.storage, auction_asset.clone().to_string())?;

    //If the auction is active, i.e. there is still debt to be repaid & auction has started
    //...swap for auctioned asset
    if !auction.auction_asset.amount.is_zero() && auction.auction_start_time <= env.block.time.seconds() {

        //Get desired_asset price
        let desired_res: PriceResponse = deps.querier.query_wasm_smart(
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
        let desired_asset_value = desired_res.get_value(coin.amount)?;
                
        //Get auction asset price
        let mut auction_res: PriceResponse = deps.querier.query_wasm_smart(
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
        let mut auction_asset_value = auction_res.get_value(auction.auction_asset.amount)?;
        
        //Get discount
        let discount_ratio = get_discount_ratio(env.clone(), auction.clone().auction_start_time, config.clone())?;
        //Incorporate discount to auction asset value
        auction_asset_value = decimal_multiplication(auction_asset_value, discount_ratio)?.floor();

        //Get successful_swap_amount
        //If the value of the sent desired_Asset is greater than the value of the auction asset, set overpay amount
        //Zero auction asset amount
        if desired_asset_value > auction_asset_value {

            //Calc overpay amount in desired_asset
            overpay = desired_res.get_amount((desired_asset_value - auction_asset_value))?;

            successful_swap_amount = auction.auction_asset.amount;
            auction.auction_asset.amount = Uint128::zero();

            //Delete Auction
            FEE_AUCTIONS.remove(deps.storage, auction_asset.clone().to_string());

        } else if desired_asset_value < auction_asset_value {
            /////If the value of the sent desired_Asset is less than the value of the auction asset, set successful_swap_amount
            //Set auction_price to the discounted price
            let discounted_auction_price = decimal_multiplication(auction_res.price, discount_ratio)?;
            auction_res.price = discounted_auction_price;

            //Update auction asset amount
            successful_swap_amount = auction_res.get_amount(desired_asset_value)?;
            auction.auction_asset.amount = auction_res.get_amount(auction_asset_value - desired_asset_value)?;
            
            //Update Auction
            FEE_AUCTIONS.save(deps.storage, auction_asset.clone().to_string(), &auction)?;
        } else {
            successful_swap_amount = auction.auction_asset.amount;
            auction.auction_asset.amount = Uint128::zero();

            //Delete Auction
            FEE_AUCTIONS.remove(deps.storage, auction_asset.clone().to_string());
        }

        //Send desired asset to Governance or deposit to Stakers
        if coin.amount - overpay > Uint128::zero(){
            if config.send_to_stakers {
                //Staking DepositFee
                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.clone().staking_contract.to_string(),
                    msg: to_binary(&StakingExecuteMsg::DepositFee { })?,
                    funds: vec![Coin {
                        denom: config.clone().desired_asset,
                        amount: coin.amount - overpay,
                    }],
                }));

            } else {
                //Governance
                msgs.push(CosmosMsg::Bank(BankMsg::Send {
                    to_address: config.clone().governance_contract.to_string(),
                    amount: vec![Coin {
                        denom: config.clone().desired_asset,
                        amount: coin.amount - overpay,
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
        return Err(ContractError::Std(StdError::GenericErr { msg: String::from("Auction isn't running now") }));
    }

    Ok(Response::new().add_messages(msgs).add_attributes(attrs))
}


/// Get swap discount based on time elapsed since auction start
fn get_discount_ratio(
    env: Env,
    auction_start_time: u64,
    config: Config,
) -> StdResult<Decimal> {

    //Get discount
    let time_elapsed = env.block.time.seconds() - auction_start_time;
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

/// Swap the debt asset in the ongoing auction for MBRN at a discount.
/// Handle Position repayments and arbitrary sends.
/// Excess swap amount is returned to the sender.
fn swap_for_mbrn(deps: DepsMut, info: MessageInfo, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let mut overpay = Uint128::zero();

    let mut msgs: Vec<CosmosMsg> = vec![];
    let mut attrs = vec![attr("method", "swap_for_mbrn")];

    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::GenericErr { msg: String::from("Only one coin can be sent") }));
    }
    let coin = validate_asset(info.funds[0].clone(), config.clone().cdt_denom)?;

    //Get DebtAuction
    let mut auction = DEBT_AUCTION.load(deps.storage)?;

    //If the auction is active, i.e. there is still debt to be repaid, swap for MBRN
    if !auction.remaining_recapitalization.is_zero() {

        let swap_amount = Decimal::from_ratio(coin.amount, Uint128::new(1u128));                

        let res: PriceResponse = deps.querier.query_wasm_smart(
            config.clone().oracle_contract.to_string(), 
        &OracleQueryMsg::Price {
                asset_info: AssetInfo::NativeToken {
                    denom: config.clone().mbrn_denom,
                },
                twap_timeframe: config.clone().twap_timeframe,
                oracle_time_limit: 600,
                basket_id: None,
            })?;
        let mbrn_price = res.price;

        //Get credit price at peg to further incentivize recapitalization
        let basket_credit_price = deps
            .querier
            .query::<Basket>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().positions_contract.to_string(),
                msg: to_binary(&CDPQueryMsg::GetBasket { })?,
            }))?
            .credit_price;

        //Get discount
        let discount_ratio = get_discount_ratio(env, auction.auction_start_time, config.clone())?;

        //Mint MBRN for user
        let discounted_mbrn_price = decimal_multiplication(mbrn_price, discount_ratio)?;
        let credit_value = basket_credit_price.get_value(swap_amount.to_uint_floor())?;
        let mbrn_mint_amount =
            decimal_division(credit_value, discounted_mbrn_price)? * Uint128::new(1u128);

        let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().osmosis_proxy.to_string(),
            msg: to_binary(&OsmoExecuteMsg::MintTokens {
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
        
        let mut swap_amount: Uint128 = swap_amount * Uint128::new(1u128);

        //Calculate what positions can be repaid for
        for (i, position) in auction.repayment_positions.clone().into_iter().enumerate() {
            if !position.repayment.is_zero() && !swap_amount.is_zero() {
                let repay_amount: Uint128;
                //Calc how much to repay for this position
                if position.repayment >= swap_amount {
                    //Repay the full swap_amount
                    repay_amount = swap_amount;
                } else {
                    //Repay the position.repayment
                    repay_amount = position.repayment;
                }

                //Update Position repayment
                auction.repayment_positions[i].repayment -= repay_amount;
                //Update swap amount
                swap_amount -= repay_amount;

                //Create Repay message
                if !repay_amount.is_zero() {
                    let message = CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: config.clone().positions_contract.to_string(),
                        msg: to_binary(&CDPExecuteMsg::Repay {
                            position_id: position.clone().position_info.position_id,
                            position_owner: Some(
                                position.clone().position_info.position_owner,
                            ),
                            send_excess_to: None,
                        })?,
                        funds: coins(repay_amount.u128(), coin.clone().denom),
                    });
                    msgs.push(message);

                    attrs.push(attr(
                        "position_repaid",
                        format!(
                            "Position Info: {:?}, Repayment: {}",
                            position.clone().position_info,
                            repay_amount
                        ),
                    ));
                }
            }                    
        }

        //Filter out fully repaid debts
        auction.repayment_positions = auction
            .clone()
            .repayment_positions
            .into_iter()
            .filter(|info| !info.repayment.is_zero())
            .collect::<Vec<RepayPosition>>();

        //Subtract from send_to users if Some
        for (i, recipient) in auction.clone().send_to.into_iter().enumerate() {

            if !swap_amount.is_zero() && !recipient.amount.is_zero(){

                let withdrawal_amount: Uint128;

                //Calculate amount able to send & update DebtAuction state
                if swap_amount >= recipient.amount {
                    auction.send_to[i].amount = Uint128::zero();

                    swap_amount -= recipient.amount;

                    withdrawal_amount = recipient.amount;

                } else {
                    auction.send_to[i].amount -= swap_amount;

                    withdrawal_amount = swap_amount;

                    swap_amount = Uint128::zero();                          
                }

                //Get credit asset info
                let credit_asset = deps
                .querier
                .query::<Basket>(&QueryRequest::Wasm(WasmQuery::Smart {
                    contract_addr: config.clone().positions_contract.to_string(),
                    msg: to_binary(&CDPQueryMsg::GetBasket { })?,
                }))?
                .credit_asset.info;

                //Create withdrawal msg
                let msg = withdrawal_msg(
                    Asset {
                        amount: withdrawal_amount,
                        info: credit_asset,
                    }, recipient.recipient)?;
                
                //Push msg
                msgs.push(msg);
            }                    
        }

        if swap_amount > Uint128::zero() {                            
            //Calculate the the user's overpayment
            //We want to allow users to focus on speed rather than correctness
            overpay = swap_amount;
            
            //Update DebtAuction limit
            auction.remaining_recapitalization -= (coin.clone().amount - overpay);
        } else {
            
            //Update DebtAuction limit
            auction.remaining_recapitalization -= coin.clone().amount;
        }
    } else {
        return Err(ContractError::Std(StdError::GenericErr { msg: String::from("Auction ended") }));
    }

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

    //Update or Remove DebtAuction 
    if auction.remaining_recapitalization.is_zero() {
        DEBT_AUCTION.remove(deps.storage);
    } else {
        DEBT_AUCTION.save(deps.storage, &auction)?;
    }
      
    Ok(Response::new().add_messages(msgs))
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::DebtAuction {} => to_binary(&DEBT_AUCTION.load(deps.storage)?),
        QueryMsg::OngoingFeeAuctions { auction_asset, limit, start_after } => {
            to_binary(&get_ongoing_fee_auctions(
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
            Err(StdError::GenericErr {
                msg: format!("Auction asset: {}, doesn't have an ongoing auction", auction_asset),
            })
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
                return Err(StdError::GenericErr {
                    msg: format!("Invalid auction swap asset: {}", asset),
                });
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