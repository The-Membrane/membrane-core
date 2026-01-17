//Token factory fork
//https://github.com/osmosis-labs/bindings/blob/main/contracts/tokenfactory

use std::convert::TryInto;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary, BankMsg, Addr, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Order, Reply, Response, StdError, StdResult, SubMsg, Uint128, WasmMsg
};
use membrane::neutron_proxy::{
    Config, ContractDenomsResponse, DualityRoute, ExecuteMsg, GetDenomResponse, InstantiateMsg, MigrateMsg, QueryMsg, TokenInfoResponse, NeutronOwnerEntry, NeutronMsg, TransmuteSupplyThresholdEntry
};
use membrane::{mars_vault_token, transmuter};
use membrane::types::{AssetInfo, NeutronOwner, TransmutationPair, TransmutationPairEntry, VaultEntry, VestingPeriod};
use membrane::helpers::get_contract_balances;
use cw2::set_contract_version;

use crate::error::TokenFactoryError;
use crate::state::{PendingTokenInfo, TokenInfo, SwapInfo, CONFIG, PENDING, TOKENS, SWAP_ROUTES, SWAP_INFO, SWAP_ROUTE_CONFIG, TRANSMUTE_SUPPLY_THRESHOLDS};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{self as TokenFactory, QueryDenomsFromCreatorResponse, MsgCreateDenomResponse};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:neutron-proxy";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const MAX_LIMIT: u32 = 64;

const CREATE_DENOM_REPLY_ID: u64 = 1u64;
const SWAP_REPLY_ID: u64 = 2u64;
const USE_BALANCE_SWAP_REPLY_ID: u64 = 3u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    // Query transmuter config if provided
    let (transmuter_contract, cdt_denom, usdc_denom) = if let Some(transmuter_addr) = msg.transmuter_contract {
        let transmuter_addr = deps.api.addr_validate(&transmuter_addr)?;
        
        // Query the transmuter config
        let transmuter_config: transmuter::Config = deps.querier.query_wasm_smart(
            transmuter_addr.clone(),
            &transmuter::QueryMsg::Config {}
        )?;
        
        (
            Some(transmuter_addr),
            Some(transmuter_config.deposit_pair.cdt),
            Some(transmuter_config.deposit_pair.paired_asset),
        )
    } else {
        (None, None, None)
    };

    let astroport_factory = if let Some(factory_addr) = msg.astroport_factory {
        Some(deps.api.addr_validate(&factory_addr)?)
    } else {
        None
    };

    let astroport_router = if let Some(router_addr) = msg.astroport_router {
        Some(deps.api.addr_validate(&router_addr)?)
    } else {
        None
    };

    let config = Config {
        owners: vec![
            NeutronOwner {
                owner: info.sender.clone(),
                non_token_contract_auth: true,
            }],
        debt_auction: None,
        transmutation_pairs: vec![],
        transmuter_contract,
        cdt_denom,
        usdc_denom,
        vaults: msg.vaults.unwrap_or_default(),
        astroport_factory,
        astroport_router,
        enable_dynamic_routing: msg.enable_dynamic_routing.unwrap_or(false),
        vesting_contract: None,
        vesting_period: None,
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::<NeutronMsg>::new()
        .add_attribute("method", "instantiate")
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
) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    match msg {
        ExecuteMsg::CreateDenom {
            subdenom,
            max_supply,
        } => create_denom(
            deps,
            env,
            info,
            subdenom,
            max_supply,
        ),
        ExecuteMsg::ChangeAdmin {
            denom,
            new_admin_address,
        } => change_admin(deps, env, info, denom, new_admin_address),
        ExecuteMsg::EditOwner { owner, non_token_contract_auth, remove } => {
             edit_owner(deps, info, owner, non_token_contract_auth, remove)
         },
        ExecuteMsg::MintTokens {
            denom,
            amount,
            mint_to_address,
        } => mint_tokens(deps, env, info, denom, amount, mint_to_address),
        ExecuteMsg::BurnTokens {
            denom,
            amount,
            burn_from_address,
        } => burn_tokens(deps, env, info, denom, amount, burn_from_address),
        ExecuteMsg::EditTokenMaxSupply { denom, max_supply } => {
            edit_token_max(deps, info, denom, max_supply)
        },
        ExecuteMsg::ExecuteSwaps { token_out, max_slippage } => {
            execute_swaps(deps, env, info.sender.clone(), info.funds.clone(), token_out, max_slippage)
        },
        ExecuteMsg::UpdateConfig {
            owners,
            debt_auction,
            transmutation_pairs,
            transmuter_contract,
            vaults,
            astroport_factory,
            astroport_router,
            enable_dynamic_routing,
            transmute_supply_thresholds,
            vesting_contract,
            vesting_period,
        } => update_config(deps, info, owners, debt_auction, transmutation_pairs, transmuter_contract, vaults, astroport_factory, astroport_router, enable_dynamic_routing, transmute_supply_thresholds, vesting_contract, vesting_period),
        // ExecuteMsg::CreatePclPair { asset_infos, params } => {
        //     execute_create_pcl_pair(deps, env, info, asset_infos, params)
        // },
        ExecuteMsg::UpdateSwapRoute { route } => {
            execute_update_swap_route(deps, info, route)
        },
        ExecuteMsg::UpdateSwapRoutes { routes } => {
            execute_update_swap_routes(deps, info, routes)
        },
        ExecuteMsg::TransmuteTokens { } => transmute_tokens(deps, env, info),
    }
}

