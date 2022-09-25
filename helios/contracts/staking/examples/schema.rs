use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

use membrane::staking::{
    ConfigResponse, Cw20HookMsg, ExecuteMsg, FeeEventsResponse, InstantiateMsg, QueryMsg,
    RewardsResponse, StakedResponse, StakerResponse, TotalStakedResponse,
};
use staking::state::Config;

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
    export_schema(&schema_for!(StakerResponse), &out_dir);
    export_schema(&schema_for!(RewardsResponse), &out_dir);
    export_schema(&schema_for!(StakedResponse), &out_dir);
    export_schema(&schema_for!(TotalStakedResponse), &out_dir);
    export_schema(&schema_for!(FeeEventsResponse), &out_dir);
}
