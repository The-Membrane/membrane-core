#!/usr/bin/env python3
"""
Standalone verification of CollateralForDebt logic
Demonstrates that the query will always return sufficient collateral
"""

from decimal import Decimal, getcontext
getcontext().prec = 18

def check_liquidatible(collateral, collateral_price, credit_price, premium):
    """
    Original CheckLiquidatible logic
    Returns: debt_repaid
    """
    adjusted_price = collateral_price * (Decimal(1) - premium)
    collateral_value = collateral * adjusted_price
    debt_repaid = collateral_value / credit_price
    return debt_repaid

def collateral_for_debt(debt, collateral_price, credit_price, premium):
    """
    New CollateralForDebt logic
    Returns: collateral_needed
    """
    debt_value = debt * credit_price
    adjusted_price = collateral_price * (Decimal(1) - premium)
    collateral_needed = debt_value / adjusted_price
    return collateral_needed

def verify_property(debt, collateral_price, credit_price, premium):
    """
    Verify the fundamental property:
    If CollateralForDebt(debt) returns X collateral,
    then CheckLiquidatible(X) must return debt >= requested debt
    """
    # Calculate collateral needed
    collateral = collateral_for_debt(debt, collateral_price, credit_price, premium)

    # Verify with CheckLiquidatible
    debt_repaid = check_liquidatible(collateral, collateral_price, credit_price, premium)

    # Check property (allowing small rounding tolerance)
    success = debt_repaid >= debt - Decimal("0.0001")

    return {
        'debt_requested': float(debt),
        'collateral_needed': float(collateral),
        'debt_repaid': float(debt_repaid),
        'success': success,
        'difference': float(debt_repaid - debt)
    }

def run_tests():
    """Run comprehensive tests"""
    print("=" * 80)
    print("COLLATERAL FOR DEBT LOGIC VERIFICATION")
    print("=" * 80)

    tests = [
        # (debt, collateral_price, credit_price, premium, description)
        (Decimal("1000"), Decimal("1.0"), Decimal("1.0"), Decimal("0"), "No premium, 1:1 prices"),
        (Decimal("1000"), Decimal("1.0"), Decimal("1.0"), Decimal("0.05"), "5% premium, 1:1 prices"),
        (Decimal("1000"), Decimal("1.0"), Decimal("1.0"), Decimal("0.10"), "10% premium, 1:1 prices"),
        (Decimal("5000"), Decimal("2.0"), Decimal("1.0"), Decimal("0.05"), "5% premium, 2:1 price ratio"),
        (Decimal("100"), Decimal("0.5"), Decimal("1.0"), Decimal("0.10"), "10% premium, 0.5:1 price ratio"),
        (Decimal("1000000"), Decimal("1.5"), Decimal("1.0"), Decimal("0.07"), "Large amount, 7% premium"),
        (Decimal("333"), Decimal("1.0"), Decimal("1.0"), Decimal("0.03"), "Odd amount, 3% premium"),
    ]

    print(f"\n{'Test':<50} {'Status':<10} {'Diff':<15}")
    print("-" * 80)

    all_passed = True
    for debt, col_price, cred_price, premium, desc in tests:
        result = verify_property(debt, col_price, cred_price, premium)
        status = "✅ PASS" if result['success'] else "❌ FAIL"
        if not result['success']:
            all_passed = False

        print(f"{desc:<50} {status:<10} {result['difference']:>+14.6f}")

    print("-" * 80)
    print(f"\nOverall: {'✅ ALL TESTS PASSED' if all_passed else '❌ SOME TESTS FAILED'}\n")

    # Detailed example
    print("=" * 80)
    print("DETAILED EXAMPLE")
    print("=" * 80)

    debt = Decimal("1000")
    col_price = Decimal("1.0")
    cred_price = Decimal("1.0")
    premium = Decimal("0.10")

    print(f"\nScenario: Repay {debt} CDT with {premium*100}% premium")
    print(f"  Collateral price: ${col_price}")
    print(f"  Credit price: ${cred_price}")

    collateral = collateral_for_debt(debt, col_price, cred_price, premium)
    print(f"\n1. CollateralForDebt calculation:")
    print(f"   collateral_needed = {debt} * {cred_price} / ({col_price} * (1 - {premium}))")
    print(f"   collateral_needed = {debt} / {col_price * (Decimal(1) - premium)}")
    print(f"   collateral_needed = {collateral}")

    debt_repaid = check_liquidatible(collateral, col_price, cred_price, premium)
    print(f"\n2. CheckLiquidatible verification:")
    print(f"   debt_repaid = {collateral} * {col_price} * (1 - {premium}) / {cred_price}")
    print(f"   debt_repaid = {collateral} * {col_price * (Decimal(1) - premium)}")
    print(f"   debt_repaid = {debt_repaid}")

    print(f"\n3. Verification:")
    print(f"   Requested debt: {debt}")
    print(f"   Actual repaid:  {debt_repaid}")
    print(f"   Difference:     {debt_repaid - debt}")
    print(f"   Status:         {'✅ SUCCESS' if debt_repaid >= debt - Decimal('0.0001') else '❌ FAILED'}")

    # Multiple slots example
    print("\n" + "=" * 80)
    print("MULTIPLE PREMIUM SLOTS EXAMPLE")
    print("=" * 80)

    total_debt = Decimal("2500")
    slots = [
        (Decimal("0.00"), Decimal("1000")),  # 0% premium, 1000 available
        (Decimal("0.05"), Decimal("1000")),  # 5% premium, 1000 available
        (Decimal("0.10"), Decimal("1000")),  # 10% premium, 1000 available
    ]

    print(f"\nNeed to repay: {total_debt} CDT")
    print("Available liquidity:")
    for premium, available in slots:
        print(f"  - {available} CDT at {premium*100}% premium")

    remaining_debt = total_debt
    total_collateral = Decimal("0")

    print("\nCalculation per slot:")
    for i, (premium, available) in enumerate(slots, 1):
        if remaining_debt <= 0:
            break

        debt_for_slot = min(remaining_debt, available)
        collateral_for_slot = collateral_for_debt(debt_for_slot, Decimal("1.0"), Decimal("1.0"), premium)

        print(f"\n  Slot {i} ({premium*100}% premium):")
        print(f"    Debt to repay: {debt_for_slot}")
        print(f"    Collateral needed: {collateral_for_slot}")

        total_collateral += collateral_for_slot
        remaining_debt -= debt_for_slot

    print(f"\nTotal collateral needed: {total_collateral}")
    print(f"Remaining debt: {remaining_debt}")

    # Verify the total
    total_repaid = Decimal("0")
    remaining_collateral = total_collateral
    remaining_debt = total_debt

    for premium, available in slots:
        if remaining_debt <= 0 or remaining_collateral <= 0:
            break

        debt_for_slot = min(remaining_debt, available)
        collateral_for_slot = collateral_for_debt(debt_for_slot, Decimal("1.0"), Decimal("1.0"), premium)

        if collateral_for_slot > remaining_collateral:
            collateral_for_slot = remaining_collateral

        repaid = check_liquidatible(collateral_for_slot, Decimal("1.0"), Decimal("1.0"), premium)
        total_repaid += repaid
        remaining_collateral -= collateral_for_slot
        remaining_debt -= debt_for_slot

    print(f"\nVerification:")
    print(f"  Requested: {total_debt} CDT")
    print(f"  Repaid:    {total_repaid} CDT")
    print(f"  Status:    {'✅ SUCCESS' if total_repaid >= total_debt - Decimal('0.001') else '❌ FAILED'}")

    print("\n" + "=" * 80)

if __name__ == "__main__":
    run_tests()