/// Transmute tokens
fn transmute_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    ) -> Result<Response<NeutronMsg>, TokenFactoryError> {
        let config = CONFIG.load(deps.storage)?;
        let transmutation_in = config.transmutation_pairs.clone().into_iter().map(|pair| pair.token_in).collect::<Vec<String>>();

        //Assert only one asset sent
        if info.funds.len() != 1 {
            return Err(TokenFactoryError::CustomError { val: format!("Invalid number of assets sent: {}", info.funds.len()) });
        }

        //Assert asset is a transmutation token in
        if !transmutation_in.contains(&info.funds[0].denom) {
            return Err(TokenFactoryError::CustomError { val: format!("Invalid transmutation token in: {}", info.funds[0].denom) });
        }

        //Get transmutation pair
        let transmutation_pair = config.transmutation_pairs.clone().into_iter().find(|pair| pair.token_in == info.funds[0].denom).unwrap();
        let transmutation_pair_clone = transmutation_pair.clone();


        //Check if the token_to_mint is a denom we can mint
        if !TOKENS.has(deps.storage, transmutation_pair.token_to_mint.clone()) {
            //If not, we get the "amount_to_mint" from the contract balance of the token_to_mint
            let token_out_balance = get_contract_balances(deps.querier, env.clone(), vec![AssetInfo::NativeToken { denom: transmutation_pair.token_to_mint.clone() }])?[0];
            //If no balance, error
            if token_out_balance.is_zero() {
                return Err(TokenFactoryError::CustomError { val: format!("No balance of token to mint: {}", transmutation_pair.token_to_mint) });
            }

            //Calculate amount to send
            let amount_to_send = token_out_balance * transmutation_pair.mint_ratio;

            //Send tokens
            let send_tokens_msg: CosmosMsg<NeutronMsg> = CosmosMsg::Bank(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![Coin { denom: transmutation_pair.token_to_mint, amount: amount_to_send }],
            });

            Ok(Response::<NeutronMsg>::new()
                .add_attribute("method", "transmute_tokens")
                .add_attribute("transmutation_pair", format!("{:?}", transmutation_pair_clone))
                .add_attribute("amount_to_send", amount_to_send)
                .add_message(send_tokens_msg))
        } else {
            //Check if there's a supply threshold for this token
            if let Some(threshold) = TRANSMUTE_SUPPLY_THRESHOLDS.may_load(deps.storage, transmutation_pair.token_to_mint.clone())? {
                //Load token info to check current supply
                let token_info = TOKENS.load(deps.storage, transmutation_pair.token_to_mint.clone())?;

                //CRITICAL: Check if threshold crossed
                if token_info.current_supply >= threshold {
                    // POST-THRESHOLD: Redirect to vesting
                    return execute_vesting_transmutation(
                        deps, env, info, config, transmutation_pair
                    );
                } else {
                    // PRE-THRESHOLD: Block transmutation
                    return Err(TokenFactoryError::CustomError {
                        val: format!(
                            "Transmuting not yet enabled. Current supply: {}, required threshold: {}",
                            token_info.current_supply,
                            threshold
                        )
                    });
                }
            }

            //Calculate amount to mint
            let amount_to_mint = info.funds[0].amount * transmutation_pair.mint_ratio;

            //Mint tokens
            let mint_tokens_msg: CosmosMsg<NeutronMsg> = TokenFactory::MsgMint {
                sender: env.contract.address.to_string(),
                amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
                    denom: transmutation_pair.token_to_mint.clone(),
                    amount: amount_to_mint.to_string(),
                }),
                mint_to_address: info.sender.to_string(),
            }.into();


            Ok(Response::<NeutronMsg>::new()
                .add_attribute("method", "transmute_tokens")
                .add_attribute("transmutation_pair", format!("{:?}", transmutation_pair_clone))
                .add_attribute("amount_to_mint", amount_to_mint)
                .add_message(mint_tokens_msg))

        }

}

/// Execute vesting transmutation (post-threshold)
fn execute_vesting_transmutation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    config: Config,
    transmutation_pair: TransmutationPair,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    // Validate vesting config
    let vesting_contract = config.vesting_contract
        .ok_or(TokenFactoryError::CustomError {
            val: "Vesting contract not configured".to_string()
        })?;

    let vesting_period = config.vesting_period
        .ok_or(TokenFactoryError::CustomError {
            val: "Vesting period not configured".to_string()
        })?;

    // Calculate amount to vest
    let amount_to_vest = info.funds[0].amount * transmutation_pair.mint_ratio;

    // Forward to vesting contract
    let vesting_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: vesting_contract.to_string(),
        msg: to_json_binary(&membrane::vesting::ExecuteMsg::AddVestedTransmutation {
            recipient: info.sender.to_string(),
            amount_to_mint: amount_to_vest,
            vesting_period,
        })?,
        funds: info.funds.clone(), // Forward oldMBRN
    });

    Ok(Response::<NeutronMsg>::new()
        .add_attribute("method", "vesting_transmutation")
        .add_attribute("user", info.sender)
        .add_attribute("amount_to_vest", amount_to_vest)
        .add_message(vesting_msg))
}

