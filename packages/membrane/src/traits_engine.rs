use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::CarAttribute;

/// ---------- Core Trait Types ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum PaintFinish { Solid, Metallic, Matte, Pearlescent }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum BaseColor {
    Black, White, Silver, Gray, Red, Blue, Green, Yellow, Orange, Purple,
    Teal, Gold, CustomHex(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum AccentPattern { None, Stripes, Flames, Camo, Geometric, Gradient }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum HeadlightColor { White, Blue, NeonGreen, Pink }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum UnderglowColor { None, White, Blue, Purple, Red, Green }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum BrakeLightStyle { ClassicRect, SlimStrip, Circular, SplitPanel }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum FrontBumperStyle { Standard, SportAggressive, OffroadReinforced, Retro }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum SpoilerType { None, SmallLip, Ducktail, LargeGtWing }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum RoofType { Hardtop, Convertible, Sunroof, Targa }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum FenderStyle { Stock, Widebody, RetroFlare, AeroCutout }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum ExhaustLength { Short, Mid, Long }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum ExhaustTipStyle { Round, Square, Angled, DualPipe }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum EngineVisuals { Covered, ChromePipes, VisibleIntercooler, PaintedValveCover }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum RimStyle { Classic5Spoke, FuturisticSolid, Mesh, DeepDish }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum RimColor { Chrome, Black, Gold, Custom }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum TireType { Slicks, SemiSlicks, OffroadTread, Whitewall }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum NumberFont { BoldBlock, ScriptItalic, RetroStencil, Digital }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum RoofAccessory { None, LightBar, Antenna, RoofRack }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum SideMirror { Standard, AeroSmall, RetroRound, WideRacing }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum WindowTint { None, Light, Medium, Dark, Colored }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum TrailEffect { None, Smoke, Sparks, NeonStreak }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum DecalPreset {
    FlamesA, FlamesB, CamoDesert, CamoUrban, GeometricLines, SponsorPackA, SponsorPackB
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum LayerMode { Overlay, Multiply, Screen }

// Removed DecalCustom; custom decals are now raw SVG strings

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(tag = "kind")]
pub enum Decal {
    Preset { preset: DecalPreset },
    /// Holds raw SVG content; empty string means entitlement to set later
    Custom(String),
}

/// ---------- Aggregated Traits for a Car ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct CarTraits {
    // Color & Paint
    pub base_color: BaseColor,
    pub accent_pattern: AccentPattern,
    pub paint_finish: PaintFinish,

    // Lighting
    pub headlight_color: HeadlightColor,
    pub underglow: UnderglowColor,
    pub brake_light_style: BrakeLightStyle,

    // Body & Shape
    pub front_bumper: FrontBumperStyle,
    pub spoiler: SpoilerType,
    pub roof: RoofType,
    pub fender: FenderStyle,

    // Exhaust & Engine
    pub exhaust_length: ExhaustLength,
    pub exhaust_tip: ExhaustTipStyle,
    pub engine_visuals: EngineVisuals,

    // Wheels
    pub rim_style: RimStyle,
    pub rim_color: RimColor,
    pub tire: TireType,

    // Misc Flair
    pub number_font: NumberFont,
    pub roof_accessory: RoofAccessory,
    pub side_mirror: SideMirror,
    pub window_tint: WindowTint,
    pub trail_effect: TrailEffect,

    // Decal
    pub decal: Decal,
}

