use cosmwasm_std::{
    attr, coins, entry_point, to_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, QueryRequest, Response, StdError, StdResult, Uint128, WasmMsg,
    WasmQuery,
};
use cw2::set_contract_version;
use cw20::Cw20ExecuteMsg;

use membrane::debt_auction::{AuctionResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};
use membrane::oracle::{PriceResponse, QueryMsg as OracleQueryMsg};
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::positions::{BasketResponse, ExecuteMsg as CDPExecuteMsg, QueryMsg as CDPQueryMsg};
use membrane::types::{Asset, AssetInfo, RepayPosition, UserInfo};

use crate::error::ContractError;
use crate::state::{Auction, Config, ASSETS, CONFIG, ONGOING_AUCTIONS};

// Contract name and version used for migration.
const CONTRACT_NAME: &str = "debt_auction";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const MAX_LIMIT: u64 = 31u64;

pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config: Config;
    if let Some(owner) = msg.owner {
        config = Config {
            owner: deps.api.addr_validate(&owner)?,
            oracle_contract: deps.api.addr_validate(&msg.oracle_contract)?,
            osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
            mbrn_denom: msg.mbrn_denom,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            twap_timeframe: msg.twap_timeframe,
            initial_discount: msg.initial_discount,
            discount_increase_timeframe: msg.discount_increase_timeframe,
            discount_increase: msg.discount_increase,
        };
    } else {
        config = Config {
            owner: info.sender,
            oracle_contract: deps.api.addr_validate(&msg.oracle_contract)?,
            osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
            mbrn_denom: msg.mbrn_denom,
            positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
            twap_timeframe: msg.twap_timeframe,
            initial_discount: msg.initial_discount,
            discount_increase_timeframe: msg.discount_increase_timeframe,
            discount_increase: msg.discount_increase,
        };
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    //Initialize Assets
    ASSETS.save(deps.storage, &vec![])?;

    Ok(Response::default())
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
            debt_asset,
        } => start_auction(deps, env, info, repayment_position_info, debt_asset),
        ExecuteMsg::SwapForMBRN {} => swap_for_mbrn(deps, info, env),
        ExecuteMsg::RemoveAuction { debt_asset } => remove_auction(deps, info, debt_asset),
        ExecuteMsg::UpdateConfig {
            owner,
            oracle_contract,
            osmosis_proxy,
            positions_contract,
            mbrn_denom,
            twap_timeframe,
            initial_discount,
            discount_increase_timeframe,
            discount_increase,
        } => update_config(
            deps,
            info,
            owner,
            oracle_contract,
            osmosis_proxy,
            mbrn_denom,
            positions_contract,
            twap_timeframe,
            initial_discount,
            discount_increase_timeframe,
            discount_increase,
        ),
    }
}


fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    oracle_contract: Option<String>,
    osmosis_proxy: Option<String>,
    mbrn_denom: Option<String>,
    positions_contract: Option<String>,
    twap_timeframe: Option<u64>,
    initial_discount: Option<Decimal>,
    discount_increase_timeframe: Option<u64>, //in seconds
    discount_increase: Option<Decimal>,       //% increase
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    //Save optionals
    if let Some(addr) = owner {
        config.owner = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = oracle_contract {
        config.oracle_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = osmosis_proxy {
        config.osmosis_proxy = deps.api.addr_validate(&addr)?;
    }
    if let Some(addr) = positions_contract {
        config.positions_contract = deps.api.addr_validate(&addr)?;
    }
    if let Some(mbrn_denom) = mbrn_denom {
        config.mbrn_denom = mbrn_denom;
    }
    if let Some(twap_timeframe) = twap_timeframe {
        config.twap_timeframe = twap_timeframe;
    }
    if let Some(initial_discount) = initial_discount {
        config.initial_discount = initial_discount;
    }
    if let Some(discount_increase_timeframe) = discount_increase_timeframe {
        config.discount_increase_timeframe = discount_increase_timeframe;
    }
    if let Some(discount_increase) = discount_increase {
        config.discount_increase = discount_increase;
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new())
}

//Start Auction 
//Auctions have set recaptilization limits and will automatically repay for CDP Positions
fn start_auction(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user_info: UserInfo,
    debt_asset: Asset,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Only positions contract or owner can start auctions
    if info.sender != config.owner && info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }

    let mut attrs = vec![
        attr("method", "start_auction"),
        attr("debt_asset", debt_asset.to_string()),
    ];

    //Update Asset list
    ASSETS.update(
        deps.storage,
        |mut assets: Vec<AssetInfo>| -> Result<Vec<AssetInfo>, ContractError> {
            //Add to list if new asset
            if !assets
                .clone()
                .into_iter().any(|asset| asset.equal(&debt_asset.info))
            {
                assets.push(debt_asset.clone().info);
            }

            Ok(assets)
        },
    )?;

    //Update Auctions
    ONGOING_AUCTIONS.update(
        deps.storage,
        debt_asset.clone().info.to_string(),
        |auction| -> Result<Auction, ContractError> {
            match auction {
                //Add debt_amount and repayment info to the auction
                Some(mut auction) => {
                    auction.remaining_recapitalization += debt_asset.clone().amount;

                    auction.repayment_positions.push(RepayPosition {
                        repayment: debt_asset.clone().amount,
                        position_info: user_info,
                    });

                    attrs.push(attr("auction_status", "added_to"));

                    Ok(auction)
                }
                //Add new auction
                None => {
                    attrs.push(attr("auction_status", "started_anew"));

                    Ok(Auction {
                        remaining_recapitalization: debt_asset.clone().amount,
                        repayment_positions: vec![RepayPosition {
                            repayment: debt_asset.clone().amount,
                            position_info: user_info.clone(),
                        }],
                        auction_start_time: env.block.time.seconds(),
                        basket_id_price_source: user_info.basket_id,
                    })
                }
            }
        },
    )?;

    Ok(Response::new().add_attributes(attrs))
}