/// Execute a swap to token out 
fn execute_swaps(
    deps: DepsMut,
    env: Env,
    swapper: Addr,
    funds: Vec<Coin>,
    token_out: String,
    max_slippage: Decimal,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    let mut msgs: Vec<SubMsg<NeutronMsg>> = vec![];
    let config = CONFIG.load(deps.storage)?;

    //If no funds sent, error
    if funds.is_empty() {
        return Err(TokenFactoryError::ZeroAmount {});
    }

    //Filter out transmutation token_ins
    let transmutation_token_ins: Vec<String> = config.transmutation_pairs.into_iter().map(|pair| pair.token_in).collect();
    let filtered_funds: Vec<Coin> = funds.iter().filter(|coin| !transmutation_token_ins.contains(&coin.denom)).cloned().collect();
    let filtered_denoms: Vec<String> = funds.iter().filter(|coin| transmutation_token_ins.contains(&coin.denom)).map(|coin| coin.denom.clone()).collect();

    //If no funds left after filtering, error
    if filtered_funds.is_empty() {
        return Err(TokenFactoryError::ZeroAmount {});
    }

    //create swap msgs for each asset sent
    for coin in filtered_funds.clone().into_iter() {
        // Check if this is a vault token that needs to be exited first
        let vault_for_token = config.vaults
            .iter()
            .find(|v| v.vault_token == coin.denom);
        
        if let Some(vault) = vault_for_token {
            // Exit vault token to get underlying asset, then swap the result
            let exit_vault_msg: CosmosMsg<NeutronMsg> = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: vault.vault_addr.to_string(),
                msg: to_json_binary(&mars_vault_token::ExecuteMsg::ExitVault {})?,
                funds: vec![coin.clone()],
            });

            // Use reply to swap the exited vault tokens
            msgs.push(SubMsg::reply_on_success(exit_vault_msg, USE_BALANCE_SWAP_REPLY_ID));
            continue;
        }

        // Check if this is a USDC<>CDT swap that should use the transmuter contract (Different than the transmutation pairs)
        let is_transmuter_swap = if let (Some(_), Some(ref cdt), Some(ref usdc)) = 
            (&config.transmuter_contract, &config.cdt_denom, &config.usdc_denom) {
            // Check if we're swapping USDC->CDT or CDT->USDC
            (coin.denom == *usdc && token_out == *cdt) || (coin.denom == *cdt && token_out == *usdc)
        } else {
            false
        };

        if is_transmuter_swap {
            // Use transmuter for USDC<>CDT swaps
            let transmuter_addr = config.transmuter_contract.as_ref().unwrap();
            
            let transmute_msg: CosmosMsg<NeutronMsg> = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: transmuter_addr.to_string(),
                msg: to_json_binary(&transmuter::ExecuteMsg::Transmute {
                    recipient: Some(swapper.to_string()),
                })?,
                funds: vec![coin.clone()],
            });

            msgs.push(SubMsg::new(transmute_msg));
        } else {
            // Check route configuration for this pair
            let route_pref = SWAP_ROUTE_CONFIG.may_load(
                deps.storage,
                (coin.denom.clone(), token_out.clone()),
            )?;

            // Calculate minimum amount out with slippage protection
            let min_receive = coin.amount * (Decimal::one() - max_slippage);

            match route_pref {
                Some(membrane::neutron_proxy::DexPreference::Best) => {
                    // Dynamic routing: query both DEXes and choose best
                    if !config.enable_dynamic_routing {
                        return Err(TokenFactoryError::DynamicRoutingDisabled {});
                    }

                    // Query Duality output (simulate)
                    // Note: Duality doesn't have a simulation query, so we return None
                    // In production, you might want to implement a price oracle or use a different method
                    let duality_output: Option<Uint128> = None;

                    // Query Astroport output (if factory configured and pair exists)
                    let astroport_output = if let Some(ref factory) = config.astroport_factory {
                        let asset_in = membrane::types::AssetInfo::NativeToken {
                            denom: coin.denom.clone(),
                        };
                        let asset_out = membrane::types::AssetInfo::NativeToken {
                            denom: token_out.clone(),
                        };
                        
                        match crate::astroport_helpers::resolve_astroport_pair(
                            &deps.querier,
                            factory,
                            &[asset_in.clone(), asset_out.clone()],
                        ) {
                            Ok(pair_addr) => {
                                crate::astroport_helpers::query_astroport_swap_output(
                                    &deps.querier,
                                    &pair_addr,
                                    &coin,
                                    &asset_out,
                                ).ok()
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    // Choose best DEX
                    let best_dex = crate::astroport_helpers::choose_best_dex(
                        duality_output,
                        astroport_output,
                    )?;

                    match best_dex {
                        membrane::neutron_proxy::DexChoice::Duality => {
                            // Use Duality
                            let route = DualityRoute {
                                from: coin.denom.clone(),
                                to: token_out.clone(),
                                swap_denoms: vec![coin.denom.clone(), token_out.clone()],
                            };
                            route.validate(&deps.querier, &coin.denom, &token_out)?;
                            let swap_msg = route.build_exact_in_swap_msg(
                                &deps.querier,
                                &env,
                                &coin,
                                min_receive,
                            )?;
                            msgs.push(SubMsg::reply_on_success(swap_msg, SWAP_REPLY_ID));
                        }
                        membrane::neutron_proxy::DexChoice::Astroport => {
                            // Use Astroport
                            let factory = config.astroport_factory.as_ref().ok_or(
                                TokenFactoryError::RouterNotConfigured {}
                            )?;
                            let asset_in = membrane::types::AssetInfo::NativeToken {
                                denom: coin.denom.clone(),
                            };
                            let asset_out = membrane::types::AssetInfo::NativeToken {
                                denom: token_out.clone(),
                            };
                            let pair_addr = crate::astroport_helpers::resolve_astroport_pair(
                                &deps.querier,
                                factory,
                                &[asset_in, asset_out.clone()],
                            )?;
                            let swap_msg = crate::astroport_helpers::build_astroport_swap_msg(
                                &pair_addr,
                                &coin,
                                &asset_out,
                                min_receive,
                                Some(max_slippage),
                                Some(env.contract.address.clone()),
                            )?;
                            msgs.push(SubMsg::reply_on_success(swap_msg, SWAP_REPLY_ID));
                        }
                    }
                }
                Some(membrane::neutron_proxy::DexPreference::Astroport) => {
                    // Use Astroport
                    let factory = config.astroport_factory.as_ref().ok_or(
                        TokenFactoryError::RouterNotConfigured {}
                    )?;
                    let asset_in = membrane::types::AssetInfo::NativeToken {
                        denom: coin.denom.clone(),
                    };
                    let asset_out = membrane::types::AssetInfo::NativeToken {
                        denom: token_out.clone(),
                    };
                    let pair_addr = crate::astroport_helpers::resolve_astroport_pair(
                        &deps.querier,
                        factory,
                        &[asset_in, asset_out.clone()],
                    )?;
                    let swap_msg = crate::astroport_helpers::build_astroport_swap_msg(
                        &pair_addr,
                        &coin,
                        &asset_out,
                        min_receive,
                        Some(max_slippage),
                        Some(env.contract.address.clone()),
                    )?;
                    msgs.push(SubMsg::reply_on_success(swap_msg, SWAP_REPLY_ID));
                }
                Some(membrane::neutron_proxy::DexPreference::Duality) => {
                    // Use Duality (explicit)
                    let route = DualityRoute {
                        from: coin.denom.clone(),
                        to: token_out.clone(),
                        swap_denoms: vec![coin.denom.clone(), token_out.clone()],
                    };
                    route.validate(&deps.querier, &coin.denom, &token_out)?;
                    let swap_msg = route.build_exact_in_swap_msg(
                        &deps.querier,
                        &env,
                        &coin,
                        min_receive,
                    )?;
                    msgs.push(SubMsg::reply_on_success(swap_msg, SWAP_REPLY_ID));
                }
                Some(membrane::neutron_proxy::DexPreference::MultiHop(_hops)) => {
                    // Multi-hop routing - for now, fall back to simple route
                    // TODO: Implement multi-hop with per-hop DEX selection
                    return Err(TokenFactoryError::InvalidRouteConfig {
                        reason: "Multi-hop routing not yet implemented".to_string(),
                    });
                }
                None => {
                    // No route config - default to Duality (backward compatible)
                    let route = DualityRoute {
                        from: coin.denom.clone(),
                        to: token_out.clone(),
                        swap_denoms: vec![coin.denom.clone(), token_out.clone()],
                    };
                    route.validate(&deps.querier, &coin.denom, &token_out)?;
                    let swap_msg = route.build_exact_in_swap_msg(
                        &deps.querier,
                        &env,
                        &coin,
                        min_receive,
                    )?;
                    msgs.push(SubMsg::reply_on_success(swap_msg, SWAP_REPLY_ID));
                }
            }
        }
    }

    //Save SwapInfo
    SWAP_INFO.save(deps.storage, &SwapInfo {
        swapper: swapper,
        prev_balances: filtered_funds.clone(),
        token_out: token_out.clone(),
        max_slippage: max_slippage.clone(),
    })?;

    Ok(Response::<NeutronMsg>::new()
        .add_attribute("method", "execute_swaps")
        .add_attribute("token_out", token_out)
        .add_attribute("max_slippage", max_slippage.to_string())
        .add_attribute("filtered_transmutation_tokens", format!("{:?}", filtered_denoms))
        .add_submessages(msgs))
}


/// Update contract configuration
/// This function is only callable by an owner with non_token_contract_auth set to true
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owners: Option<Vec<NeutronOwnerEntry>>,
    debt_auction: Option<String>,
    transmutation_pairs: Option<Vec<TransmutationPairEntry>>,
    transmuter_contract: Option<String>,
    vaults: Option<Vec<VaultEntry>>,
    astroport_factory: Option<String>,
    astroport_router: Option<String>,
    enable_dynamic_routing: Option<bool>,
    transmute_supply_thresholds: Option<Vec<TransmuteSupplyThresholdEntry>>,
    vesting_contract: Option<String>,
    vesting_period: Option<VestingPeriod>,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    let mut config = CONFIG.load(deps.storage)?;

    let (authorized, owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized || !config.owners[owner_index].non_token_contract_auth {
        return Err(TokenFactoryError::Unauthorized {});
    }

    //Edit Owner
    if let Some(owners) = owners {
        for owner_entry in owners {
            if owner_entry.remove {
                //Remove owner
                config.owners.retain(|o| o.owner != owner_entry.owner.owner);
            } else {
                //Validate Owner address
                deps.api.addr_validate(&owner_entry.owner.owner.to_string())?;

                //Error if owner already exists
                for stored_owner in config.clone().owners {
                    if stored_owner.owner == owner_entry.owner.owner {
                        return Err(TokenFactoryError::AlreadyOwner {});
                    }
                }

                //Add owner
                config.owners.push(owner_entry.owner);
            }
        }
    }

    //Edit Contracts
    if let Some(debt_auction) = debt_auction {
        config.debt_auction = Some(deps.api.addr_validate(&debt_auction)?);
    }

    //Edit Transmutation Pairs
    if let Some(transmutation_pairs) = transmutation_pairs {
        for pair in transmutation_pairs {
            if pair.remove {
                config.transmutation_pairs.retain(|p| p.token_in != pair.transmutation_pair.token_in);
            } else {
                //Add new pair or update existing pair
                if let Some((index, _pair)) = config.transmutation_pairs.clone().into_iter().enumerate()
                .find(|(_i, existing_pair)| existing_pair.token_in == pair.transmutation_pair.token_in && existing_pair.token_to_mint == pair.transmutation_pair.token_to_mint){
                    config.transmutation_pairs[index].mint_ratio = pair.transmutation_pair.mint_ratio;
                } else {
                    config.transmutation_pairs.push(pair.transmutation_pair);
                }
            }
        }
    }

    //Edit Transmuter Contract
    if let Some(transmuter_addr) = transmuter_contract {
        let transmuter_addr = deps.api.addr_validate(&transmuter_addr)?;
        
        // Query the transmuter config to update denoms
        let transmuter_config: transmuter::Config = deps.querier.query_wasm_smart(
            transmuter_addr.clone(),
            &transmuter::QueryMsg::Config {}
        )?;
        
        config.transmuter_contract = Some(transmuter_addr);
        config.cdt_denom = Some(transmuter_config.deposit_pair.cdt);
        config.usdc_denom = Some(transmuter_config.deposit_pair.paired_asset);
    }

    //Edit Vaults
    if let Some(vaults) = vaults {
        for vault_entry in vaults {
            if vault_entry.remove {
                // Remove vault by vault_token
                config.vaults.retain(|v| v.vault_token != vault_entry.vault_info.vault_token);
            } else {
                // Validate vault address
                deps.api.addr_validate(&vault_entry.vault_info.vault_addr.to_string())?;

                // Add new vault or update existing vault
                if let Some(index) = config.vaults.iter().position(|v| v.vault_token == vault_entry.vault_info.vault_token) {
                    // Update existing vault
                    config.vaults[index] = vault_entry.vault_info;
                } else {
                    // Add new vault
                    config.vaults.push(vault_entry.vault_info);
                }
            }
        }
    }

    //Edit Astroport Factory
    if let Some(factory_addr) = astroport_factory {
        config.astroport_factory = Some(deps.api.addr_validate(&factory_addr)?);
    }

    //Edit Astroport Router
    if let Some(router_addr) = astroport_router {
        config.astroport_router = Some(deps.api.addr_validate(&router_addr)?);
    }

    //Edit Dynamic Routing
    if let Some(enabled) = enable_dynamic_routing {
        config.enable_dynamic_routing = enabled;
    }

    //Edit Transmute Supply Thresholds
    if let Some(thresholds) = transmute_supply_thresholds {
        for threshold_entry in thresholds {
            if threshold_entry.remove {
                // Remove threshold
                TRANSMUTE_SUPPLY_THRESHOLDS.remove(deps.storage, threshold_entry.denom);
            } else if let Some(threshold) = threshold_entry.threshold {
                // Validate denom format
                validate_denom(threshold_entry.denom.clone())?;
                // Save threshold
                TRANSMUTE_SUPPLY_THRESHOLDS.save(deps.storage, threshold_entry.denom, &threshold)?;
            }
        }
    }

    //Edit Vesting Contract
    if let Some(vesting) = vesting_contract {
        config.vesting_contract = Some(deps.api.addr_validate(&vesting)?);
    }

    //Edit Vesting Period
    if let Some(period) = vesting_period {
        config.vesting_period = Some(period);
    }

    //Save Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::<NeutronMsg>::new().add_attributes(vec![
        attr("method", "update_config"),
        attr("updated_config", format!("{:?}", config)),
        ]))
}

