# Historical LTV Storage & Query Specification

## Overview
Add historical LTV tracking to the CDP contract to enable LTV charts showing historical data + pending LTV shifts with countdown timers.

## Required Contract Changes

### 1. State Storage

Add to `state.rs`:

```rust
/// Historical LTV snapshot for an asset
#[cw_serde]
pub struct LTVSnapshot {
    pub timestamp: u64,
    pub ltv: Decimal,
}

// Storage for historical LTV data per asset
// Key: (asset_denom, timestamp)
pub const LTV_HISTORY: Map<(&str, u64), LTVSnapshot> = Map::new("ltv_history");

// Note: LTV shift data is stored in existing LTV_UPDATE_TRACKERS
// pub const LTV_UPDATE_TRACKERS: Map<String, LTVUpdateTracker> = Map::new("ltv_update_trackers");
```

### 2. Update Logic

Modify existing position operations to record LTV snapshots:

```rust
// In positions.rs - Add after LTV calculations
fn record_ltv_snapshot(
    storage: &mut dyn Storage,
    env: &Env,
    asset_denom: &str,
    ltv: Decimal,
) -> StdResult<()> {
    let snapshot = LTVSnapshot {
        timestamp: env.block.time.seconds(),
        ltv,
    };

    // Store with (asset_denom, timestamp) as key for easy time-range queries
    LTV_HISTORY.save(storage, (asset_denom, env.block.time.seconds()), &snapshot)?;
    Ok(())
}
```

Call `record_ltv_snapshot` in:
- `deposit()` - After collateral is added
- `withdraw()` - After collateral is removed
- `increase_debt()` - After debt is increased
- `repay()` - After debt is repaid

### 3. Query Messages

Add to `msg.rs`:

```rust
#[cw_serde]
#[derive(QueryExt)]
pub enum QueryMsg {
    // ... existing queries ...

    /// Get historical LTV for a position
    GetHistoricalLTV {
        position_id: String,
        start_time: Option<u64>,  // Unix timestamp
        end_time: Option<u64>,    // Unix timestamp
        limit: Option<u32>,       // Max snapshots to return
    },

    /// Get current LTV shift info
    GetLTVShiftInfo {},
}

#[cw_serde]
pub struct HistoricalLTVResponse {
    pub position_id: String,
    pub snapshots: Vec<LTVSnapshot>,
}

#[cw_serde]
pub struct LTVShiftInfoResponse {
    pub current_shift_number: u64,
    pub next_shift_time: u64,  // Unix timestamp
    pub time_until_shift: u64, // Seconds until shift
}
```

### 4. Query Handlers

Add to `query.rs`:

```rust
pub fn query_historical_ltv(
    deps: Deps,
    position_id: String,
    start_time: Option<u64>,
    end_time: Option<u64>,
    limit: Option<u32>,
) -> StdResult<HistoricalLTVResponse> {
    let limit = limit.unwrap_or(100).min(500); // Max 500 snapshots
    let start = start_time.unwrap_or(0);
    let end = end_time.unwrap_or(u64::MAX);

    let snapshots: Vec<LTVSnapshot> = LTV_HISTORY
        .prefix(&position_id)
        .range(deps.storage, Some(Bound::inclusive(start)), Some(Bound::inclusive(end)), Order::Ascending)
        .take(limit as usize)
        .filter_map(|item| item.ok().map(|(_, snapshot)| snapshot))
        .collect();

    Ok(HistoricalLTVResponse {
        position_id,
        snapshots,
    })
}

pub fn query_ltv_shift_info(
    deps: Deps,
    env: Env,
) -> StdResult<LTVShiftInfoResponse> {
    let shift = LTV_SHIFT.load(deps.storage)?;
    let current_time = env.block.time.seconds();
    let time_until_shift = if shift.shift_time > current_time {
        shift.shift_time - current_time
    } else {
        0
    };

    Ok(LTVShiftInfoResponse {
        current_shift_number: shift.shift_number,
        next_shift_time: shift.shift_time,
        time_until_shift,
    })
}
```

## Frontend Integration

### Hook: `useHistoricalLTV.ts`

```typescript
import { useQuery } from '@tanstack/react-query'
import { useCDPClient } from '@/hooks/useCDP'

export const useHistoricalLTV = (
  positionId: string,
  startTime?: number,
  endTime?: number,
  limit?: number
) => {
  const { data: cdpClient } = useCDPClient(rpcUrl)

  return useQuery({
    queryKey: ['historical_ltv', positionId, startTime, endTime, limit],
    queryFn: async () => {
      if (!cdpClient) return null
      return cdpClient.getHistoricalLTV({
        positionId,
        startTime,
        endTime,
        limit,
      })
    },
    enabled: !!cdpClient && !!positionId,
  })
}

export const useLTVShiftInfo = () => {
  const { data: cdpClient } = useCDPClient(rpcUrl)

  return useQuery({
    queryKey: ['ltv_shift_info'],
    queryFn: async () => {
      if (!cdpClient) return null
      return cdpClient.getLTVShiftInfo()
    },
    enabled: !!cdpClient,
    refetchInterval: 1000, // Refresh every second for countdown
  })
}
```

## Chart Component Structure

The chart will display:
1. **Historical LTV Line** - Past LTV values from snapshots
2. **Pending LTV Line** - Projected LTV after pending shift (dashed line)
3. **Countdown Timer** - In-chart display of time until shift
4. **Max LTV Threshold** - Horizontal line showing liquidation threshold

Example data structure:
```typescript
{
  historical: [
    { timestamp: 1706745600, ltv: 45.2 },
    { timestamp: 1706832000, ltv: 47.8 },
    // ...
  ],
  pendingLTV: 52.3,
  shiftTime: 1707091200,
  timeUntilShift: 3600, // seconds
  maxLTV: 90.0
}
```

## Implementation Order

1. ✅ Frontend UI (expandable rows, chart placeholder) - DONE
2. 🔲 Add LTV storage to CDP contract
3. 🔲 Implement query endpoints
4. 🔲 Generate TypeScript types from contract
5. 🔲 Create frontend hooks
6. 🔲 Build LTV chart component
7. 🔲 Integrate chart into expandable row

## Notes

- Store snapshots periodically (not on every tiny change) to avoid storage bloat
- Consider implementing a cleanup mechanism for very old snapshots
- The "shift" concept may need clarification - is this related to rate index shifts or something else?
- Consider adding pagination for positions with many snapshots