//Remove Auction 
fn remove_auction(
    deps: DepsMut,
    info: MessageInfo,
    debt_asset: AssetInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Only positions contract or owner can start auctions
    if info.sender != config.owner && info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }

    let attrs = vec![
        attr("method", "remove_auction"),
        attr("debt_asset", debt_asset.to_string()),
    ];

    //Update Auctions
    ONGOING_AUCTIONS.remove(deps.storage, debt_asset.to_string());

    Ok(Response::new().add_attributes(attrs))
}

//Swap the debt asset in an auction for MBRN at a discount
fn swap_for_mbrn(deps: DepsMut, info: MessageInfo, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let mut overpay = Uint128::zero();

    let mut msgs: Vec<CosmosMsg> = vec![];
    let mut attrs = vec![attr("method", "swap_for_mbrn")];

    //Parse thru all sent assets 
    //Swap the ones that have auctions open w/ non-zero recapitalization requests
    for coin in info.clone().funds {

        //If the asset has an ongoing auction
        if let Ok(mut auction) = ONGOING_AUCTIONS.load(deps.storage, coin.clone().denom) {
            if !auction.remaining_recapitalization.is_zero() {
                let swap_amount: Decimal;
                //Set swap_amount
                if coin.amount > auction.remaining_recapitalization {
                    swap_amount = Decimal::from_ratio(
                        auction.remaining_recapitalization,
                        Uint128::new(1u128),
                    );

                    //Calculate the the user's overpayment
                    //We want to allow  users to focus on speed rather than correctness
                    overpay = coin.amount - auction.remaining_recapitalization;
                } else {
                    swap_amount = Decimal::from_ratio(coin.amount, Uint128::new(1u128));
                }

                //Get MBRN price
                let mbrn_price = deps
                    .querier
                    .query::<PriceResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: config.clone().oracle_contract.to_string(),
                        msg: to_binary(&OracleQueryMsg::Price {
                            asset_info: AssetInfo::NativeToken {
                                denom: config.clone().mbrn_denom,
                            },
                            twap_timeframe: config.clone().twap_timeframe,
                            basket_id: Some(auction.basket_id_price_source),
                        })?,
                    }))?
                    .avg_price;

                //Get credit price from Positions contract to further incentivize recapitalization
                let basket_credit_price = deps
                    .querier
                    .query::<BasketResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: config.clone().positions_contract.to_string(),
                        msg: to_binary(&CDPQueryMsg::GetBasket {
                            basket_id: auction.basket_id_price_source,
                        })?,
                    }))?
                    .credit_price;

                //Get discount
                let time_elapsed = env.block.time.seconds() - auction.auction_start_time;
                let discount_multiplier = time_elapsed / config.discount_increase_timeframe;
                let current_discount_increase = decimal_multiplication(
                    Decimal::from_ratio(
                        Uint128::new(discount_multiplier.into()),
                        Uint128::new(1u128),
                    ),
                    config.discount_increase,
                );
                let discount = decimal_subtraction(
                    Decimal::one(),
                    (current_discount_increase + config.initial_discount),
                );

                //Mint MBRN for user
                let discounted_mbrn_price = decimal_multiplication(mbrn_price, discount);
                let credit_value = decimal_multiplication(swap_amount, basket_credit_price);
                let mbrn_mint_amount =
                    decimal_division(credit_value, discounted_mbrn_price) * Uint128::new(1u128);

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

                //Update Auction limit
                auction.remaining_recapitalization -= swap_amount;

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
                                    basket_id: position.clone().position_info.basket_id,
                                    position_id: position.clone().position_info.position_id,
                                    position_owner: Some(
                                        position.clone().position_info.position_owner,
                                    ),
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

                overpay = Uint128::zero();
            }

            //Save new auction
            ONGOING_AUCTIONS.save(deps.storage, coin.denom, &auction)?;
        } else {
            return Err(ContractError::InvalidAsset { asset: coin.denom });
        }
    }

    Ok(Response::new().add_messages(msgs))
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::OngoingAuctions {
            debt_asset,
            limit,
            start_without,
        } => to_binary(&get_ongoing_auction(
            deps,
            debt_asset,
            limit,
            start_without,
        )?),
        QueryMsg::ValidDebtAssets {
            debt_asset,
            limit,
            start_without,
        } => to_binary(&get_valid_assets(deps, debt_asset, limit, start_without)?),
    }
}

