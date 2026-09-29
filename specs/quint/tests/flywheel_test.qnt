// specs/quint/tests/flywheel_test.qnt
//
// End-to-end flywheel scenarios. Exercises the cross-organ flow:
//   CDP interest → RevenueDistributor → Disco/Staking →
//     Points System awards → System Discounts query → CDP fee discount
//
// Run:
//   quint run --max-steps=300 --max-samples=10000 \
//     --invariant=safety specs/quint/tests/flywheel_test.qnt
module flywheel_test {
  import types.*               from "../lib/types"
  import money.*               from "../lib/money"
  import flywheel.*            from "../compositions/flywheel"

  // ----------------------------------------------------------------
  // Scenario 1: CDP-mint user repays during volatility → 5 pts
  //
  //   Position enters volatile window (vol_window_initial_debt set)
  //   User repays during the window → CDP::CheckAndClearDebtDelta returns
  //     qualifies_for_points = true
  //   PointsSystem::CDPGivesUserManagementPoints awards exactly
  //     MANAGEMENT_POINTS_PER_ACTION = 5_000_000
  // ----------------------------------------------------------------
  // (Implementer wires concrete CDP setup + reply chain.)

  // ----------------------------------------------------------------
  // Scenario 2: Disco affiliate fee → Points System
  //
  //   Disco claim path with affiliate set: revenue claim succeeds,
  //   affiliate_fee deducted, GivePointsForAffiliateFee called on PS.
  //   Witnesses MGR.ev_ManagerFeeEarned event.
  // ----------------------------------------------------------------

  // ----------------------------------------------------------------
  // Scenario 3: Discount priority stack — timed > static > linear
  //
  //   Set timed-discount window: 40% for next 12 hours
  //   User has static discount: 25%
  //   User MBRN balance grants linear: 10%
  //   At t=0: timed wins → 40%
  //   At t=12h+1s: static wins → 25%
  //   After ClearDiscountPeriod: linear → 10%
  // ----------------------------------------------------------------

  // ----------------------------------------------------------------
  // Scenario 4: Fee-decrease-instant path
  //
  //   Manager has 5% fee. Calls RequestManagerFeeChange(3%).
  //   Decrease applies INSTANTLY (no pending record).
  //   Same manager calls RequestManagerFeeChange(7%).
  //   Increase creates pending record; ExecuteManagerFeeChange
  //   succeeds only after 14 days within 2-day window.
  // ----------------------------------------------------------------

  // ----------------------------------------------------------------
  // Scenario 5: Acquisition → Disco → claim flow
  //
  //   User deposits USDC via Acquisition.Deposit
  //   Window expires; AcquisitionRewardsToDisco fires
  //   Disco binding: deposit_owner = acquisition, manager = user,
  //                  revenue_destination = user
  //   User earns CDT yield during cliff (revenue_destination)
  //   Cliff passes → Claim transfers ownership to user
  // ----------------------------------------------------------------

  // ----------------------------------------------------------------
  // Scenario 6: Bad-debt flow ends with ClearBadDebtFreeze
  //
  //   CDP::FulfillBadDebt (insufficient collateral)
  //   → Auction::StartMBRNSale called by Disco
  //   → Buyer sends CDT via BuySuppliedMBRN
  //   → Auction emits Disco::SendMBRNForSale + Debt::FulfillBadDebt
  //   → On full asset closure: Auction emits Disco::ClearBadDebtFreeze
  //   Disco's PENDING_BAD_DEBT[asset] returns to 0; unstaking unfrozen.
  // ----------------------------------------------------------------
}
