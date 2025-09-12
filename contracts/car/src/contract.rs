// car_nft/src/contract.rs

use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, Uint128, WasmMsg,
};
use cw2::set_contract_version;
use cw721_base::state::TokenInfo;
use cw721_base::{Cw721Contract, ExecuteMsg as Cw721ExecuteMsg, InstantiateMsg as Cw721InstantiateMsg, MintMsg};

use crate::error::CarError;
use membrane::car::{ExecuteMsg, InstantiateMsg, QueryMsg, MigrateMsg, Config, MAX_NAME_SIZE};
use crate::state::{CAR_ID_COUNTER, CONFIG, PENDING_OWNER};
use membrane::types::{CarMetadata, StringEntry};
use membrane::traits_engine::{default_rarity_table, generate_traits_with_rarity, traits_to_attributes};
use crate::state::USED_TRAIT_COMBOS;
use crate::state::NAME_REGISTRY;
use crate::state::{PENDING_FREE_CARS, PendingFreeCar, CAR_INFO, set_car_info, CarInfo};
use cosmwasm_std::Addr;
use crate::base_nft_msgs::{execute_transfer_nft, execute_send_nft, execute_approve, execute_revoke, execute_approve_all, execute_revoke_all, execute_mint, execute_burn, execute_extension};
use crate::base_nft_queries::{
    query_owner_of, query_approval, query_approvals, query_all_operators, query_num_tokens,
    query_contract_info, query_nft_info, query_all_nft_info, query_tokens, query_all_tokens, query_minter
};
use membrane::traits_engine::{
    CarTraits,
    BaseColor, AccentPattern, PaintFinish, HeadlightColor, UnderglowColor, BrakeLightStyle,
    FrontBumperStyle, SpoilerType, RoofType, FenderStyle, ExhaustLength, ExhaustTipStyle,
    EngineVisuals, RimStyle, RimColor, TireType, NumberFont, RoofAccessory, SideMirror,
    WindowTint, TrailEffect, Decal, DecalPreset,
};
// use crate::state::get_car_info;

const CONTRACT_NAME: &str = "car_nft";
const CONTRACT_VERSION: &str = "0.1.0";

// Plug our extension into cw721-base
pub type CarCw721<'a> = Cw721Contract<'a, Option<CarMetadata>, cosmwasm_std::Empty, cosmwasm_std::Empty, cosmwasm_std::Empty>;