/// ---------- Rarity Table Types ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Weighted<T> {
    pub item: T,
    pub weight: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct RarityTable {
    // Color & Paint
    pub base_color: Vec<Weighted<BaseColor>>,
    pub accent_pattern: Vec<Weighted<AccentPattern>>,
    pub paint_finish: Vec<Weighted<PaintFinish>>,

    // Lighting
    pub headlight_color: Vec<Weighted<HeadlightColor>>,
    pub underglow: Vec<Weighted<UnderglowColor>>,
    pub brake_light_style: Vec<Weighted<BrakeLightStyle>>,

    // Body & Shape
    pub front_bumper: Vec<Weighted<FrontBumperStyle>>,
    pub spoiler: Vec<Weighted<SpoilerType>>,
    pub roof: Vec<Weighted<RoofType>>,
    pub fender: Vec<Weighted<FenderStyle>>,

    // Exhaust & Engine
    pub exhaust_length: Vec<Weighted<ExhaustLength>>,
    pub exhaust_tip: Vec<Weighted<ExhaustTipStyle>>,
    pub engine_visuals: Vec<Weighted<EngineVisuals>>,

    // Wheels
    pub rim_style: Vec<Weighted<RimStyle>>,
    pub rim_color: Vec<Weighted<RimColor>>,
    pub tire: Vec<Weighted<TireType>>,

    // Misc Flair
    pub number_font: Vec<Weighted<NumberFont>>,
    pub roof_accessory: Vec<Weighted<RoofAccessory>>,
    pub side_mirror: Vec<Weighted<SideMirror>>,
    pub window_tint: Vec<Weighted<WindowTint>>,
    pub trail_effect: Vec<Weighted<TrailEffect>>,

    // Decal: randomizes presets; “Custom” is opt-in post-mint cosmetic
    pub decal_preset: Vec<Weighted<DecalPreset>>,
    /// optional: chance to mint a "custom decal slot" entitlement
    pub custom_decal_slot_weight: u32,
}

/// ---------- Example Rarity Table (weights are illustrative) ----------

pub fn default_rarity_table() -> RarityTable {
    use BaseColor::*;
    RarityTable {
        // Color & Paint
        base_color: vec![
            w(White, 1600), w(Black, 1600), w(Silver, 1200), w(Gray, 1200),
            w(Red, 900), w(Blue, 900), w(Green, 600), w(Orange, 400),
            w(Yellow, 300), w(Purple, 300), w(Teal, 300), w(Gold, 100),
        ],
        accent_pattern: vec![
            w(AccentPattern::None, 3500),
            w(AccentPattern::Stripes, 2200),
            w(AccentPattern::Flames, 1200),
            w(AccentPattern::Camo, 900),
            w(AccentPattern::Geometric, 900),
            w(AccentPattern::Gradient, 300),
        ],
        paint_finish: vec![
            w(PaintFinish::Solid, 5000),
            w(PaintFinish::Metallic, 3000),
            w(PaintFinish::Matte, 1500),
            w(PaintFinish::Pearlescent, 500),
        ],

        // Lighting
        headlight_color: vec![
            w(HeadlightColor::White, 6000),
            w(HeadlightColor::Blue, 2500),
            w(HeadlightColor::NeonGreen, 900),
            w(HeadlightColor::Pink, 600),
        ],
        underglow: vec![
            w(UnderglowColor::None, 6000),
            w(UnderglowColor::Blue, 1200),
            w(UnderglowColor::Purple, 900),
            w(UnderglowColor::Red, 900),
            w(UnderglowColor::Green, 700),
            w(UnderglowColor::White, 300),
        ],
        brake_light_style: vec![
            w(BrakeLightStyle::ClassicRect, 4000),
            w(BrakeLightStyle::SlimStrip, 3200),
            w(BrakeLightStyle::Circular, 1800),
            w(BrakeLightStyle::SplitPanel, 1000),
        ],

        // Body & Shape
        front_bumper: vec![
            w(FrontBumperStyle::Standard, 5000),
            w(FrontBumperStyle::SportAggressive, 2500),
            w(FrontBumperStyle::Retro, 1500),
            w(FrontBumperStyle::OffroadReinforced, 1000),
        ],
        spoiler: vec![
            w(SpoilerType::None, 4200),
            w(SpoilerType::SmallLip, 3200),
            w(SpoilerType::Ducktail, 1800),
            w(SpoilerType::LargeGtWing, 800),
        ],
        roof: vec![
            w(RoofType::Hardtop, 5200),
            w(RoofType::Sunroof, 2200),
            w(RoofType::Targa, 900),
            w(RoofType::Convertible, 700),
        ],
        fender: vec![
            w(FenderStyle::Stock, 5200),
            w(FenderStyle::Widebody, 2200),
            w(FenderStyle::RetroFlare, 1600),
            w(FenderStyle::AeroCutout, 1000),
        ],

        // Exhaust & Engine
        exhaust_length: vec![ w(ExhaustLength::Mid, 5000), w(ExhaustLength::Short, 3000), w(ExhaustLength::Long, 2000) ],
        exhaust_tip: vec![ w(ExhaustTipStyle::Round, 4500), w(ExhaustTipStyle::Square, 2000), w(ExhaustTipStyle::Angled, 2000), w(ExhaustTipStyle::DualPipe, 1500) ],
        engine_visuals: vec![
            w(EngineVisuals::Covered, 5200),
            w(EngineVisuals::PaintedValveCover, 2000),
            w(EngineVisuals::ChromePipes, 1600),
            w(EngineVisuals::VisibleIntercooler, 1200),
        ],

        // Wheels
        rim_style: vec![ w(RimStyle::Classic5Spoke, 4000), w(RimStyle::Mesh, 3000), w(RimStyle::DeepDish, 1800), w(RimStyle::FuturisticSolid, 1200) ],
        rim_color: vec![ w(RimColor::Black, 3800), w(RimColor::Chrome, 3400), w(RimColor::Gold, 1400), w(RimColor::Custom, 1400) ],
        tire: vec![ w(TireType::SemiSlicks, 4200), w(TireType::Slicks, 3200), w(TireType::OffroadTread, 1800), w(TireType::Whitewall, 800) ],

        // Misc Flair
        number_font: vec![ w(NumberFont::BoldBlock, 4200), w(NumberFont::Digital, 2400), w(NumberFont::RetroStencil, 2000), w(NumberFont::ScriptItalic, 1400) ],
        roof_accessory: vec![ w(RoofAccessory::None, 6000), w(RoofAccessory::Antenna, 1600), w(RoofAccessory::LightBar, 1200), w(RoofAccessory::RoofRack, 1200) ],
        side_mirror: vec![ w(SideMirror::Standard, 5200), w(SideMirror::AeroSmall, 2200), w(SideMirror::RetroRound, 1600), w(SideMirror::WideRacing, 1000) ],
        window_tint: vec![ w(WindowTint::None, 3000), w(WindowTint::Light, 2800), w(WindowTint::Medium, 2600), w(WindowTint::Dark, 1400), w(WindowTint::Colored, 200) ],
        trail_effect: vec![ w(TrailEffect::None, 6000), w(TrailEffect::Smoke, 1800), w(TrailEffect::Sparks, 1400), w(TrailEffect::NeonStreak, 800) ],

        // Decal
        decal_preset: vec![
            w(DecalPreset::FlamesA, 1500),
            w(DecalPreset::FlamesB, 1200),
            w(DecalPreset::CamoDesert, 1100),
            w(DecalPreset::CamoUrban, 1100),
            w(DecalPreset::GeometricLines, 900),
            w(DecalPreset::SponsorPackA, 800),
            w(DecalPreset::SponsorPackB, 800),
        ],
        custom_decal_slot_weight: 600,
    }
}

