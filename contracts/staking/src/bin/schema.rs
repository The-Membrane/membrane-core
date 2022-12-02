use cosmwasm_schema::write_api;

use membrane::staking::{InstantiateMsg, ExecuteMsg, QueryMsg};
fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExecuteMsg,
        query: QueryMsg,
    }
}