// /// Create Astroport PCL pair
// fn execute_create_pcl_pair(
//     deps: DepsMut,
//     env: Env,
//     info: MessageInfo,
//     asset_infos: Vec<AssetInfo>,
//     params: membrane::neutron_proxy::PclInitParams,
// ) -> Result<Response<NeutronMsg>, TokenFactoryError> {
//     let config = CONFIG.load(deps.storage)?;

//     // Assert authority
//     let (authorized, _) = validate_authority(config.clone(), info.clone());
//     if !authorized {
//         return Err(TokenFactoryError::Unauthorized {});
//     }

//     // Validate exactly 2 assets
//     if asset_infos.len() != 2 {
//         return Err(TokenFactoryError::InvalidRouteConfig {
//             reason: "Pair must have exactly 2 assets".to_string(),
//         });
//     }

//     // Check for duplicates
//     if asset_infos[0].equal(&asset_infos[1]) {
//         return Err(TokenFactoryError::DuplicateAssets {});
//     }

//     // Validate PCL params
//     crate::astroport_helpers::validate_pcl_params(&params)?;

//     // Canonicalize asset infos
//     let mut canonical_assets = asset_infos.clone();
//     crate::astroport_helpers::canonicalize_asset_infos(&mut canonical_assets)?;

//     // Get factory address
//     let factory = config.astroport_factory.ok_or(
//         TokenFactoryError::RouterNotConfigured {}
//     )?;

//     // Encode PCL init params
//     let init_params = crate::astroport_helpers::encode_pcl_init_params(&params)?;

