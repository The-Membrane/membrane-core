use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

use membrane::positions::{
    Config, BadDebtResponse, CallbackMsg,
    ExecuteMsg, InsolvencyResponse, InstantiateMsg, PositionResponse, PositionsResponse,
    QueryMsg, InterestResponse, CollateralInterestResponse
};
use membrane::types::Basket;

fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InstantiateMsg), &out_dir);
    export_schema(&schema_for!(ExecuteMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);
    export_schema(&schema_for!(CallbackMsg), &out_dir);
    export_schema(&schema_for!(Config), &out_dir);
    export_schema(&schema_for!(PositionsResponse), &out_dir);
    export_schema(&schema_for!(PositionResponse), &out_dir);
    export_schema(&schema_for!(Basket), &out_dir);
    export_schema(&schema_for!(BadDebtResponse), &out_dir);
    export_schema(&schema_for!(InsolvencyResponse), &out_dir);
    export_schema(&schema_for!(InterestResponse), &out_dir);
    export_schema(&schema_for!(CollateralInterestResponse), &out_dir);
    
}
