// car_nft/src/contract.rs

use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, Uint128, WasmMsg,
};
use cw2::set_contract_version;
use cw721_base::{Cw721Contract, ExecuteMsg as Cw721ExecuteMsg, InstantiateMsg as Cw721InstantiateMsg, MintMsg};

use crate::error::CarError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{CAR_ID_COUNTER, CONFIG, PENDING_OWNER};
use racing::types::CarMetadata;
use racing::car::Config;
use racing::traits_engine::{default_rarity_table, generate_traits_with_rarity, traits_to_attributes};

const CONTRACT_NAME: &str = "car_nft";
const CONTRACT_VERSION: &str = "0.1.0";

// Plug our extension into cw721-base
pub type CarCw721<'a> = Cw721Contract<'a, Option<CarMetadata>, cosmwasm_std::Empty, cosmwasm_std::Empty, cosmwasm_std::Empty>;

#[entry_point]
pub fn instantiate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, CarError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Initialize car ID counter to 0
    CAR_ID_COUNTER.save(deps.storage, &Uint128::zero())?;

    // Save owner and payment options
    let owner = info.sender.clone();
    let payment_options = msg.payment_options.unwrap_or_default();
    CONFIG.save(deps.storage, &Config { owner: owner.clone(), payment_options })?;

    // Set minter to this contract address so only self-calls can mint
    let cw_msg = Cw721InstantiateMsg {
        name: msg.name,
        symbol: msg.symbol,
        minter: env.contract.address.to_string(),
    };

    let contract: CarCw721 = Cw721Contract::default();
    let resp = contract
        .instantiate(deps, env.clone(), info, cw_msg)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;

    Ok(resp.add_attribute("minter", env.contract.address))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, CarError> {
    match msg {
        ExecuteMsg::Base(base) => {
            let contract: CarCw721 = Cw721Contract::default();
            contract
                .execute(deps, env, info, base)
                .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
                .map_err(CarError::from)
        }
        ExecuteMsg::MintCar { owner, token_uri, extension } => execute_mint_car(deps, env, info, owner, token_uri, extension),
        ExecuteMsg::UpdateConfig { payment_options, new_owner } => execute_update_config(deps, info, payment_options, new_owner),
        ExecuteMsg::UpdateCustomDecal { token_id, svg } => execute_update_custom_decal(deps, info, token_id, svg),
    }
}