// Produce a compact u128 key for a name (trimmed), using a stable hash
fn name_key(name_trimmed: &str) -> u128 {
    // 128-bit hash via XXH3-style mixing (deterministic). Keep simple but stable.
    let mut h: u128 = 0x9E37_79B9_7F4A_7C15_6C8E_9CF5_9D1B_BCD7u128;
    for b in name_trimmed.as_bytes() {
        h ^= (*b as u128).wrapping_mul(0x100_0000_01B3);
        h = h.rotate_left(13).wrapping_mul(0xC2B2_AE3D_27D4_EB4Fu128);
    }
    h
}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, CarError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Initialize car ID counter to 1.
    //ID 0 is reserved for The Singularity..
    CAR_ID_COUNTER.save(deps.storage, &Uint128::one())?;

    // Save owner and payment options
    let owner = info.sender.clone();
    let payment_options = msg.payment_options.unwrap_or_default();
    CONFIG.save(
        deps.storage, 
        &Config { 
            owner: owner.clone(), 
            payment_options, 
            race_engine_contract: None,
            revenue_contract: None,
            max_energy: 100,
            energy_recovery_hours: 24,
            energy_per_training: 5,
            training_payment_options: vec![],
            valid_energy_consumers: Some(vec![]),
        }
    )?;

    // Register reserved name to enforce uniqueness
    let reserved_key = name_key("The Singularity");
    NAME_REGISTRY.save(deps.storage, reserved_key, &true)?;

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

    // Mint the reserved token_id "0" named "The Singularity" to the contract owner
    let singularity_ext = Some(CarMetadata {
        name: "The Singularity".to_string(),
        image_data: None,
        attributes: None,
        car_id: Some("0".to_string()),
    });

    let self_mint = ExecuteMsg::<Option<CarMetadata>, cosmwasm_std::Empty>::Mint(MintMsg {
        token_id: "0".to_string(),
        owner: owner.to_string(),
        token_uri: None,
        extension: singularity_ext,
    });
    let msg = WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&self_mint)?,
        funds: vec![],
    };

    Ok(resp
        .add_message(msg)
        .add_attribute("minter", env.contract.address))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg<Option<CarMetadata>, cosmwasm_std::Empty>,
) -> Result<Response, CarError> {
    match msg {
        ExecuteMsg::TransferNft { recipient, token_id } => execute_transfer_nft(deps, env, info, recipient, token_id),
        ExecuteMsg::SendNft { contract, token_id, msg } => execute_send_nft(deps, env, info, contract, token_id, msg),
        ExecuteMsg::Approve { spender, token_id, expires } => execute_approve(deps, env, info, spender, token_id, expires),
        ExecuteMsg::Revoke { spender, token_id } => execute_revoke(deps, env, info, spender, token_id),
        ExecuteMsg::ApproveAll { operator, expires } => execute_approve_all(deps, env, info, operator, expires),
        ExecuteMsg::RevokeAll { operator } => execute_revoke_all(deps, env, info, operator),
        ExecuteMsg::Mint(mint) => execute_mint(deps, env, info, mint),
        ExecuteMsg::Burn { token_id } => execute_burn(deps, env, info, token_id),
        ExecuteMsg::Extension { msg } => execute_extension(deps, env, info, msg),
        ExecuteMsg::CreateCar { name, owner, token_uri } => execute_mint_car(deps, env, info, name, owner, token_uri),
        ExecuteMsg::UpdateConfig { payment_options, new_owner, race_engine_contract, revenue_contract, energy_consumers } => execute_update_config(deps, info, payment_options, new_owner, race_engine_contract, revenue_contract, energy_consumers),
        ExecuteMsg::UpdateEnergyParams { max_energy, energy_recovery_hours, energy_per_training } => execute_update_energy_params(deps, info, max_energy, energy_recovery_hours, energy_per_training),
        ExecuteMsg::UpdateTrainingPayments { training_payment_options } => execute_update_training_payments(deps, info, training_payment_options),
        ExecuteMsg::UpdateCustomDecal { token_id, svg } => execute_update_custom_decal(deps, info, token_id, svg),
        ExecuteMsg::UpdateCarName { token_id, new_name } => execute_update_car_name(deps, info, token_id, new_name),
        ExecuteMsg::PayToFinalize { token_id } => execute_pay_to_finalize(deps, env, info, token_id),
        ExecuteMsg::ExpireCar { token_id } => execute_expire_car(deps, env, info, token_id),
        ExecuteMsg::PayForTraining { token_id } => execute_pay_for_training(deps, env, info, token_id),
        ExecuteMsg::ConsumeTrainingEnergy { token_id, sessions } => execute_consume_training_energy(deps, env, info, token_id, sessions),
    }
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    payment_options: Option<Vec<Coin>>,
    new_owner: Option<String>,
    race_engine_contract: Option<String>,
    revenue_contract: Option<String>,
    energy_consumers: Option<StringEntry>,
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
    if let Some(race_engine_contract) = race_engine_contract {
        if !race_engine_contract.is_empty() {
            let _ = deps.api.addr_validate(&race_engine_contract)?;
            config.race_engine_contract = Some(race_engine_contract);
        } else {
            config.race_engine_contract = None;
        }
    }
    if let Some(revenue_contract) = revenue_contract {
        if !revenue_contract.is_empty() {
            let _ = deps.api.addr_validate(&revenue_contract)?;
            config.revenue_contract = Some(revenue_contract);
        } else {
            config.revenue_contract = None;
        }
    }
    
    // Handle energy_consumers update
    if let Some(energy_consumers) = energy_consumers {
        let entry = energy_consumers.entry;
        let remove = energy_consumers.remove;
        
        // Validate the address if adding
        if !remove {
            let _ = deps.api.addr_validate(&entry)?;
        }
        
        if remove {
            // Remove the entry if it exists
            if let Some(ref mut consumers) = config.valid_energy_consumers {
                consumers.retain(|e| e != &entry);
            }
        } else {
            // Add the entry if it doesn't already exist
            match &mut config.valid_energy_consumers {
                Some(ref mut consumers) => {
                    if consumers.contains(&entry) {
                        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("energy consumer already exists")));
                    }
                    consumers.push(entry);
                }
                None => {
                    config.valid_energy_consumers = Some(vec![entry]);
                }
            }
        }
    }
    
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

