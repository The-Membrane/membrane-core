use cosmwasm_std::{to_json_binary, Addr, Coin, CosmosMsg, StdResult, WasmMsg};
use cosmwasm_schema::cw_serde;

use membrane::liquidation_engine::ExecuteMsg;

/// LiquidationContract is a wrapper around Addr that provides helpers
/// for constructing execute messages to the Liquidation Engine contract.
///
/// CDP parity: new in modular design
#[cw_serde]
pub struct LiquidationContract(pub Addr);

impl LiquidationContract {
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
