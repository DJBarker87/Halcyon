use crate::autocall_hedged::HedgedAutocallError;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolSwapCostConfig {
    pub base_fee_bps: f64,
    pub slippage_coeff: f64,
    pub liquidity_proxy: f64,
    pub stress_multiplier: f64,
    pub keeper_bounty_usdc: f64,
}

impl Default for SolSwapCostConfig {
    fn default() -> Self {
        Self {
            base_fee_bps: 10.0,
            slippage_coeff: 25.0,
            liquidity_proxy: 250_000.0,
            stress_multiplier: 1.0,
            keeper_bounty_usdc: 0.10,
        }
    }
}

impl SolSwapCostConfig {
    pub fn validate(&self) -> Result<(), HedgedAutocallError> {
        if !(self.base_fee_bps.is_finite() && self.base_fee_bps >= 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "base_fee_bps must be finite and non-negative",
            ));
        }
        if !(self.slippage_coeff.is_finite() && self.slippage_coeff >= 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "slippage_coeff must be finite and non-negative",
            ));
        }
        if !(self.liquidity_proxy.is_finite() && self.liquidity_proxy > 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "liquidity_proxy must be positive",
            ));
        }
        if !(self.stress_multiplier.is_finite() && self.stress_multiplier > 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "stress_multiplier must be positive",
            ));
        }
        if !(self.keeper_bounty_usdc.is_finite() && self.keeper_bounty_usdc >= 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "keeper_bounty_usdc must be finite and non-negative",
            ));
        }
        Ok(())
    }

    pub fn cost_bps(&self, trade_notional_abs: f64) -> f64 {
        if trade_notional_abs <= 0.0 {
            return 0.0;
        }
        let slippage = self.slippage_coeff * (trade_notional_abs / self.liquidity_proxy).sqrt();
        (self.base_fee_bps + slippage) * self.stress_multiplier
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolSwapExecution {
    pub oracle_price: f64,
    pub execution_price: f64,
    pub trade_notional_abs: f64,
    pub trade_quantity_sol: f64,
    pub fee_cost_usdc: f64,
    pub slippage_cost_usdc: f64,
    pub keeper_cost_usdc: f64,
    pub total_cost_usdc: f64,
    pub total_cost_bps: f64,
}

pub fn estimate_swap_execution(
    oracle_price: f64,
    trade_quantity_sol: f64,
    config: &SolSwapCostConfig,
) -> Result<SolSwapExecution, HedgedAutocallError> {
    config.validate()?;
    if !(oracle_price.is_finite() && oracle_price > 0.0) {
        return Err(HedgedAutocallError::InvalidPath(
            "oracle_price must be positive",
        ));
    }
    if !trade_quantity_sol.is_finite() {
        return Err(HedgedAutocallError::InvalidPath(
            "trade_quantity_sol must be finite",
        ));
    }

    let trade_notional_abs = trade_quantity_sol.abs() * oracle_price;
    let total_cost_bps = config.cost_bps(trade_notional_abs);
    let total_cost_usdc = trade_notional_abs * total_cost_bps / 10_000.0;
    let fee_cost_usdc =
        trade_notional_abs * config.base_fee_bps * config.stress_multiplier / 10_000.0;
    let slippage_cost_usdc = (total_cost_usdc - fee_cost_usdc).max(0.0);
    let keeper_cost_usdc = if trade_notional_abs > 0.0 {
        config.keeper_bounty_usdc
    } else {
        0.0
    };
    let signed_cost = if trade_quantity_sol >= 0.0 {
        total_cost_usdc
    } else {
        -total_cost_usdc
    };
    let execution_price = if trade_quantity_sol > 0.0 {
        oracle_price + signed_cost / trade_quantity_sol.abs()
    } else if trade_quantity_sol < 0.0 {
        oracle_price + signed_cost / trade_quantity_sol.abs()
    } else {
        oracle_price
    };

    Ok(SolSwapExecution {
        oracle_price,
        execution_price,
        trade_notional_abs,
        trade_quantity_sol,
        fee_cost_usdc,
        slippage_cost_usdc,
        keeper_cost_usdc,
        total_cost_usdc: total_cost_usdc + keeper_cost_usdc,
        total_cost_bps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_curve_grows_with_trade_size() {
        let config = SolSwapCostConfig::default();
        assert!(config.cost_bps(50_000.0) > config.cost_bps(10_000.0));
    }

    #[test]
    fn execution_cost_is_zero_for_zero_trade() {
        let execution = estimate_swap_execution(150.0, 0.0, &SolSwapCostConfig::default())
            .expect("zero trade should validate");
        assert_eq!(execution.trade_notional_abs, 0.0);
        assert_eq!(execution.total_cost_usdc, 0.0);
    }
}
