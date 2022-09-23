use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{
    to_binary, Addr, CosmosMsg, StdResult, WasmMsg, Coin,
};

use membrane::positions::{ExecuteMsg};

/// CDPContract is a wrapper around Addr that provides a lot of helpers
/// for working with this.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
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

    // /// Get Count
    // pub fn count<Q, T, CQ>(&self, querier: &Q) -> StdResult<CountResponse>
    // where
    //     Q: Querier,
    //     T: Into<String>,
    //     CQ: CustomQuery,
    // {
    //     let msg = QueryMsg::GetCount {};
    //     let query = WasmQuery::Smart {
    //         contract_addr: self.addr().into(),
    //         msg: to_binary(&msg)?,
    //     }
    //     .into();
    //     let res: CountResponse = QuerierWrapper::<CQ>::new(querier).query(&query)?;
    //     Ok(res)
    // }
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
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

    // /// Get Count
    // pub fn count<Q, T, CQ>(&self, querier: &Q) -> StdResult<CountResponse>
    // where
    //     Q: Querier,
    //     T: Into<String>,
    //     CQ: CustomQuery,
    // {
    //     let msg = QueryMsg::GetCount {};
    //     let query = WasmQuery::Smart {
    //         contract_addr: self.addr().into(),
    //         msg: to_binary(&msg)?,
    //     }
    //     .into();
    //     let res: CountResponse = QuerierWrapper::<CQ>::new(querier).query(&query)?;
    //     Ok(res)
    // }
}