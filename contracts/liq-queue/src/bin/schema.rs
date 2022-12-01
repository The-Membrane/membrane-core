use cosmwasm_schema::write_api;

use membrane::liq_queue::{InstantiateMsg, ExecuteMsg, QueryMsg};
fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExecuteMsg,
        query: QueryMsg,
    }
}