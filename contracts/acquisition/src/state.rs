use cosmwasm_std::{StdResult, Storage};
use cw_storage_plus::{Item, Map};

use membrane::acquisition::{
    Config, AcquisitionWindow, AcquisitionDeposit, MbrnIntentOption,
    AcquisitionModelConfig, AcquisitionModelState, AcquisitionHistoryEntry,
};

pub const CONFIG: Item<Config> = Item::new("config");
pub const CURRENT_WINDOW_ID: Item<u64> = Item::new("current_window_id");
pub const CURRENT_ACQUISITION_WINDOW: Item<AcquisitionWindow> = Item::new("current_acquisition_window");
pub const USER_ACQUISITION_DEPOSITS: Map<(String, u64), AcquisitionDeposit> = Map::new("user_acquisition_deposits");
pub const USER_INTENTS: Map<String, Vec<MbrnIntentOption>> = Map::new("user_intents");
pub const ACQUISITION_CONFIG: Item<AcquisitionModelConfig> = Item::new("acq_config");
pub const ACQUISITION_STATE: Item<AcquisitionModelState> = Item::new("acq_state");

// ================= Acquisition History =================
pub const ACQUISITION_HISTORY: Item<Vec<AcquisitionHistoryEntry>> = Item::new("acq_history");
const ACQ_HISTORY_CAP: usize = 365;

/// Append a snapshot to the acquisition history, trimming oldest entries if over cap.
pub fn append_acquisition_history(
    store: &mut dyn Storage,
    entry: AcquisitionHistoryEntry,
) -> StdResult<()> {
    let mut history = ACQUISITION_HISTORY.may_load(store)?.unwrap_or_default();
    history.push(entry);
    if history.len() > ACQ_HISTORY_CAP {
        let remove = history.len() - ACQ_HISTORY_CAP;
        history.drain(0..remove);
    }
    ACQUISITION_HISTORY.save(store, &history)
}

