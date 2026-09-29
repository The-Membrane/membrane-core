# Tournament Sunday Scheduling Implementation

## Summary
I've successfully implemented automatic tournament scheduling to run every Sunday. Here's what was added:

## New Features Added

### 1. New Execute Messages
- `EnableWeeklyTournaments` - Admin can configure weekly tournaments
- `DisableWeeklyTournaments` - Admin can disable weekly tournaments  
- `CheckAndStartScheduledTournament` - Checks if it's Sunday and starts tournament if conditions are met

### 2. New Query Message
- `GetScheduledTournament` - Query the current scheduled tournament configuration

### 3. New Data Structures
- `ScheduledTournament` - Stores weekly tournament configuration
- `GetScheduledTournamentResponse` - Response for scheduled tournament queries

### 4. New Storage
- `SCHEDULED_TOURNAMENT` - Stores the scheduled tournament configuration

## How It Works

### Sunday Detection Algorithm
The system calculates the current Sunday start time (00:00:00 UTC) using:
- Unix timestamp calculation
- Day of week calculation (January 1, 1970 was a Thursday)
- Days since epoch modulo 7 to find current day
- Subtraction to get to Sunday start

### Tournament Scheduling Logic
1. **Enable**: Admin sets up weekly tournament with criteria, track, participants, etc.
2. **Check**: Anyone can call `CheckAndStartScheduledTournament` to trigger the check
3. **Start**: If it's Sunday, no tournament running, and enough participants → tournament starts
4. **Track**: System tracks last Sunday start to prevent duplicate tournaments

### Key Features
- **Admin Control**: Only admin can enable/disable weekly tournaments
- **Sunday Only**: Tournaments only start on Sundays (00:00:00 UTC)
- **No Duplicates**: Prevents multiple tournaments on the same Sunday
- **Participant Check**: Requires minimum participants before starting
- **Automatic Cleanup**: Clears pre-registrations after tournament starts

## Usage Examples

### Enable Weekly Tournaments
```rust
ExecuteMsg::EnableWeeklyTournaments {
    criteria: TournamentCriteria::Random,
    track_id: "track_1".to_string(),
    max_participants: Some(16),
    allow_free_registration: true,
    registration_payment_options: vec![],
    max_ticks: 1000,
}
```

### Check and Start (can be called by anyone)
```rust
ExecuteMsg::CheckAndStartScheduledTournament {}
```

### Query Scheduled Tournament
```rust
QueryMsg::GetScheduledTournament {}
```

## External Trigger Options
Since blockchain contracts can't automatically trigger themselves, you'll need an external service to call `CheckAndStartScheduledTournament` regularly. Options include:

1. **Cron Job**: Server calls the contract every hour
2. **Blockchain Validator**: Validator includes the call in blocks
3. **Bot Service**: Automated service monitors and triggers
4. **Manual**: Users can trigger the check manually

The system is now ready to run tournaments every Sunday automatically!