fn execute_update_energy_params(
    deps: DepsMut,
    info: MessageInfo,
    max_energy: Option<u32>,
    energy_recovery_hours: Option<u32>,
    energy_per_training: Option<u32>,
) -> Result<Response, CarError> {
    let mut config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(CarError::Unauthorized {});
    }
    if let Some(v) = max_energy { config.max_energy = v; }
    if let Some(v) = energy_recovery_hours { config.energy_recovery_hours = v; }
    if let Some(v) = energy_per_training { config.energy_per_training = v; }
    CONFIG.save(deps.storage, &config)?;
    Ok(Response::new().add_attribute("action", "update_energy_params"))
}

fn execute_update_training_payments(
    deps: DepsMut,
    info: MessageInfo,
    training_payment_options: Vec<Coin>,
) -> Result<Response, CarError> {
    let mut config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner { return Err(CarError::Unauthorized {}); }
    config.training_payment_options = training_payment_options;
    CONFIG.save(deps.storage, &config)?;
    Ok(Response::new().add_attribute("action", "update_training_payments"))
}

fn encode_traits_combo(t: &CarTraits) -> u64 {
    // Assign compact ordinals per enum
    fn base_color_idx(v: &BaseColor) -> u64 { match v {
        BaseColor::Black => 0, BaseColor::White => 1, BaseColor::Silver => 2, BaseColor::Gray => 3,
        BaseColor::Red => 4, BaseColor::Blue => 5, BaseColor::Green => 6, BaseColor::Yellow => 7,
        BaseColor::Orange => 8, BaseColor::Purple => 9, BaseColor::Teal => 10, BaseColor::Gold => 11,
        BaseColor::CustomHex(_) => 12,
    }}
    fn accent_idx(v: &AccentPattern) -> u64 { match v {
        AccentPattern::None => 0, AccentPattern::Stripes => 1, AccentPattern::Flames => 2,
        AccentPattern::Camo => 3, AccentPattern::Geometric => 4, AccentPattern::Gradient => 5,
    }}
    fn paint_idx(v: &PaintFinish) -> u64 { match v {
        PaintFinish::Solid => 0, PaintFinish::Metallic => 1, PaintFinish::Matte => 2, PaintFinish::Pearlescent => 3,
    }}
    fn headlight_idx(v: &HeadlightColor) -> u64 { match v {
        HeadlightColor::White => 0, HeadlightColor::Blue => 1, HeadlightColor::NeonGreen => 2, HeadlightColor::Pink => 3,
    }}
    fn underglow_idx(v: &UnderglowColor) -> u64 { match v {
        UnderglowColor::None => 0, UnderglowColor::White => 1, UnderglowColor::Blue => 2,
        UnderglowColor::Purple => 3, UnderglowColor::Red => 4, UnderglowColor::Green => 5,
    }}
    fn brake_idx(v: &BrakeLightStyle) -> u64 { match v {
        BrakeLightStyle::ClassicRect => 0, BrakeLightStyle::SlimStrip => 1, BrakeLightStyle::Circular => 2, BrakeLightStyle::SplitPanel => 3,
    }}
    fn front_bumper_idx(v: &FrontBumperStyle) -> u64 { match v {
        FrontBumperStyle::Standard => 0, FrontBumperStyle::SportAggressive => 1, FrontBumperStyle::OffroadReinforced => 2, FrontBumperStyle::Retro => 3,
    }}
    fn spoiler_idx(v: &SpoilerType) -> u64 { match v {
        SpoilerType::None => 0, SpoilerType::SmallLip => 1, SpoilerType::Ducktail => 2, SpoilerType::LargeGtWing => 3,
    }}
    fn roof_idx(v: &RoofType) -> u64 { match v {
        RoofType::Hardtop => 0, RoofType::Convertible => 1, RoofType::Sunroof => 2, RoofType::Targa => 3,
    }}
    fn fender_idx(v: &FenderStyle) -> u64 { match v {
        FenderStyle::Stock => 0, FenderStyle::Widebody => 1, FenderStyle::RetroFlare => 2, FenderStyle::AeroCutout => 3,
    }}
    fn exhaust_len_idx(v: &ExhaustLength) -> u64 { match v {
        ExhaustLength::Short => 0, ExhaustLength::Mid => 1, ExhaustLength::Long => 2,
    }}
    fn exhaust_tip_idx(v: &ExhaustTipStyle) -> u64 { match v {
        ExhaustTipStyle::Round => 0, ExhaustTipStyle::Square => 1, ExhaustTipStyle::Angled => 2, ExhaustTipStyle::DualPipe => 3,
    }}
    fn engine_idx(v: &EngineVisuals) -> u64 { match v {
        EngineVisuals::Covered => 0, EngineVisuals::ChromePipes => 1, EngineVisuals::VisibleIntercooler => 2, EngineVisuals::PaintedValveCover => 3,
    }}
    fn rim_style_idx(v: &RimStyle) -> u64 { match v {
        RimStyle::Classic5Spoke => 0, RimStyle::FuturisticSolid => 1, RimStyle::Mesh => 2, RimStyle::DeepDish => 3,
    }}
    fn rim_color_idx(v: &RimColor) -> u64 { match v {
        RimColor::Chrome => 0, RimColor::Black => 1, RimColor::Gold => 2, RimColor::Custom => 3,
    }}
    fn tire_idx(v: &TireType) -> u64 { match v {
        TireType::Slicks => 0, TireType::SemiSlicks => 1, TireType::OffroadTread => 2, TireType::Whitewall => 3,
    }}
    fn number_font_idx(v: &NumberFont) -> u64 { match v {
        NumberFont::BoldBlock => 0, NumberFont::ScriptItalic => 1, NumberFont::RetroStencil => 2, NumberFont::Digital => 3,
    }}
    fn roof_acc_idx(v: &RoofAccessory) -> u64 { match v {
        RoofAccessory::None => 0, RoofAccessory::LightBar => 1, RoofAccessory::Antenna => 2, RoofAccessory::RoofRack => 3,
    }}
    fn side_mirror_idx(v: &SideMirror) -> u64 { match v {
        SideMirror::Standard => 0, SideMirror::AeroSmall => 1, SideMirror::RetroRound => 2, SideMirror::WideRacing => 3,
    }}
    fn tint_idx(v: &WindowTint) -> u64 { match v {
        WindowTint::None => 0, WindowTint::Light => 1, WindowTint::Medium => 2, WindowTint::Dark => 3, WindowTint::Colored => 4,
    }}
    fn trail_idx(v: &TrailEffect) -> u64 { match v {
        TrailEffect::None => 0, TrailEffect::Smoke => 1, TrailEffect::Sparks => 2, TrailEffect::NeonStreak => 3,
    }}
    fn decal_idx(v: &Decal) -> u64 { match v {
        Decal::Preset { preset } => match preset {
            DecalPreset::FlamesA => 0, DecalPreset::FlamesB => 1, DecalPreset::CamoDesert => 2,
            DecalPreset::CamoUrban => 3, DecalPreset::GeometricLines => 4, DecalPreset::SponsorPackA => 5,
            DecalPreset::SponsorPackB => 6,
        },
        Decal::Custom(_) => 7,
    }}

    // Pack into u64 in a fixed order, LSB-first
    let mut acc: u64 = 0;
    let mut shift: u32 = 0;
    macro_rules! put {
        ($val:expr, $bits:expr) => {{
            acc |= (($val as u64) & ((1u64 << $bits) - 1)) << shift;
            shift += $bits;
        }};
    }

    put!(base_color_idx(&t.base_color), 4);
    put!(accent_idx(&t.accent_pattern), 3);
    put!(paint_idx(&t.paint_finish), 2);
    put!(headlight_idx(&t.headlight_color), 2);
    put!(underglow_idx(&t.underglow), 3);
    put!(brake_idx(&t.brake_light_style), 2);
    put!(front_bumper_idx(&t.front_bumper), 2);
    put!(spoiler_idx(&t.spoiler), 2);
    put!(roof_idx(&t.roof), 2);
    put!(fender_idx(&t.fender), 2);
    put!(exhaust_len_idx(&t.exhaust_length), 2);
    put!(exhaust_tip_idx(&t.exhaust_tip), 2);
    put!(engine_idx(&t.engine_visuals), 2);
    put!(rim_style_idx(&t.rim_style), 2);
    put!(rim_color_idx(&t.rim_color), 2);
    put!(tire_idx(&t.tire), 2);
    put!(number_font_idx(&t.number_font), 2);
    put!(roof_acc_idx(&t.roof_accessory), 2);
    put!(side_mirror_idx(&t.side_mirror), 2);
    put!(tint_idx(&t.window_tint), 3);
    put!(trail_idx(&t.trail_effect), 2);
    put!(decal_idx(&t.decal), 3);

    acc
}