//Get Assets that have been used as Auction debt assets
fn get_valid_assets(
    deps: Deps,
    debt_asset: Option<AssetInfo>,
    limit: Option<u64>,
    start_without: Option<AssetInfo>,
) -> StdResult<Vec<AssetInfo>> {
    if let Some(debt_asset) = debt_asset {
        if let Some(_asset) = ASSETS
            .load(deps.storage)?
            .into_iter()
            .find(|asset| debt_asset.equal(asset))
        {
            Ok(vec![debt_asset])
        } else {
            Err(StdError::GenericErr {
                msg: format!("Invalid auction swap asset: {}", debt_asset),
            })
        }
    } else {
        let limit: u64 = limit.unwrap_or(MAX_LIMIT);
        let start = if let Some(start) = start_without {
            start
        } else {
            AssetInfo::NativeToken {
                denom: String::from(""),
            }
        };

        let valid_assets: Vec<AssetInfo> = ASSETS
            .load(deps.storage)?
            .into_iter()
            .filter(|asset| !asset.equal(&start))
            .take(limit as usize)
            .collect::<Vec<AssetInfo>>();

        Ok(valid_assets)
    }
}

//Get an AuctionResponse of any Auction w/ a non-zero remaining_recapitalization
fn get_ongoing_auction(
    deps: Deps,
    debt_asset: Option<AssetInfo>,
    limit: Option<u64>,
    start_without: Option<AssetInfo>,
) -> StdResult<Vec<AuctionResponse>> {
    if let Some(debt_asset) = debt_asset {
        if let Ok(auction) = ONGOING_AUCTIONS.load(deps.storage, debt_asset.to_string()) {
            //Skip zeroed auctions
            if !auction.remaining_recapitalization.is_zero() {
                Ok(vec![AuctionResponse {
                    remaining_recapitalization: auction.remaining_recapitalization,
                    repayment_positions: auction.clone().repayment_positions,
                    auction_start_time: auction.auction_start_time,
                    basket_id_price_source: auction.basket_id_price_source,
                }])
            } else {
                Err(StdError::GenericErr {
                    msg: String::from("Auction recapitalization amount empty"),
                })
            }
        } else {
            Err(StdError::GenericErr {
                msg: format!("Invalid auction swap asset: {}", debt_asset),
            })
        }
    } else {
        let limit: u64 = limit.unwrap_or(MAX_LIMIT);
        let start = if let Some(start) = start_without {
            start
        } else {
            AssetInfo::NativeToken {
                denom: String::from(""),
            }
        };

        let mut resp = vec![];

        let assets: Vec<AssetInfo> = ASSETS
            .load(deps.storage)?
            .into_iter()
            .filter(|asset| !asset.equal(&start))
            .take(limit as usize)
            .collect::<Vec<AssetInfo>>();

        for asset in assets {
            //Load auction
            if let Ok(auction) = ONGOING_AUCTIONS.load(deps.storage, asset.to_string()) {
                //Add Response
                //Skip zeroed aucitons
                if !auction.remaining_recapitalization.is_zero() {
                    resp.push(AuctionResponse {
                        remaining_recapitalization: auction.clone().remaining_recapitalization,
                        repayment_positions: auction.clone().repayment_positions,
                        auction_start_time: auction.clone().auction_start_time,
                        basket_id_price_source: auction.clone().basket_id_price_source,
                    });
                }
            } else {
                return Err(StdError::GenericErr {
                    msg: format!("Invalid auction swap asset: {}", asset),
                });
            }
        }

        Ok(resp)
    }
}

//Helper functions

pub fn withdrawal_msg(asset: Asset, recipient: Addr) -> StdResult<CosmosMsg> {
    match asset.clone().info {
        AssetInfo::NativeToken { denom: _ } => {
            let coin: Coin = asset_to_coin(asset)?;
            let message = CosmosMsg::Bank(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![coin],
            });
            Ok(message)
        }
        AssetInfo::Token { address } => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: address.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: recipient.to_string(),
                    amount: asset.amount,
                })?,
                funds: vec![],
            });
            Ok(message)
        }
    }
}

pub fn asset_to_coin(asset: Asset) -> StdResult<Coin> {
    match asset.info {
        //
        AssetInfo::Token { address: _ } => {
            Err(StdError::GenericErr {
                msg: "Only native assets can become Coin objects".to_string(),
            })
        }
        AssetInfo::NativeToken { denom } => Ok(Coin {
            denom,
            amount: asset.amount,
        }),
    }
}