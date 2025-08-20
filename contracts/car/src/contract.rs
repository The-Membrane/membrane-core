// car_nft/src/contract.rs

use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, Uint128, WasmMsg,
};
use cw2::set_contract_version;
use cw721_base::{Cw721Contract, ExecuteMsg as Cw721ExecuteMsg, InstantiateMsg as Cw721InstantiateMsg, MintMsg};

use crate::error::CarError;
use membrane::car::{ExecuteMsg, InstantiateMsg, QueryMsg, MigrateMsg, Config};
use crate::state::{CAR_ID_COUNTER, CONFIG, PENDING_OWNER};
use membrane::types::CarMetadata;
use membrane::traits_engine::{default_rarity_table, generate_traits_with_rarity, traits_to_attributes};
use crate::state::USED_TRAIT_COMBOS;
use membrane::traits_engine::{
    CarTraits,
    BaseColor, AccentPattern, PaintFinish, HeadlightColor, UnderglowColor, BrakeLightStyle,
    FrontBumperStyle, SpoilerType, RoofType, FenderStyle, ExhaustLength, ExhaustTipStyle,
    EngineVisuals, RimStyle, RimColor, TireType, NumberFont, RoofAccessory, SideMirror,
    WindowTint, TrailEffect, Decal, DecalPreset,
};

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

    // Initialize car ID counter to 1.
    //ID 0 is reserved for The Singularity..
    CAR_ID_COUNTER.save(deps.storage, &Uint128::one())?;

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

    // Mint the reserved token_id "0" named "The Singularity" to the contract owner
    let singularity_ext = Some(CarMetadata {
        name: "The Singularity".to_string(),
        image_data: None,
        attributes: None,
        car_id: Some("0".to_string()),
    });
    let self_mint = ExecuteMsg::Base(Cw721ExecuteMsg::Mint(MintMsg {
        token_id: "0".to_string(),
        owner: owner.to_string(),
        token_uri: None,
        extension: singularity_ext,
    }));
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
        ExecuteMsg::CreateCar { owner, token_uri, extension } => execute_mint_car(deps, env, info, owner, token_uri, extension),
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
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: Option<String>,
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

    // If owner is not provided, use the sender
    let owner = owner.unwrap_or(info.sender.to_string());

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

    if let Some(meta) = &mut extension {
        let mut attrs = meta.attributes.take().unwrap_or_default();
        attrs.append(&mut to_add);
        meta.attributes = Some(attrs);
    }

    // Perform a self-call to cw721-base Mint
    let self_mint = ExecuteMsg::Base(Cw721ExecuteMsg::Mint(MintMsg {
        token_id,
        owner,
        token_uri,
        extension: extension.clone(),
    }));

    let msg = WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&self_mint)?,
        funds: vec![],
    };

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "mint_car")
        .add_attribute("extension", format!("{:?}", extension))
    )
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
            attrs.push(membrane::types::CarAttribute { trait_type: "decal".to_string(), value: svg.clone() });
            has_decal_attr = true;
        }
    } else {
        ext.attributes = Some(vec![membrane::types::CarAttribute { trait_type: "decal".to_string(), value: svg.clone() }]);
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

#[entry_point]
pub fn migrate(deps: DepsMut, env: Env, msg: MigrateMsg) -> Result<Response, CarError> {
    Ok(Response::new())
}
