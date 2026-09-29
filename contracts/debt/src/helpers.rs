use cosmwasm_std::{to_json_binary, Addr, Coin, CosmosMsg, StdResult, WasmMsg};
use cosmwasm_schema::cw_serde;

use membrane::debt::ExecuteMsg;

/// DebtContract is a wrapper around Addr that provides helpers
/// for constructing execute messages to the Debt contract.
///
/// Used by Collateral and Liquidation contracts to build SubMsgs.
///
/// CDP parity: maps to cdp/helpers.rs::CDPContract (debt portion)
#[cw_serde]
pub struct DebtContract(pub Addr);

impl DebtContract {
    pub fn addr(&self) -> Addr {
        self.0.clone()
    }

    pub fn call<T: Into<ExecuteMsg>>(&self, msg: T, funds: Vec<Coin>) -> StdResult<CosmosMsg> {
        let msg = to_json_binary(&msg.into())?;
        Ok(WasmMsg::Execute {
            contract_addr: self.addr().into(),
            msg,
            funds,
        }
        .into())
    }
}