fn execute_mint_car(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    name: String,
    owner: Option<String>,
    token_uri: Option<String>,
) -> Result<Response, CarError> {
    // Enforce payment or allow pending free option
    let config = CONFIG.load(deps.storage)?;
    let sent = &info.funds;
    let has_free_opt = config.payment_options.iter().find(|c| c.denom == "free");
    let paid_ok = if config.payment_options.is_empty() { true } else {
        config.payment_options.iter().any(|Coin { denom, amount }| {
            if denom == "free" { return false; }
            sent.iter().any(|c| c.denom == *denom && c.amount >= *amount)
        })
    };
    if !paid_ok && has_free_opt.is_none() {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("insufficient payment: must include at least one accepted option or free option not configured")));
    }

    // If owner is not provided, use the sender
    let owner = owner.unwrap_or(info.sender.to_string());
    let owner_addr: Addr = deps.api.addr_validate(&owner)?;

    // Validate and ensure name existence + uniqueness
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("car name cannot be empty")));
    }
    if trimmed.chars().count() > MAX_NAME_SIZE {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("car name too long")));
    }
    let key = name_key(trimmed);
    if NAME_REGISTRY.has(deps.storage, key) {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("car name already exists")));
    }

    // Generate incremental token_id from CAR_ID_COUNTER
    let next_id = CAR_ID_COUNTER.load(deps.storage)?;
    let token_id = next_id.to_string();
    CAR_ID_COUNTER.save(deps.storage, &(next_id + Uint128::one()))?;

    // Populate car_id in metadata
    let mut extension = CarMetadata {
            name: trimmed.to_string(),
            image_data: None,
            attributes: None,
            car_id: Some(token_id.clone()),
        };

    // Reserve the name to prevent race conditions
    NAME_REGISTRY.save(deps.storage, key, &true)?;

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
    let base_seed = mix64(env.block.height as u64)
        ^ mix64(env.block.time.nanos())
        ^ hash_str(&token_id)
        ^ hash_str(&owner)
        ^ hash_str(info.sender.as_str());

    // Generate traits + rarity; ensure unique combo by re-rolling if necessary
    let table = default_rarity_table();
    let mut attempt: u32 = 0;
    let chosen_traits;
    let chosen_breakdown;
    let encoded_combo: u64;
    loop {
        if attempt > 64 {
            return Err(CarError::Std(cosmwasm_std::StdError::generic_err("failed to find unique trait combination after retries")));
        }
        let attempt_seed = mix64(base_seed ^ (attempt as u64));
        let (traits, breakdown) = generate_traits_with_rarity(attempt_seed, &table);
        let code = encode_traits_combo(&traits);
        if !USED_TRAIT_COMBOS.has(deps.storage, code) {
            chosen_traits = traits;
            chosen_breakdown = breakdown;
            encoded_combo = code;
            break;
        }
        attempt = attempt.wrapping_add(1);
    }

    // Persist used combination
    USED_TRAIT_COMBOS.save(deps.storage, encoded_combo, &true)?;

    // Generate and append metadata attributes
    let mut to_add = traits_to_attributes(&chosen_traits, &chosen_breakdown);

    let mut attrs = extension.attributes.take().unwrap_or_default();
    attrs.append(&mut to_add);
    extension.attributes = Some(attrs);

    // If not paid but free option exists: create pending free car state and return
    if !paid_ok {
        let free_minutes: u64 = has_free_opt.unwrap().amount.u128() as u64;
        let expires_at_nanos = env.block.time.nanos().saturating_add(free_minutes.saturating_mul(60).saturating_mul(1_000_000_000));

        let car_id_u128: u128 = token_id.parse().unwrap();
        // Snapshot minimal info for potential later use
        let cfg_snapshot = CONFIG.load(deps.storage)?;
        set_car_info(deps.storage, car_id_u128, crate::state::CarInfo {
            owners: vec![owner_addr.clone()],
            metadata: Some(extension.clone()),
            created_at: env.block.time.nanos(),
            current_energy: cfg_snapshot.max_energy,
            last_energy_update_nanos: env.block.time.nanos(),
        })?;

        PENDING_FREE_CARS.save(deps.storage, car_id_u128, &PendingFreeCar {
            reserved_for: owner_addr,
            expires_at_nanos,
            trait_code: encoded_combo,
        })?;

        return Ok(Response::new()
            .add_attribute("action", "create_pending_free_car")
            .add_attribute("token_id", token_id)
            .add_attribute("expires_at_nanos", expires_at_nanos.to_string()));
    }

    // Perform a self-call to cw721-base Mint
    let self_mint = ExecuteMsg::<Option<CarMetadata>, cosmwasm_std::Empty>::Mint(MintMsg {
        token_id: token_id.clone(),
        owner,
        token_uri,
        extension: Some(extension.clone()),
    });

    let msg = WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&self_mint)?,
        funds: vec![],
    };

    // Persist CarInfo for minted car with full energy
    let car_id_u128: u128 = token_id.parse().unwrap_or_default();
    let cfg_snapshot = CONFIG.load(deps.storage)?;
    set_car_info(deps.storage, car_id_u128, crate::state::CarInfo {
        owners: vec![owner_addr],
        metadata: Some(extension.clone()),
        created_at: env.block.time.nanos(),
        current_energy: cfg_snapshot.max_energy * 3,
        last_energy_update_nanos: env.block.time.nanos(),
    })?;

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "mint_car")
        .add_attribute("extension", format!("{:?}", extension))
    )
}

