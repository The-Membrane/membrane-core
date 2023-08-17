use std::{fs::create_dir_all, env::current_dir};

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

use membrane::cdp::{InstantiateMsg, ExecuteMsg, QueryMsg, Config, PositionResponse, RedeemabilityResponse, BasketPositionsResponse, BadDebtResponse, InsolvencyResponse, InterestResponse, CollateralInterestResponse};
use membrane::types::{Basket, DebtCap};
fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InstantiateMsg), &out_dir);
    export_schema(&schema_for!(ExecuteMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);
    export_schema(&schema_for!(Config), &out_dir);
    export_schema(&schema_for!(PositionResponse), &out_dir);
    export_schema(&schema_for!(BasketPositionsResponse), &out_dir);
    export_schema(&schema_for!(BadDebtResponse), &out_dir);
    export_schema(&schema_for!(InsolvencyResponse), &out_dir);
    export_schema(&schema_for!(InterestResponse), &out_dir);
    export_schema(&schema_for!(CollateralInterestResponse), &out_dir);
    export_schema(&schema_for!(RedeemabilityResponse), &out_dir);
    export_schema(&schema_for!(Basket), &out_dir);
    export_schema(&schema_for!(DebtCap), &out_dir);
}