fn w<T>(item: T, weight: u32) -> Weighted<T> { Weighted { item, weight } }

/// ---------- Deterministic Picker Utilities ----------

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn pick_weighted<'a, T>(roll: u64, options: &'a [Weighted<T>]) -> &'a T {
    let total: u64 = options.iter().map(|w| w.weight as u64).sum();
    let mut r = roll % total.max(1);
    for w in options {
        if r < w.weight as u64 { return &w.item; }
        r -= w.weight as u64;
    }
    &options.last().unwrap().item
}

/// ---------- Construct Traits from a Seed + Table ----------

pub fn generate_traits(seed: u64, table: &RarityTable) -> CarTraits {
    let mut s = seed;

    macro_rules! next {
        () => {{
            s = splitmix64(s);
            s
        }};
    }

    // Color & Paint
    let base_color = pick_weighted(next!(), &table.base_color).clone();
    let accent_pattern = pick_weighted(next!(), &table.accent_pattern).clone();
    let paint_finish = pick_weighted(next!(), &table.paint_finish).clone();

    // Lighting
    let headlight_color = pick_weighted(next!(), &table.headlight_color).clone();
    let underglow = pick_weighted(next!(), &table.underglow).clone();
    let brake_light_style = pick_weighted(next!(), &table.brake_light_style).clone();

    // Body & Shape
    let front_bumper = pick_weighted(next!(), &table.front_bumper).clone();
    let spoiler = pick_weighted(next!(), &table.spoiler).clone();
    let roof = pick_weighted(next!(), &table.roof).clone();
    let fender = pick_weighted(next!(), &table.fender).clone();

    // Exhaust & Engine
    let exhaust_length = pick_weighted(next!(), &table.exhaust_length).clone();
    let exhaust_tip = pick_weighted(next!(), &table.exhaust_tip).clone();
    let engine_visuals = pick_weighted(next!(), &table.engine_visuals).clone();

    // Wheels
    let rim_style = pick_weighted(next!(), &table.rim_style).clone();
    let rim_color = pick_weighted(next!(), &table.rim_color).clone();
    let tire = pick_weighted(next!(), &table.tire).clone();

    // Misc Flair
    let number_font = pick_weighted(next!(), &table.number_font).clone();
    let roof_accessory = pick_weighted(next!(), &table.roof_accessory).clone();
    let side_mirror = pick_weighted(next!(), &table.side_mirror).clone();
    let window_tint = pick_weighted(next!(), &table.window_tint).clone();
    let trail_effect = pick_weighted(next!(), &table.trail_effect).clone();

    // Decal
    let sum_presets: u64 = table.decal_preset.iter().map(|w| w.weight as u64).sum();
    let total = sum_presets + table.custom_decal_slot_weight as u64;
    let r = next!() % total.max(1);
    let decal = if r < sum_presets {
        let preset = pick_weighted(r, &table.decal_preset).clone();
        Decal::Preset { preset }
    } else {
        // Entitlement: default empty SVG, can be updated off-chain or via later feature
        Decal::Custom(String::new())
    };

    CarTraits {
        base_color,
        accent_pattern,
        paint_finish,
        headlight_color,
        underglow,
        brake_light_style,
        front_bumper,
        spoiler,
        roof,
        fender,
        exhaust_length,
        exhaust_tip,
        engine_visuals,
        rim_style,
        rim_color,
        tire,
        number_font,
        roof_accessory,
        side_mirror,
        window_tint,
        trail_effect,
        decal,
    }
}