fn execute_pay_for_training(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    token_id: String,
) -> Result<Response, CarError> {
    // Anyone can pay for any car's training
    let car_id: u128 = token_id.parse().map_err(|_| CarError::Std(cosmwasm_std::StdError::generic_err("invalid token id")))?;
    let mut car = CAR_INFO.load(deps.storage, car_id)
        .map_err(|_| CarError::CarNotFound { car_id })?;
    let cfg = CONFIG.load(deps.storage)?;

    // Payment logic: if no training payment options are configured, refilling is free
    if !cfg.training_payment_options.is_empty() {
        let sent = &info.funds;
        let paid_ok = cfg.training_payment_options.iter().any(|Coin { denom, amount }| {
            sent.iter().any(|c| c.denom == *denom && c.amount >= *amount)
        });
        if !paid_ok {
            return Err(CarError::Std(cosmwasm_std::StdError::generic_err("training payment required")));
        }
    }

    // Refill energy to full
    car.current_energy = cfg.max_energy;
    car.last_energy_update_nanos = env.block.time.nanos();
    CAR_INFO.save(deps.storage, car_id, &car)?;

    Ok(Response::new()
        .add_attribute("action", "pay_for_training")
        .add_attribute("token_id", token_id))
}

fn ensure_energy_consumers_only(config: &Config, info: &MessageInfo) -> Result<(), CarError> {
    let sender_addr = info.sender.to_string();
    if let Some(ref consumers) = config.valid_energy_consumers {
        if consumers.contains(&sender_addr) {
            return Ok(());
        }
    }
    Err(CarError::Unauthorized {})
}

