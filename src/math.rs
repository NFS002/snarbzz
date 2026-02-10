use alloy::primitives::{I256, U256};

/// Returns signed percentage change in basis points (1 bp = 0.01%)
/// Example: 1234 => +12.34%, -50 => -0.50%
pub fn percentage_change_bp(old: U256, new: U256) -> Option<I256> {
    if old.is_zero() {
        return None;
    }

    // Determine sign and absolute difference
    let (sign, diff) = if new >= old {
        (I256::ONE, new - old)
    } else {
        (I256::MINUS_ONE, old - new)
    };

    // Overflow-safe ordering:
    // (diff / old) * 10000 would lose precision,
    // so we instead do (diff * 10000) / old
    // but split the multiplication to reduce overflow risk.
    let scaled = diff
        .checked_mul(U256::from(10_000u64))?
        .checked_div(old)?;

    Some(I256::from_raw(scaled) * sign)
}

pub fn format_percent_bp(bp: I256) -> String {
    let sign = if bp.is_negative() { "-" } else { "" };
    let abs = bp.abs();

    let integer = abs / I256::unchecked_from(100);
    let fractional = abs % I256::unchecked_from(100);

    format!("{sign}{integer}.{fractional:02}%")
}