//     // Convert to astroport AssetInfo
//     let astroport_assets: Vec<astroport::asset::AssetInfo> = canonical_assets
//         .iter()
//         .map(|a| crate::astroport_helpers::asset_info_to_astroport(a.clone()))
//         .collect();

//     // Build create pair message
//     // For PCL (Passive Concentrated Liquidity) pairs, use Custom variant
//     let create_pair_msg = astroport::factory::ExecuteMsg::CreatePair {
//         pair_type: astroport::factory::PairType::Custom("concentrated".to_string()),
//         asset_infos: astroport_assets,
//         init_params: Some(init_params),
//     };

//     Ok(Response::<NeutronMsg>::new()
//         .add_message(CosmosMsg::Wasm(WasmMsg::Execute {
//             contract_addr: factory.to_string(),
//             msg: to_json_binary(&create_pair_msg)?,
//             funds: vec![],
//         }))
//         .add_attribute("method", "create_pcl_pair")
//         .add_attribute("assets", format!("{:?}", canonical_assets)))
// }

/// Update swap route configuration
fn execute_update_swap_route(
    deps: DepsMut,
    info: MessageInfo,
    route: membrane::neutron_proxy::SwapRouteEntry,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;

    // Assert authority
    let (authorized, owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized || !config.owners[owner_index].non_token_contract_auth {
        return Err(TokenFactoryError::Unauthorized {});
    }

    let key = (route.token_in.clone(), route.token_out.clone());

    if route.remove {
        SWAP_ROUTE_CONFIG.remove(deps.storage, key);
    } else {
        SWAP_ROUTE_CONFIG.save(deps.storage, key, &route.preference)?;
    }

    Ok(Response::<NeutronMsg>::new()
        .add_attribute("method", "update_swap_route")
        .add_attribute("token_in", route.token_in)
        .add_attribute("token_out", route.token_out)
        .add_attribute("remove", route.remove.to_string()))
}

/// Batch update swap route configurations
fn execute_update_swap_routes(
    deps: DepsMut,
    info: MessageInfo,
    routes: Vec<membrane::neutron_proxy::SwapRouteEntry>,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;

    // Assert authority
    let (authorized, owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized || !config.owners[owner_index].non_token_contract_auth {
        return Err(TokenFactoryError::Unauthorized {});
    }

    let routes_count = routes.len();
    for route in routes {
        let key = (route.token_in.clone(), route.token_out.clone());
        if route.remove {
            SWAP_ROUTE_CONFIG.remove(deps.storage, key);
        } else {
            SWAP_ROUTE_CONFIG.save(deps.storage, key, &route.preference)?;
        }
    }

    Ok(Response::<NeutronMsg>::new()
        .add_attribute("method", "update_swap_routes")
        .add_attribute("routes_count", routes_count.to_string()))
}

/// Edit Owner params
/// This function is only callable by an owner with non_token_contract_auth set to true
fn edit_owner(
    deps: DepsMut,
    info: MessageInfo,
    owner: String,
    non_token_contract_auth: bool,
    remove: bool,
) -> Result<Response<NeutronMsg>, TokenFactoryError>{
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    let (authorized, owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized || !config.owners[owner_index].non_token_contract_auth {
        return Err(TokenFactoryError::Unauthorized {});
    }
    let valid_owner_addr = deps.api.addr_validate(&owner)?;

    //Find Owner to edit
    if let Some((owner_index, mut owner)) = config.clone().owners
        .into_iter()
        .enumerate()
        .find(|(_i, owner)| owner.owner == valid_owner_addr){
        //Update Optionals
        owner.non_token_contract_auth = non_token_contract_auth;

        if remove {
            config.owners.remove(owner_index);
        } else {
            config.owners[owner_index] = owner;
        }
        
    } else { return Err(TokenFactoryError::CustomError { val: String::from("Non-existent owner address") }) }

    //Save edited Owner
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::<NeutronMsg>::new().add_attribute("edited_owner", format!("{:?}", config.owners[owner_index])))
}

/// Assert info.sender is an owner
fn validate_authority(config: Config, info: MessageInfo) -> (bool, usize) {
    //Owners && Debt Auction have contract authority
    match config
        .owners
        .into_iter()
        .enumerate()
        .find(|(_i, owner)| owner.owner == info.sender)
    {
        Some((index, _owner)) => (true, index),
        None => (false, 0),        
    }
}

/// Create a new denom using TokenFactory.
/// Saves the denom in the reply.
pub fn create_denom(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    subdenom: String,
    max_supply: Option<Uint128>,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    let (authorized, _owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }

    if subdenom.eq("") {
        return Err(TokenFactoryError::InvalidSubdenom { subdenom });
    }    

    //Create Msg
    let msg = TokenFactory::MsgCreateDenom { sender: env.contract.address.to_string(), subdenom: subdenom.clone() };
    let create_denom_msg = SubMsg::reply_on_success(msg, CREATE_DENOM_REPLY_ID );
    
    //Save PendingTokenInfo
    PENDING.save(deps.storage, &PendingTokenInfo { subdenom: subdenom.clone(), max_supply })?;

    let res = Response::<NeutronMsg>::new()
        .add_attribute("method", "create_denom")
        .add_attribute("sub_denom", subdenom)
        .add_attribute("max_supply", max_supply.unwrap_or_else(Uint128::zero))
        .add_submessage(create_denom_msg);

    Ok(res)
}

/// Change the admin of a denom created from this contract
pub fn change_admin(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    denom: String,
    new_admin_address: String,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {

    let config = CONFIG.load(deps.storage)?;
    //Assert Authority
    let (authorized, _owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }

    deps.api.addr_validate(&new_admin_address)?;

    validate_denom(denom.clone())?;

    let change_admin_msg = TokenFactory::MsgChangeAdmin {
        denom: denom.clone(),
        sender: env.contract.address.to_string(),
        new_admin: new_admin_address.clone(),
    };

    let res = Response::<NeutronMsg>::new()
        .add_attribute("method", "change_admin")
        .add_attribute("denom", denom)
        .add_attribute("new_admin_address", new_admin_address)
        .add_message(change_admin_msg);

    Ok(res)
}

/// Edit token max supply
fn edit_token_max(
    deps: DepsMut,
    info: MessageInfo,
    denom: String,
    max_supply: Uint128,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {

    let config = CONFIG.load(deps.storage)?;
    //Assert Authority
    let (authorized, _owner_index) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }

    //Update Token Max
    TOKENS.update(
        deps.storage,
        denom.clone(),
        |token_info| -> Result<TokenInfo, TokenFactoryError> {
            match token_info {
                Some(mut token_info) => {
                    token_info.max_supply = Some(max_supply);

                    Ok(token_info)
                }
                None => {
                    Err(TokenFactoryError::CustomError {
                        val: String::from("Denom was not created in this contract"),
                    })
                }
            }
        },
    )?;

    //If max supply is changed to under current_supply, it halts new mints.

    Ok(Response::<NeutronMsg>::new().add_attributes(vec![
        attr("method", "edit_token_max"),
        attr("denom", denom),
        attr("new_max", max_supply),
    ]))
}

/// Mint tokens to an address
pub fn mint_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    denom: String,
    amount: Uint128,
    mint_to_address: String,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    let (authorized, _) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }    

    //Validate mint_to_address
    deps.api.addr_validate(&mint_to_address)?;

    if amount.eq(&Uint128::new(0_u128)) {
        return Result::Err(TokenFactoryError::ZeroAmount {});
    }
    //Validate denom
    validate_denom(denom.clone())?;

    //Debt Auction can mint over max supply
    let mut mint_allowed = false;
    if let Some(debt_auction) = config.clone().debt_auction {
        if info.sender == debt_auction {
            mint_allowed = true;
        }
    }; 
    
    //Update Token Supply
    TOKENS.update(
        deps.storage,
        denom.clone(),
        |token_info| -> Result<TokenInfo, TokenFactoryError> {
            match token_info {
                Some(mut token_info) => {
                    if token_info.clone().max_supply.is_some() {
                        if token_info.current_supply <= token_info.max_supply.unwrap()
                            || mint_allowed
                        {
                            token_info.current_supply += amount;
                            mint_allowed = true;
                        }
                    } else {
                        token_info.current_supply += amount;
                        mint_allowed = true;
                    }

                    Ok(token_info)
                }
                None => {
                    Err(TokenFactoryError::CustomError {
                        val: String::from("Denom was not created in this contract"),
                    })
                }
            }
        },
    )?;

    //Create mint msg
    let mint_tokens_msg: CosmosMsg<NeutronMsg> = TokenFactory::MsgMint{
        sender: env.contract.address.to_string(), 
        amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin{
            denom: denom.clone(),
            amount: amount.to_string(),
        }), 
        mint_to_address: mint_to_address.clone(),
    }.into(); 

    let mut res = Response::<NeutronMsg>::new()
        .add_attribute("method", "mint_tokens")
        .add_attribute("mint_status", mint_allowed.to_string())
        .add_attribute("denom", denom.clone())
        .add_attribute("amount", Uint128::zero());

    //If a mint was made/allowed
    if mint_allowed {
        res = Response::<NeutronMsg>::new()
            .add_attribute("method", "mint_tokens")
            .add_attribute("mint_status", mint_allowed.to_string())
            .add_attribute("denom", denom)
            .add_attribute("amount", amount)
            .add_attribute("mint_to_address", mint_to_address)
            .add_messages(vec![mint_tokens_msg]);
    }

    Ok(res)
}