use std::collections::BTreeMap;

// ==== Rarity descriptors ====

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub enum RarityBand { Common, Uncommon, Rare, Epic, Legendary }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RarityStat<T> {
    pub value: T,
    pub probability: f64,
    pub band: RarityBand,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RarityBreakdown {
    // Use string summaries to remain no_std friendly
    pub per_trait: BTreeMap<String, String>,
    pub overall_probability: f64,
    pub overall_band: RarityBand,
}

fn band_for(p: f64) -> RarityBand {
    if p <= 0.005 { RarityBand::Legendary }
    else if p <= 0.02 { RarityBand::Epic }
    else if p <= 0.08 { RarityBand::Rare }
    else if p <= 0.20 { RarityBand::Uncommon }
    else { RarityBand::Common }
}

fn overall_band_for(p: f64) -> RarityBand {
    if p <= 1e-9 { RarityBand::Legendary }
    else if p <= 5e-9 { RarityBand::Epic }
    else if p <= 2e-8 { RarityBand::Rare }
    else if p <= 1e-7 { RarityBand::Uncommon }
    else { RarityBand::Common }
}

fn prob_of<T: Clone + Serialize>(chosen: &T, options: &[Weighted<T>]) -> f64
where
    T: PartialEq,
{
    let total: f64 = options.iter().map(|w| w.weight as f64).sum();
    if total <= 0.0 { return 0.0; }
    let w = options.iter().find(|w| &w.item == chosen).map(|w| w.weight as f64).unwrap_or(0.0);
    w / total
}

fn insert_stat<T: Clone + Serialize + core::fmt::Debug>(
    map: &mut BTreeMap<String, String>,
    key: &str,
    value: &T,
    prob: f64,
) {
    let band = band_for(prob);
    let summary = format!("value={:?}, p={:.8}, band={:?}", value, prob, band);
    map.insert(key.to_string(), summary);
}

/// ---- Main generator with rarity ----

pub fn generate_traits_with_rarity(seed: u64, table: &RarityTable) -> (CarTraits, RarityBreakdown) {
    let traits = generate_traits(seed, table);

    let mut per: BTreeMap<String, String> = BTreeMap::new();
    let mut overall: f64 = 1.0;

    // Color & Paint
    {
        let p = prob_of(&traits.base_color, &table.base_color);
        insert_stat(&mut per, "base_color", &traits.base_color, p); overall *= p;

        let p = prob_of(&traits.accent_pattern, &table.accent_pattern);
        insert_stat(&mut per, "accent_pattern", &traits.accent_pattern, p); overall *= p;

        let p = prob_of(&traits.paint_finish, &table.paint_finish);
        insert_stat(&mut per, "paint_finish", &traits.paint_finish, p); overall *= p;
    }

    // Lighting
    {
        let p = prob_of(&traits.headlight_color, &table.headlight_color);
        insert_stat(&mut per, "headlight_color", &traits.headlight_color, p); overall *= p;

        let p = prob_of(&traits.underglow, &table.underglow);
        insert_stat(&mut per, "underglow", &traits.underglow, p); overall *= p;

        let p = prob_of(&traits.brake_light_style, &table.brake_light_style);
        insert_stat(&mut per, "brake_light_style", &traits.brake_light_style, p); overall *= p;
    }

    // Body & Shape
    {
        let p = prob_of(&traits.front_bumper, &table.front_bumper);
        insert_stat(&mut per, "front_bumper", &traits.front_bumper, p); overall *= p;

        let p = prob_of(&traits.spoiler, &table.spoiler);
        insert_stat(&mut per, "spoiler", &traits.spoiler, p); overall *= p;

        let p = prob_of(&traits.roof, &table.roof);
        insert_stat(&mut per, "roof", &traits.roof, p); overall *= p;

        let p = prob_of(&traits.fender, &table.fender);
        insert_stat(&mut per, "fender", &traits.fender, p); overall *= p;
    }

    // Exhaust & Engine
    {
        let p = prob_of(&traits.exhaust_length, &table.exhaust_length);
        insert_stat(&mut per, "exhaust_length", &traits.exhaust_length, p); overall *= p;

        let p = prob_of(&traits.exhaust_tip, &table.exhaust_tip);
        insert_stat(&mut per, "exhaust_tip", &traits.exhaust_tip, p); overall *= p;

        let p = prob_of(&traits.engine_visuals, &table.engine_visuals);
        insert_stat(&mut per, "engine_visuals", &traits.engine_visuals, p); overall *= p;
    }

    // Wheels
    {
        let p = prob_of(&traits.rim_style, &table.rim_style);
        insert_stat(&mut per, "rim_style", &traits.rim_style, p); overall *= p;

        let p = prob_of(&traits.rim_color, &table.rim_color);
        insert_stat(&mut per, "rim_color", &traits.rim_color, p); overall *= p;

        let p = prob_of(&traits.tire, &table.tire);
        insert_stat(&mut per, "tire", &traits.tire, p); overall *= p;
    }

    // Misc Flair
    {
        let p = prob_of(&traits.number_font, &table.number_font);
        insert_stat(&mut per, "number_font", &traits.number_font, p); overall *= p;

        let p = prob_of(&traits.roof_accessory, &table.roof_accessory);
        insert_stat(&mut per, "roof_accessory", &traits.roof_accessory, p); overall *= p;

        let p = prob_of(&traits.side_mirror, &table.side_mirror);
        insert_stat(&mut per, "side_mirror", &traits.side_mirror, p); overall *= p;

        let p = prob_of(&traits.window_tint, &table.window_tint);
        insert_stat(&mut per, "window_tint", &traits.window_tint, p); overall *= p;

        let p = prob_of(&traits.trail_effect, &table.trail_effect);
        insert_stat(&mut per, "trail_effect", &traits.trail_effect, p); overall *= p;
    }

    // Decal (special case)
    {
        let sum_presets: f64 = table.decal_preset.iter().map(|w| w.weight as f64).sum();
        let total = sum_presets + table.custom_decal_slot_weight as f64;

        let p = match &traits.decal {
            Decal::Preset { preset } => {
                let w = table.decal_preset.iter().find(|w| &w.item == preset).map(|w| w.weight as f64).unwrap_or(0.0);
                if total > 0.0 { w / total } else { 0.0 }
            }
            Decal::Custom(svg) => {
                if total > 0.0 { (table.custom_decal_slot_weight as f64) / total } else { 0.0 }
            }
        };

        let summary = match &traits.decal {
            Decal::Preset { preset } => format!("value=Preset::{:?}, p={:.8}, band={:?}", preset, p, band_for(p)),
            Decal::Custom(svg) => {
                let label = if svg.is_empty() { "CustomSVG(Empty)" } else { "CustomSVG(Set)" };
                format!("value={}, p={:.8}, band={:?}", label, p, band_for(p))
            }
        };
        per.insert("decal".to_string(), summary);
        overall *= p;
    }

    let breakdown = RarityBreakdown {
        per_trait: per,
        overall_probability: overall,
        overall_band: overall_band_for(overall),
    };

    (traits, breakdown)
}

/// Convert `CarTraits` + rarity to a flat list of metadata attributes.
pub fn traits_to_attributes(traits: &CarTraits, breakdown: &RarityBreakdown) -> Vec<CarAttribute> {
    let mut attrs: Vec<CarAttribute> = Vec::new();

    // helper
    let mut push_attr = |trait_type: &str, value: String| {
        attrs.push(CarAttribute { trait_type: trait_type.to_string(), value });
    };

    // Color & Paint
    push_attr("base_color", format!("{:?}", traits.base_color));
    push_attr("accent_pattern", format!("{:?}", traits.accent_pattern));
    push_attr("paint_finish", format!("{:?}", traits.paint_finish));

    // Lighting
    push_attr("headlight_color", format!("{:?}", traits.headlight_color));
    push_attr("underglow", format!("{:?}", traits.underglow));
    push_attr("brake_light_style", format!("{:?}", traits.brake_light_style));

    // Body & Shape
    push_attr("front_bumper", format!("{:?}", traits.front_bumper));
    push_attr("spoiler", format!("{:?}", traits.spoiler));
    push_attr("roof", format!("{:?}", traits.roof));
    push_attr("fender", format!("{:?}", traits.fender));

    // Exhaust & Engine
    push_attr("exhaust_length", format!("{:?}", traits.exhaust_length));
    push_attr("exhaust_tip", format!("{:?}", traits.exhaust_tip));
    push_attr("engine_visuals", format!("{:?}", traits.engine_visuals));

    // Wheels
    push_attr("rim_style", format!("{:?}", traits.rim_style));
    push_attr("rim_color", format!("{:?}", traits.rim_color));
    push_attr("tire", format!("{:?}", traits.tire));

    // Misc Flair
    push_attr("number_font", format!("{:?}", traits.number_font));
    push_attr("roof_accessory", format!("{:?}", traits.roof_accessory));
    push_attr("side_mirror", format!("{:?}", traits.side_mirror));
    push_attr("window_tint", format!("{:?}", traits.window_tint));
    push_attr("trail_effect", format!("{:?}", traits.trail_effect));

    // Decal
    match &traits.decal {
        Decal::Preset { preset } => push_attr("decal", format!("Preset::{:?}", preset)),
        Decal::Custom(svg) => {
            // Store the raw SVG string (may be empty if entitlement only)
            push_attr("decal", svg.clone());
        }
    }

    // Single trait that shows the rarity of the car combination
    push_attr("rarity", format!("{:?}", breakdown.overall_band));

    attrs
} 

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_traits_and_rarity_attribute() {
        let table = default_rarity_table();
        let seed = 123456789u64;
        let (traits, breakdown) = generate_traits_with_rarity(seed, &table);
        let attrs = traits_to_attributes(&traits, &breakdown);
        assert!(attrs.iter().any(|a| a.trait_type == "rarity"));
        // 22 feature traits + 1 decal + 1 rarity = 23
        assert!(attrs.len() >= 23);
    }

    #[test]
    fn deterministic_for_same_seed() {
        let table = default_rarity_table();
        let seed = 42u64;
        let (t1, b1) = generate_traits_with_rarity(seed, &table);
        let (t2, b2) = generate_traits_with_rarity(seed, &table);
        assert_eq!(t1, t2);
        assert_eq!(b1.overall_band, b2.overall_band);
        assert!((b1.overall_probability - b2.overall_probability).abs() < 1e-12);
    }
} 