fn execute_consume_training_energy(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    token_id: String,
    sessions: u32,
) -> Result<Response, CarError> {
    // Load config first
    let cfg = CONFIG.load(deps.storage)?;
    
    // Only valid energy consumers may meter energy consumption during training
    ensure_energy_consumers_only(&cfg, &info)?;
    
    let car_id: u128 = token_id.parse().map_err(|_| CarError::Std(cosmwasm_std::StdError::generic_err("invalid token id")))?;
    let mut car = CAR_INFO.load(deps.storage, car_id)
        .map_err(|_| CarError::CarNotFound { car_id })?;

    // Recover before consuming
    car.recover_energy(env.block.time.nanos(), &cfg);

    // Compute required energy and check availability
    let required = (cfg.energy_per_training as u64)
        .saturating_mul(sessions as u64) as u32;
    if car.current_energy < required {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("insufficient energy for training")));
    }

    car.current_energy = car.current_energy.saturating_sub(required);
    // Update last update timestamp to now for consistency
    car.last_energy_update_nanos = env.block.time.nanos();
    CAR_INFO.save(deps.storage, car_id, &car)?;

    Ok(Response::new()
        .add_attribute("action", "consume_training_energy")
        .add_attribute("token_id", token_id)
        .add_attribute("sessions", sessions.to_string()))
}