/// Create Osmosis Incentive Gauge.
/// Uses osmosis-std to make it easier for contracts to execute osmosis messages.
// fn create_gauge(
//     gauge_msg: MsgCreateGauge,
// ) -> Result<Response, TokenFactoryError>{
//     Ok(Response::<NeutronMsg>::new().add_message(gauge_msg))
// }

/// Query's Position Basket collateral supplyCaps and finds the owner's ratio of the total supply
// fn get_owner_liquidity_multiplier(
//     querier: QuerierWrapper,
//     liquidity_multiplier: Decimal,
//     owners: Vec<Owner>,
//     owner: Addr,
//     oracle_contract: Addr,
//     positions_contract: Addr,
// ) -> Result<Decimal, TokenFactoryError> {

//     //Initialize variables
//     let mut owner_totals: Vec<(Addr, Decimal)> = vec![];

//     //Get twap_timeframe
//     let cdp_config: CDPConfig = querier.query_wasm_smart(positions_contract, &CDPQueryMsg::Config {  })?;
//     let twap_timeframe = cdp_config.collateral_twap_timeframe;

//     //Get per owner collateral total value
//     for owner in owners {
//         //Must have GetBasket query
//         if owner.is_position_contract {
//             let basket: Basket = querier.query_wasm_smart(owner.clone().owner, &CDPQueryMsg::GetBasket {  })?;
//             let mut total = Decimal::zero();

//             //Parse thru assets and value them
//             for asset in basket.collateral_supply_caps {

//                 //Get Price
//                 let asset_price: PriceResponse = querier.query_wasm_smart(oracle_contract.clone(), &OracleQueryMsg::Price { 
//                     asset_info: asset.asset_info.clone(),
//                     twap_timeframe: twap_timeframe.clone(),
//                     oracle_time_limit: cdp_config.oracle_time_limit,
//                     basket_id: None,
//                 })?;

//                 //Get Value
//                 let asset_value = asset_price.get_value(asset.current_supply)?;

//                 //Add to total
//                 total += asset_value;
//             }

//             owner_totals.push((owner.owner, total));
//         }
//     }

//     //Get total collateral value
//     let mut total_collateral_value = Decimal::zero();
//     for owner in owner_totals.clone() {
//         total_collateral_value += owner.1;
//     }

//     //Get owner's ratio of total collateral value
//     let mut owner_ratio = Decimal::zero();
//     for listed_owner in owner_totals {
//         if listed_owner.0 == owner && total_collateral_value > Decimal::zero(){
//             owner_ratio = decimal_division(listed_owner.1, total_collateral_value)?;
//         }
//     }
    
//     //Return owner's liquidity multiplier
//     Ok(decimal_multiplication(owner_ratio, liquidity_multiplier)?)
// }

