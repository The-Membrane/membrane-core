use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

#[cw_serde]
pub enum ExecuteMsg {
    BasketLiquidate {
        offer_assets: apollo_cw_asset::AssetListUnchecked,
        receive_asset: apollo_cw_asset::AssetInfoUnchecked,
        minimum_receive: Option<Uint128>,
        to: Option<String>,
    }
}
