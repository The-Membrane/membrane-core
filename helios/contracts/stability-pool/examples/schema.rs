use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

use membrane::stability_pool::{ExecuteMsg, InstantiateMsg, QueryMsg, Cw20HookMsg, LiquidatibleResponse, DepositResponse, ClaimsResponse, PoolResponse, ConfigResponse};
use stability_pool::state::{Config, Propagation};

fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InstantiateMsg), &out_dir);
    export_schema(&schema_for!(ExecuteMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);
    export_schema(&schema_for!(Cw20HookMsg), &out_dir);
    export_schema(&schema_for!(Config), &out_dir);
    export_schema(&schema_for!(ConfigResponse), &out_dir);
    export_schema(&schema_for!(Propagation), &out_dir);
    export_schema(&schema_for!(LiquidatibleResponse), &out_dir);
    export_schema(&schema_for!(DepositResponse), &out_dir);
    export_schema(&schema_for!(ClaimsResponse), &out_dir);
    export_schema(&schema_for!(PoolResponse), &out_dir);
}