/// Burns tokens 
pub fn burn_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    denom: String,
    amount: Uint128,
    burn_from_address: String,
) -> Result<Response<NeutronMsg>, TokenFactoryError> {    
    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    let (authorized, _) = validate_authority(config.clone(), info.clone());
    if !authorized {
        return Err(TokenFactoryError::Unauthorized {});
    }

    if amount.eq(&Uint128::new(0_u128)) {
        return Result::Err(TokenFactoryError::ZeroAmount {});
    }

    validate_denom(denom.clone())?;
    CONFIG.save(deps.storage, &config)?;


    //Update Token Supply
    TOKENS.update(
        deps.storage,
        denom.clone(),
        |token_info| -> Result<TokenInfo, TokenFactoryError> {
            match token_info {
                Some(mut token_info) => {
                    //Update token_info
                    token_info.current_supply -= amount;
                    token_info.burned_supply += amount;
                    
                    Ok(token_info)
                }
                None => {
                    Err(TokenFactoryError::CustomError {
                        val: String::from("Denom was not created in this contract"),
                    })
                }
            }
        },
    )?;

    let burn_token_msg: CosmosMsg<NeutronMsg> = TokenFactory::MsgBurn {
        sender: env.contract.address.to_string(),
        amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin{
            denom,
            amount: amount.to_string(),
        }),
        burn_from_address: burn_from_address.clone(),
    }.into();

    let res = Response::<NeutronMsg>::new()
        .add_attribute("method", "burn_tokens")
        .add_attribute("amount", amount)
        .add_attribute("burn_from_address", burn_from_address)
        .add_message(burn_token_msg);

    Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config { } => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::GetOwner { owner } => to_json_binary(&get_contract_owner(deps, owner)?),
        QueryMsg::GetDenom {
            creator_address,
            subdenom,
        } => to_json_binary(&get_denom(deps, creator_address, subdenom)?),
        QueryMsg::GetContractDenoms { limit } => to_json_binary(&get_contract_denoms(deps, limit)?),
        // QueryMsg::PoolState { id } => to_json_binary(&get_pool_state(deps, id)?),
        QueryMsg::GetTokenInfo { denom } => to_json_binary(&get_token_info(deps, denom)?),
        QueryMsg::GetSwapRoutes { } => to_json_binary(&SWAP_ROUTES.load(deps.storage)?),
        QueryMsg::GetSwapRouteConfig { token_in, token_out } => {
            to_json_binary(&get_swap_route_config(deps, token_in, token_out)?)
        },
        QueryMsg::SimulateSwap { token_in, token_out, amount_in } => {
            to_json_binary(&simulate_swap(deps, env, token_in, token_out, amount_in)?)
        },
        QueryMsg::AstroportPairInfo { asset_infos } => {
            to_json_binary(&get_astroport_pair_info(deps, asset_infos)?)
        },
    }
}

/// Returns state data regarding a specified contract owner
fn get_contract_owner(deps: Deps, owner: String) -> StdResult<NeutronOwner> {
    let config = CONFIG.load(deps.storage)?;
    if let Some(owner) = config.clone().owners.into_iter().find(|stored_owner| stored_owner .owner == owner) {

        // If we end up with multiple positions contracts, we'll need to query OP's total minted in the Positions contracts instead of only using the Basket's total minted

        Ok(owner)
    } else {
        Err(StdError::generic_err("Owner not found"))
    }
}

/// Returns token info for a specified denom
fn get_token_info(deps: Deps, denom: String) -> StdResult<TokenInfoResponse> {
    let token_info = TOKENS.load(deps.storage, denom.clone())?;
    
    Ok(TokenInfoResponse {
        denom,
        current_supply: token_info.current_supply,
        max_supply: token_info.max_supply.unwrap_or_else(Uint128::zero),
        burned_supply: token_info.burned_supply,
    })
    
}

/// Returns a list of all denoms created by this contract
fn get_contract_denoms(deps: Deps, limit: Option<u32>) -> StdResult<ContractDenomsResponse> {
    let limit = limit.unwrap_or_else(|| MAX_LIMIT);

    let denoms = 
        TOKENS
            .range(deps.storage, None, None, Order::Ascending)
            .take(limit as usize)
            .map(|info|{
                if let Ok(info) = info {
                    info.0
                } else { String::from("error") }
            })
            .collect::<Vec<String>>();

    Ok(
        ContractDenomsResponse {
            denoms,
        }
    )
}

/// Get swap route configuration for a pair
fn get_swap_route_config(
    deps: Deps,
    token_in: String,
    token_out: String,
) -> StdResult<membrane::neutron_proxy::SwapRouteConfigResponse> {
    let preference = SWAP_ROUTE_CONFIG.may_load(deps.storage, (token_in, token_out))?;
    Ok(membrane::neutron_proxy::SwapRouteConfigResponse { preference })
}

