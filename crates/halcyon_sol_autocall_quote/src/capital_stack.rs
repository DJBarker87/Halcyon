use crate::autocall_hedged::HedgedAutocallError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapitalArchitecture {
    SeparateSleeves,
    SharedSleeves,
    UnderwritingFundedHedge,
}

impl CapitalArchitecture {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SeparateSleeves => "separate_sleeves",
            Self::SharedSleeves => "shared_sleeves",
            Self::UnderwritingFundedHedge => "underwriting_funded_hedge",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapitalStackSnapshot {
    pub buyer_notional: f64,
    pub coupon_vault_cash: f64,
    pub hedge_sleeve_inventory_sol: f64,
    pub hedge_sleeve_cash_usdc: f64,
    pub accrued_unpaid_coupons: f64,
    pub issuer_margin_usdc: f64,
    pub underwriting_reserve: f64,
    pub reserve_occupancy: f64,
    pub peak_committed_capital: f64,
    pub worst_decile_note_pnl: f64,
    pub shortfall_flag: bool,
    pub insolvency_flag: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapitalStack {
    pub architecture: CapitalArchitecture,
    pub buyer_notional: f64,
    pub coupon_vault_cash: f64,
    pub hedge_sleeve_inventory_sol: f64,
    pub hedge_sleeve_cash_usdc: f64,
    pub accrued_unpaid_coupons: f64,
    pub issuer_margin_usdc: f64,
    pub underwriting_reserve: f64,
    pub peak_committed_capital: f64,
    pub min_combined_liquidity: f64,
}

impl CapitalStack {
    pub fn new(
        architecture: CapitalArchitecture,
        buyer_notional: f64,
        underwriting_reserve: f64,
        issuer_margin_usdc: f64,
    ) -> Result<Self, HedgedAutocallError> {
        if !(buyer_notional.is_finite() && buyer_notional > 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "buyer_notional must be positive",
            ));
        }
        if !(underwriting_reserve.is_finite() && underwriting_reserve >= 0.0) {
            return Err(HedgedAutocallError::InvalidTerms(
                "underwriting_reserve must be non-negative",
            ));
        }
        if !issuer_margin_usdc.is_finite() {
            return Err(HedgedAutocallError::InvalidTerms(
                "issuer_margin_usdc must be finite",
            ));
        }
        let mut stack = Self {
            architecture,
            buyer_notional,
            coupon_vault_cash: underwriting_reserve,
            hedge_sleeve_inventory_sol: 0.0,
            hedge_sleeve_cash_usdc: 0.0,
            accrued_unpaid_coupons: 0.0,
            issuer_margin_usdc,
            underwriting_reserve,
            peak_committed_capital: underwriting_reserve,
            min_combined_liquidity: underwriting_reserve,
        };
        stack.refresh_liquidity();
        Ok(stack)
    }

    pub fn set_hedge_position(&mut self, inventory_sol: f64, cash_usdc: f64) {
        self.hedge_sleeve_inventory_sol = inventory_sol;
        self.hedge_sleeve_cash_usdc = cash_usdc;
        self.refresh_liquidity();
    }

    pub fn accrue_coupon(&mut self, coupon_usdc: f64) {
        self.accrued_unpaid_coupons += coupon_usdc;
        self.coupon_vault_cash -= coupon_usdc;
        self.refresh_liquidity();
    }

    pub fn settle_retained_principal(&mut self, amount_usdc: f64) {
        self.coupon_vault_cash += amount_usdc;
        self.refresh_liquidity();
    }

    pub fn apply_hedge_execution_cost(&mut self, cash_delta_usdc: f64) {
        self.hedge_sleeve_cash_usdc += cash_delta_usdc;
        self.refresh_liquidity();
    }

    pub fn reserve_occupancy(&self) -> f64 {
        let hedge_draw = (-self.hedge_sleeve_cash_usdc).max(0.0);
        let coupon_draw = (-self.coupon_vault_cash).max(0.0);
        match self.architecture {
            CapitalArchitecture::SeparateSleeves => self.underwriting_reserve,
            CapitalArchitecture::SharedSleeves => {
                self.underwriting_reserve + hedge_draw + coupon_draw
            }
            CapitalArchitecture::UnderwritingFundedHedge => {
                self.underwriting_reserve + hedge_draw + coupon_draw
            }
        }
    }

    pub fn shortfall_flag(&self) -> bool {
        self.coupon_vault_cash < 0.0
            || matches!(
                self.architecture,
                CapitalArchitecture::SharedSleeves | CapitalArchitecture::UnderwritingFundedHedge
            ) && self.hedge_sleeve_cash_usdc < 0.0
    }

    pub fn insolvency_flag(&self) -> bool {
        match self.architecture {
            CapitalArchitecture::SeparateSleeves => {
                self.coupon_vault_cash + self.issuer_margin_usdc < -1e-9
            }
            CapitalArchitecture::SharedSleeves | CapitalArchitecture::UnderwritingFundedHedge => {
                self.min_combined_liquidity < -1e-9
            }
        }
    }

    pub fn snapshot(&self, worst_decile_note_pnl: f64) -> CapitalStackSnapshot {
        CapitalStackSnapshot {
            buyer_notional: self.buyer_notional,
            coupon_vault_cash: self.coupon_vault_cash,
            hedge_sleeve_inventory_sol: self.hedge_sleeve_inventory_sol,
            hedge_sleeve_cash_usdc: self.hedge_sleeve_cash_usdc,
            accrued_unpaid_coupons: self.accrued_unpaid_coupons,
            issuer_margin_usdc: self.issuer_margin_usdc,
            underwriting_reserve: self.underwriting_reserve,
            reserve_occupancy: self.reserve_occupancy(),
            peak_committed_capital: self.peak_committed_capital,
            worst_decile_note_pnl,
            shortfall_flag: self.shortfall_flag(),
            insolvency_flag: self.insolvency_flag(),
        }
    }

    fn refresh_liquidity(&mut self) {
        let occupancy = self.reserve_occupancy();
        self.peak_committed_capital = self.peak_committed_capital.max(occupancy);
        let combined = match self.architecture {
            CapitalArchitecture::SeparateSleeves => {
                self.coupon_vault_cash + self.issuer_margin_usdc
            }
            CapitalArchitecture::SharedSleeves | CapitalArchitecture::UnderwritingFundedHedge => {
                self.coupon_vault_cash + self.hedge_sleeve_cash_usdc + self.issuer_margin_usdc
            }
        };
        self.min_combined_liquidity = self.min_combined_liquidity.min(combined);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separate_sleeves_keep_underwriting_reserve_stable() {
        let mut stack = CapitalStack::new(CapitalArchitecture::SeparateSleeves, 1.0, 0.05, 0.005)
            .expect("stack");
        stack.accrue_coupon(0.02);
        stack.apply_hedge_execution_cost(-0.03);
        assert_eq!(stack.reserve_occupancy(), 0.05);
        assert!(!stack.shortfall_flag());
        assert!(!stack.insolvency_flag());
    }

    #[test]
    fn shared_sleeves_flag_insolvency_when_liquidity_goes_negative() {
        let mut stack =
            CapitalStack::new(CapitalArchitecture::SharedSleeves, 1.0, 0.05, 0.0).expect("stack");
        stack.accrue_coupon(0.04);
        stack.apply_hedge_execution_cost(-0.04);
        assert!(stack.shortfall_flag());
        assert!(stack.insolvency_flag());
    }
}