fn execute_update_config(
    mut deps: DepsMut,
    info: MessageInfo,
    payment_options: Option<Vec<Coin>>,
    new_owner: Option<String>,
) -> Result<Response, CarError> {
    let mut config = CONFIG.load(deps.storage)?;
    let current_owner = config.owner.clone();

    if info.sender != current_owner {
        // Sender is not the current owner: check pending owner
        if let Ok(pending) = PENDING_OWNER.load(deps.storage){
            if info.sender == pending {
                // Promote pending owner to owner and clear pending
                config.owner = pending.clone();
                PENDING_OWNER.remove(deps.storage);
            } else {
                return Err(CarError::Unauthorized {});
            }
        } else {
            return Err(CarError::Unauthorized {});
        }
    } 
    
    if let Some(new_owner_str) = new_owner {
        // Current owner initiating transfer -> set pending
        let new_addr = deps.api.addr_validate(&new_owner_str)?;
        PENDING_OWNER.save(deps.storage, &new_addr)?;
    }

    // Update config
    if let Some(payment_options) = payment_options {
        config.payment_options = payment_options;
    }
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

fn execute_mint_car(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    token_uri: Option<String>,
    mut extension: Option<CarMetadata>,
) -> Result<Response, CarError> {
    // Enforce payment: at least one of the configured options must be present in funds
    let config = CONFIG.load(deps.storage)?;
    if !config.payment_options.is_empty() {
        let sent = &info.funds;
        let mut ok = false;
        for Coin { denom, amount } in config.payment_options.iter() {
            if sent.iter().any(|c| c.denom == *denom && c.amount >= *amount) {
                ok = true;
                break;
            }
        }
        if !ok {
            return Err(CarError::Std(cosmwasm_std::StdError::generic_err("insufficient payment: must include at least one accepted option")));
        }
    }

    // Generate incremental token_id from CAR_ID_COUNTER
    let next_id = CAR_ID_COUNTER.load(deps.storage)?;
    let token_id = next_id.to_string();
    CAR_ID_COUNTER.save(deps.storage, &(next_id + Uint128::one()))?;

    // Populate car_id in metadata
    if let Some(meta) = &mut extension {
        meta.car_id = Some(token_id.clone());
    } else {
        extension = Some(CarMetadata {
            name: String::new(),
            image_data: None,
            attributes: None,
            car_id: Some(token_id.clone()),
        });
    }

    // Build a deterministic seed from known data
    fn mix64(mut x: u64) -> u64 {
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51afd7ed558ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
        x ^ (x >> 33)
    }
    fn hash_str(s: &str) -> u64 {
        let mut h: u64 = 1469598103934665603; // FNV offset basis
        for b in s.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        mix64(h)
    }
    let seed = mix64(env.block.height as u64)
        ^ mix64(env.block.time.nanos())
        ^ hash_str(&token_id)
        ^ hash_str(&owner)
        ^ hash_str(info.sender.as_str());

    // Generate traits + rarity and append as metadata attributes
    let table = default_rarity_table();
    let (traits, breakdown) = generate_traits_with_rarity(seed, &table);
    let mut to_add = traits_to_attributes(&traits, &breakdown);

    if let Some(meta) = &mut extension {
        let mut attrs = meta.attributes.take().unwrap_or_default();
        attrs.append(&mut to_add);
        meta.attributes = Some(attrs);
    }

    // Perform a self-call to cw721-base Mint
    let self_mint = Cw721ExecuteMsg::<Option<CarMetadata>, cosmwasm_std::Empty>::Mint(MintMsg {
        token_id,
        owner,
        token_uri,
        extension,
    });

    let msg = WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&self_mint)?,
        funds: vec![],
    };

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "mint_car"))
}

fn execute_update_custom_decal(
    mut deps: DepsMut,
    info: MessageInfo,
    token_id: String,
    svg: String,
) -> Result<Response, CarError> {
    // Only token owner may update
    let contract: CarCw721 = Cw721Contract::default();
    let token = contract.tokens.load(deps.storage, &token_id)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;
    if token.owner != info.sender {
        return Err(CarError::Unauthorized {});
    }

    // Load current metadata (clone to avoid moving from token)
    let mut ext = token.extension.clone().unwrap_or(CarMetadata {
        name: String::new(),
        image_data: None,
        attributes: None,
        car_id: Some(token_id.clone()),
    });

    // Ensure the car has a custom slot either set or empty
    // We update the attributes list: find existing decal attribute and set to raw SVG
    let mut has_decal_attr = false;
    if let Some(attrs) = &mut ext.attributes {
        for a in attrs.iter_mut() {
            if a.trait_type == "decal" {
                // Prevent editing preset decals
                if a.value.starts_with("Preset::") {
                    return Err(CarError::NotCustomDecal {});
                }
                has_decal_attr = true;
                a.value = svg.clone();
                break;
            }
        }
        if !has_decal_attr {
            attrs.push(racing::types::CarAttribute { trait_type: "decal".to_string(), value: svg.clone() });
            has_decal_attr = true;
        }
    } else {
        ext.attributes = Some(vec![racing::types::CarAttribute { trait_type: "decal".to_string(), value: svg.clone() }]);
        has_decal_attr = true;
    }

    // Persist new metadata by updating token via cw721-base extension replace
    let mut token_mut = token;
    token_mut.extension = Some(ext);
    contract.tokens.save(deps.storage, &token_id, &token_mut)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;

    Ok(Response::new()
        .add_attribute("action", "update_custom_decal")
        .add_attribute("token_id", token_id))
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Base(q) => {
            let contract: CarCw721 = Cw721Contract::default();
            contract.query(deps, env, q)
        }
    }
}