/// Simulate swap on both DEXes
fn simulate_swap(
    deps: Deps,
    _env: Env,
    token_in: String,
    token_out: String,
    amount_in: Uint128,
) -> StdResult<membrane::neutron_proxy::SimulateSwapResponse> {
    let config = CONFIG.load(deps.storage)?;
    
    let coin_in = Coin {
        denom: token_in.clone(),
        amount: amount_in,
    };

    // Query Duality output (not available, return None)
    let duality_output: Option<Uint128> = None;

    // Query Astroport output
    let astroport_output = if let Some(ref factory) = config.astroport_factory {
        let asset_in = AssetInfo::NativeToken {
            denom: token_in.clone(),
        };
        let asset_out = AssetInfo::NativeToken {
            denom: token_out.clone(),
        };
        
        match crate::astroport_helpers::resolve_astroport_pair(
            &deps.querier,
            factory,
            &[asset_in, asset_out.clone()],
        ) {
            Ok(pair_addr) => {
                crate::astroport_helpers::query_astroport_swap_output(
                    &deps.querier,
                    &pair_addr,
                    &coin_in,
                    &asset_out,
                ).ok()
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // Determine best DEX
    let best_dex = crate::astroport_helpers::choose_best_dex(
        duality_output,
        astroport_output,
    ).ok();

    Ok(membrane::neutron_proxy::SimulateSwapResponse {
        duality_output,
        astroport_output,
        best_dex,
    })
}

/// Get Astroport pair info
fn get_astroport_pair_info(
    deps: Deps,
    asset_infos: Vec<AssetInfo>,
) -> StdResult<membrane::neutron_proxy::AstroportPairInfoResponse> {
    let config = CONFIG.load(deps.storage)?;
    let factory = config.astroport_factory.ok_or_else(|| {
        StdError::generic_err("Astroport factory not configured")
    })?;

    if asset_infos.len() != 2 {
        return Err(StdError::generic_err("Pair must have exactly 2 assets"));
    }

    // Canonicalize
    let mut canonical_assets = asset_infos.clone();
    crate::astroport_helpers::canonicalize_asset_infos(&mut canonical_assets)
        .map_err(|e| StdError::generic_err(e.to_string()))?;

    // Convert to astroport AssetInfo
    let astroport_assets: Vec<astroport::asset::AssetInfo> = canonical_assets
        .iter()
        .map(|a| crate::astroport_helpers::asset_info_to_astroport(a.clone()))
        .collect();

    // Query factory - use a response struct that matches the actual API
    #[derive(serde::Deserialize)]
    struct PairInfoResponse {
        pub contract_addr: Addr,
        pub liquidity_token: Addr,
        pub pair_type: String,
    }
    
    let pair_info: PairInfoResponse = deps.querier.query_wasm_smart(
        &factory,
        &astroport::factory::QueryMsg::Pair {
            asset_infos: astroport_assets,
        },
    )?;

    Ok(membrane::neutron_proxy::AstroportPairInfoResponse {
        pair_addr: pair_info.contract_addr.to_string(),
        lp_token: pair_info.liquidity_token.to_string(),
        pair_type: pair_info.pair_type,
    })
}

/// Returns denom for a specified creator address and subdenom
fn get_denom(deps: Deps, creator_addr: String, subdenom: String) -> StdResult<GetDenomResponse> {
    let response: QueryDenomsFromCreatorResponse = TokenFactory::TokenfactoryQuerier::new(&deps.querier).denoms_from_creator(creator_addr)?;

    let denom = if let Some(denom) = response.denoms.into_iter().find(|denoms| denoms.contains(&subdenom)){
        denom
    } else {
        return Err(StdError::GenericErr { msg: String::from("Can't find subdenom in list of contract denoms") })
    };

    Ok(GetDenomResponse {
        denom,
    })
}

/// Validate token factory denom
pub fn validate_denom(denom: String) -> Result<(), TokenFactoryError> {
    let denom_to_split = denom.clone();
    let tokenfactory_denom_parts: Vec<&str> = denom_to_split.split('/').collect();

    if tokenfactory_denom_parts.len() != 3 {
        return Result::Err(TokenFactoryError::InvalidDenom {
            denom,
            message: std::format!(
                "denom must have 3 parts separated by /, had {}",
                tokenfactory_denom_parts.len()
            ),
        });
    }

    let prefix = tokenfactory_denom_parts[0];

    if !prefix.eq_ignore_ascii_case("factory") {
        return Result::Err(TokenFactoryError::InvalidDenom {
            denom,
            message: std::format!("prefix must be 'factory', was {}", prefix),
        });
    }

    Result::Ok(())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response<NeutronMsg>> {
    match msg.id {
        CREATE_DENOM_REPLY_ID => handle_create_denom_reply(deps, env, msg),
        SWAP_REPLY_ID => handle_swap_reply(deps, env, msg),
        USE_BALANCE_SWAP_REPLY_ID => handle_swap_balances_reply(deps, env, msg),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

fn handle_swap_balances_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response<NeutronMsg>> {
    match msg.result.into_result() {
        Ok(_) => {
            let config = CONFIG.load(deps.storage)?;
            //Get swapper
            let swap_info = SWAP_INFO.load(deps.storage)?;

            //Swap all assets in the contract
            let balances = deps.querier.query_all_balances(&env.contract.address)?;

            //Filter out transmutation token_ins
            let transmutation_token_ins: Vec<String> = config.transmutation_pairs.into_iter().map(|pair| pair.token_in).collect();
            let filtered_funds: Vec<Coin> = balances.iter().filter(|coin| !transmutation_token_ins.contains(&coin.denom)).cloned().collect();
            let _filtered_denoms: Vec<String> = balances.iter().filter(|coin| transmutation_token_ins.contains(&coin.denom)).map(|coin| coin.denom.clone()).collect();

            //If no funds left after filtering, error
            if filtered_funds.is_empty() {
                return Err(StdError::GenericErr { msg: String::from("No funds left after filtering") });
            }
            //Execute swap with new balances
            let res = match execute_swaps(
                deps, 
                env, 
                swap_info.swapper.clone(),
                filtered_funds.clone(),
                swap_info.token_out.clone(),
                swap_info.max_slippage.clone(),
            ){
                Ok(res) => res,
                Err(err) => return Err(StdError::GenericErr { msg: err.to_string() }),
            };  

            return Ok(res
            .add_attribute("swap_info", format!("{:?}", swap_info))
            .add_attribute("tokens_received", format!("{:?}", filtered_funds)))
        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
}

fn handle_swap_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response<NeutronMsg>> {
    match msg.result.into_result() {
        Ok(_) => {
            //Load config
            let config = CONFIG.load(deps.storage)?;
            //Get swapper
            let swap_info = SWAP_INFO.load(deps.storage)?;
            let swapper = swap_info.clone().swapper;


            // Current balances
            let balances = deps.querier.query_all_balances(&env.contract.address)?;

            // Filter out transmutation token_ins
            let transmutation_token_ins: Vec<String> = config.transmutation_pairs.into_iter().map(|pair| pair.token_in).collect();

            // Compute delta: only send newly received funds (current - previous), excluding token_ins
            let mut new_funds: Vec<Coin> = Vec::new();
            for coin in balances.iter() {
                if transmutation_token_ins.contains(&coin.denom) {
                    continue;
                }
                let prev_amount = swap_info
                    .prev_balances
                    .iter()
                    .find(|c| c.denom == coin.denom)
                    .map(|c| c.amount)
                    .unwrap_or(Uint128::zero());
                if coin.amount > prev_amount {
                    new_funds.push(Coin { denom: coin.denom.clone(), amount: coin.amount - prev_amount });
                }
            }

            // If no new funds were received, do not send anything
            if new_funds.is_empty() {
                return Err(StdError::GenericErr { msg:  format!("No new funds received. Old balances: {:?} --- New balances: {:?}", swap_info.prev_balances, balances) });
            }

            // Send only the newly received funds to the swapper
            let msg: CosmosMsg<NeutronMsg> = CosmosMsg::Bank(BankMsg::Send {
                to_address: swapper.clone().to_string(),
                amount: new_funds.clone(),
            });

            //Remove swapper
            // SWAP_INFO.remove(deps.storage);
            //Don't remove incase we have 2 swap replies due to a special exit

            return Ok(Response::<NeutronMsg>::new()
            .add_attribute("swap_info", format!("{:?}", swap_info))
            .add_attribute("tokens_received", format!("{:?}", new_funds))
            .add_message(msg))
        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
}

/// Find & save created full denom
fn handle_create_denom_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> StdResult<Response<NeutronMsg>> {
    match msg.result.into_result() {
        Ok(result) => {
            //Load Pending TokenInfo
            let PendingTokenInfo { subdenom:_, max_supply} = PENDING.load(deps.storage)?;

            if let Some(b) = result.data {
                let res: MsgCreateDenomResponse = match b.try_into().map_err(TokenFactoryError::Std){
                    Ok(res) => res,
                    Err(err) => return Err(StdError::GenericErr { msg: String::from(err.to_string()) })
                };
                //Save Denom Info
                TOKENS.save(
                    deps.storage,
                    res.new_token_denom.clone(),
                    &TokenInfo {
                        current_supply: Uint128::zero(),
                        max_supply,
                        burned_supply: Uint128::zero(),
                    },
                )?;
            } else {
                return Err(StdError::GenericErr { msg: String::from("No data in reply") })
            }
        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
    Ok(Response::<NeutronMsg>::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response<NeutronMsg>, TokenFactoryError> {
    //Query routes from the test OP
    // let routes: Vec<SwapRoute> = deps.querier.query_wasm_smart::<Vec<SwapRoute>>(
    //     "osmo1968gjpryrmvkydzw47dfdae0p9jzy43p4ckr9geswekm73j4ufkq5tz07q".to_string(),
    //     &QueryMsg::GetSwapRoutes {  }
    // )?;
    // //Update current routes
    // SWAP_ROUTES.save(deps.storage, &routes)?;
    

    Ok(Response::default()
    // .add_attribute("routes", format!("{:?}", routes))
)
}