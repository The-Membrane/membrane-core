use cosmwasm_std::{to_binary, Addr, Coin, CosmosMsg, StdResult, WasmMsg};
use cosmwasm_schema::cw_serde;

use membrane::cdp::ExecuteMsg;

/// CDPContract is a wrapper around Addr that provides a lot of helpers
/// for working with this.

#[cw_serde]
pub struct CDPContract(pub Addr);

impl CDPContract {
    pub fn addr(&self) -> Addr {
        self.0.clone()
    }

    pub fn call<T: Into<ExecuteMsg>>(&self, msg: T, funds: Vec<Coin>) -> StdResult<CosmosMsg> {
        let msg = to_binary(&msg.into())?;
        Ok(WasmMsg::Execute {
            contract_addr: self.addr().into(),
            msg,
            funds,
        }
        .into())
    }

}

#[cw_serde]
pub struct LQContract(pub Addr);

impl LQContract {
    pub fn addr(&self) -> Addr {
        self.0.clone()
    }

    pub fn call<T: Into<ExecuteMsg>>(&self, msg: T) -> StdResult<CosmosMsg> {
        let msg = to_binary(&msg.into())?;
        Ok(WasmMsg::Execute {
            contract_addr: self.addr().into(),
            msg,
            funds: vec![],
        }
        .into())
    }
}