fn execute_pay_to_finalize(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    token_id: String,
) -> Result<Response, CarError> {
    let car_id: u128 = token_id.parse().map_err(|_| CarError::Std(cosmwasm_std::StdError::generic_err("invalid token id")))?;
    let pending = PENDING_FREE_CARS.load(deps.storage, car_id)
        .map_err(|_| CarError::Std(cosmwasm_std::StdError::generic_err("not pending free car")))?;

    //Anyone can pay to finalize
    // if info.sender != pending.reserved_for {
    //     return Err(CarError::Unauthorized {});
    // }

    // Require payment matching a non-free option
    let config = CONFIG.load(deps.storage)?;
    let sent = &info.funds;
    let paid_ok = config.payment_options.iter().any(|Coin { denom, amount }| {
        if denom == "free" { return false; }
        sent.iter().any(|c| c.denom == *denom && c.amount >= *amount)
    });
    if !paid_ok {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("payment required to finalize")));
    }

    // Load metadata saved during pending creation
    let car_info = CAR_INFO.load(deps.storage, car_id)?;
    let extension = car_info.metadata;

    // Perform cw721 mint now
    let self_mint = ExecuteMsg::<Option<CarMetadata>, cosmwasm_std::Empty>::Mint(MintMsg {
        token_id: token_id.clone(),
        owner: pending.reserved_for.to_string(),
        token_uri: None,
        extension: extension.clone(),
    });
    let msg = WasmMsg::Execute { contract_addr: env.contract.address.to_string(), msg: to_json_binary(&self_mint)?, funds: vec![] };

    // Clear pending state
    PENDING_FREE_CARS.remove(deps.storage, car_id);

    // Initialize energy tracking snapshot as minted
    // If CAR_INFO existed from pending snapshot, update energy to full and timestamp
    if let Ok(mut info_snap) = CAR_INFO.load(deps.storage, car_id) {
        let cfg = CONFIG.load(deps.storage)?;
        info_snap.current_energy = cfg.max_energy;
        info_snap.last_energy_update_nanos = env.block.time.nanos();
        CAR_INFO.save(deps.storage, car_id, &info_snap)?;
    }

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "finalize_free_car")
        .add_attribute("token_id", token_id))
}

fn execute_expire_car(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    token_id: String,
) -> Result<Response, CarError> {
    let car_id: u128 = token_id.parse().map_err(|_| CarError::Std(cosmwasm_std::StdError::generic_err("invalid token id")))?;
    let pending = PENDING_FREE_CARS.load(deps.storage, car_id)
        .map_err(|_| CarError::Std(cosmwasm_std::StdError::generic_err("car not pending or already finalized")))?;
    if env.block.time.nanos() < pending.expires_at_nanos {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("car has not yet expired")));
    }

    // Free name registry if present
    if let Ok(info) = CAR_INFO.load(deps.storage, car_id) {
        if let Some(meta) = info.metadata {
            let old_trim = meta.name.trim().to_string();
            if !old_trim.is_empty() {
                let old_key = name_key(&old_trim);
                NAME_REGISTRY.remove(deps.storage, old_key);
            }
        }
    }

    // Free used trait combo and mapping
    USED_TRAIT_COMBOS.remove(deps.storage, pending.trait_code);

    // Remove pending and car info snapshot
    PENDING_FREE_CARS.remove(deps.storage, car_id);
    CAR_INFO.remove(deps.storage, car_id);

    // Purge race engine state if configured
    let config = CONFIG.load(deps.storage)?;
    let mut resp = Response::new().add_attribute("action", "expire_free_car").add_attribute("token_id", token_id);
    if let Some(addr) = config.race_engine_contract {
        if !addr.is_empty() {
            let purge = membrane::race_engine::ExecuteMsg::PurgeCar { car_id: Uint128::from(car_id) };
            let msg = WasmMsg::Execute { contract_addr: addr, msg: to_json_binary(&purge)?, funds: vec![] };
            resp = resp.add_message(msg);
        }
    }

    Ok(resp)
}

