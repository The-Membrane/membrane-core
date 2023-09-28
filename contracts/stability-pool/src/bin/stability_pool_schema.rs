use std::{fs::create_dir_all, env::current_dir};

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

use membrane::stability_pool::{InstantiateMsg, ExecuteMsg, QueryMsg, Config, LiquidatibleResponse, ClaimsResponse, DepositPositionResponse};
use membrane::types::{AssetPool, Deposit};
fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InstantiateMsg), &out_dir);
    export_schema(&schema_for!(ExecuteMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);
    export_schema(&schema_for!(Config), &out_dir);
    export_schema(&schema_for!(LiquidatibleResponse), &out_dir);
    export_schema(&schema_for!(ClaimsResponse), &out_dir);
    export_schema(&schema_for!(DepositPositionResponse), &out_dir);
    export_schema(&schema_for!(Deposit), &out_dir);
    export_schema(&schema_for!(AssetPool), &out_dir); 
}