fn execute_update_custom_decal(
    deps: DepsMut,
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
    if let Some(attrs) = &mut ext.attributes {
        let mut found = false;
        for a in attrs.iter_mut() {
            if a.trait_type == "decal" {
                // Prevent editing preset decals
                if a.value.starts_with("Preset::") {
                    return Err(CarError::NotCustomDecal {});
                }
                a.value = svg.clone();
                found = true;
                break;
            }
        }
        if !found {
            attrs.push(membrane::types::CarAttribute { trait_type: "decal".to_string(), value: svg.clone() });
        }
    } else {
        ext.attributes = Some(vec![membrane::types::CarAttribute { trait_type: "decal".to_string(), value: svg.clone() }]);
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

fn execute_update_car_name(
    deps: DepsMut,
    info: MessageInfo,
    token_id: String,
    new_name: String,
) -> Result<Response, CarError> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("car name cannot be empty")));
    }
    if trimmed.chars().count() > MAX_NAME_SIZE {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("car name too long")));
    }

    // Only token owner may update
    let contract: CarCw721 = Cw721Contract::default();
    let token = contract.tokens.load(deps.storage, &token_id)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;
    if token.owner != info.sender {
        return Err(CarError::Unauthorized {});
    }

    // Load current metadata to read prior name
    let mut ext = token.extension.clone().unwrap_or(CarMetadata {
        name: String::new(),
        image_data: None,
        attributes: None,
        car_id: Some(token_id.clone()),
    });

    // If unchanged, no-op
    if ext.name.trim() == trimmed {
        return Ok(Response::new()
            .add_attribute("action", "update_car_name")
            .add_attribute("token_id", token_id)
            .add_attribute("name", trimmed));
    }

    // Ensure uniqueness via hashed key
    let new_key = name_key(trimmed);
    if NAME_REGISTRY.has(deps.storage, new_key) {
        return Err(CarError::Std(cosmwasm_std::StdError::generic_err("car name already exists")));
    }

    // Update registry: remove old, add new
    let old_name_trim = ext.name.trim().to_string();
    if !old_name_trim.is_empty() {
        let old_key = name_key(&old_name_trim);
        NAME_REGISTRY.remove(deps.storage, old_key);
    }
    NAME_REGISTRY.save(deps.storage, new_key, &true)?;

    // Persist new metadata
    ext.name = trimmed.to_string();
    let mut token_mut = token;
    token_mut.extension = Some(ext);
    contract.tokens.save(deps.storage, &token_id, &token_mut)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;

    Ok(Response::new()
        .add_attribute("action", "update_car_name")
        .add_attribute("token_id", token_id)
        .add_attribute("name", trimmed))
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        // Base CW721 queries
        QueryMsg::OwnerOf { token_id, include_expired } => {
            query_owner_of(deps, env, token_id, include_expired)
        }
        QueryMsg::Approval { token_id, spender, include_expired } => {
            query_approval(deps, env, token_id, spender, include_expired)
        }
        QueryMsg::Approvals { token_id, include_expired } => {
            query_approvals(deps, env, token_id, include_expired)
        }
        QueryMsg::AllOperators { owner, include_expired, start_after, limit } => {
            query_all_operators(deps, env, owner, include_expired, start_after, limit)
        }
        QueryMsg::NumTokens {} => {
            query_num_tokens(deps, env)
        }
        QueryMsg::ContractInfo {} => {
            query_contract_info(deps, env)
        }
        QueryMsg::NftInfo { token_id } => {
            query_nft_info(deps, env, token_id)
        }
        QueryMsg::AllNftInfo { token_id, include_expired } => {
            query_all_nft_info(deps, env, token_id, include_expired)
        }
        QueryMsg::Tokens { owner, start_after, limit } => {
            query_tokens(deps, env, owner, start_after, limit)
        }
        QueryMsg::AllTokens { start_after, limit } => {
            query_all_tokens(deps, env, start_after, limit)
        }
        QueryMsg::Minter {} => {
            query_minter(deps, env)
        }
        // Custom car queries
        QueryMsg::GetCarInfo { token_id } => {
            let id: u128 = token_id.parse().map_err(|_| cosmwasm_std::StdError::generic_err("invalid token id"))?;
            let mut car = CAR_INFO.load(deps.storage, id)
                .map_err(|_| cosmwasm_std::StdError::generic_err("car not found"))?;
            // Apply regen on the fly for query
            let cfg = CONFIG.load(deps.storage)?;
            car.recover_energy(env.block.time.nanos(), &cfg);
            let resp = membrane::car::CarInfoResponse {
                owners: car.owners.iter().map(|a| a.to_string()).collect(),
                metadata: car.metadata,
                created_at: car.created_at,
                current_energy: car.current_energy,
                last_energy_update_nanos: car.last_energy_update_nanos,
                max_energy: cfg.max_energy,
                energy_recovery_hours: cfg.energy_recovery_hours,
                energy_per_training: cfg.energy_per_training,
                training_payment_options: cfg.training_payment_options,
            };
            to_json_binary(&resp)
        }
    }
}

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, CarError> {
    //Load config
    let mut config = CONFIG.load(deps.storage)?;
    let mut consumers = vec![String::from("neutron1avcmg7e9urc7srxqd4ds8yfcnhdqk697mugqmhdc4q8njux6zazqgfguw4")];
    
    // Add race_engine_contract if it exists
    if let Some(race_engine) = config.race_engine_contract.clone() {
        consumers.push(race_engine);
    }
    
    config.valid_energy_consumers = Some(consumers);
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "migrate")
        // .add_attribute("cars_updated", cars.len().to_string())
        // .add_attribute("energy_set_to", "400")
    )
}
