use crate::bvn_cdf::bvn_cdf_gl20;
use crate::error::SolMathError;
use crate::SCALE_I;
use alloc::vec;
use alloc::vec::Vec;

const A_MIN_128: i128 = -4 * SCALE_I;
const A_MAX_128: i128 = 4 * SCALE_I;
const A_MIN_64: i64 = -4_000_000_000_000;
const A_MAX_64: i64 = 4_000_000_000_000;
const SCALE_I64: i64 = 1_000_000_000_000;
const RECIP_SHIFT: u32 = 48;

/// Precomputed bivariate normal CDF table at a fixed correlation.
///
/// Stores `Φ₂(a, b; ρ)` on a uniform 2D grid over `[-4, +4]²`.
/// Evaluation uses bilinear interpolation with division-free indexing.
///
/// Build offline with [`BvnTable::generate`], store in PDA via
/// [`BvnTable::to_bytes`], evaluate with [`BvnTable::eval`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BvnTable {
    rho: i64,
    n: usize,
    dx: i64,
    dx_recip: u64,
    values: Vec<i64>,
}

#[inline]
fn compute_dx_recip(dx: i64) -> u64 {
    (1u128 << RECIP_SHIFT).div_ceil(dx as u128) as u64
}

impl BvnTable {
    /// Generate the table offline using the high-precision GL20 bvn_cdf.
    pub fn generate(rho: i64, n: usize) -> Result<Self, SolMathError> {
        if n < 2 {
            return Err(SolMathError::DomainError);
        }
        let rho_128 = rho as i128;
        if rho_128.abs() > SCALE_I {
            return Err(SolMathError::DomainError);
        }
        let range = A_MAX_128 - A_MIN_128;
        let dx_128 = range / (n as i128 - 1);
        if dx_128 <= 0 {
            return Err(SolMathError::DomainError);
        }
        let dx = dx_128 as i64;
        let dx_recip = compute_dx_recip(dx);

        let mut values = vec![0i64; n * n];
        for i in 0..n {
            let a = A_MIN_128 + (i as i128) * dx_128;
            for j in 0..n {
                let b = A_MIN_128 + (j as i128) * dx_128;
                values[i * n + j] = bvn_cdf_gl20(a, b, rho_128)? as i64;
            }
        }

        Ok(BvnTable {
            rho,
            n,
            dx,
            dx_recip,
            values,
        })
    }

    /// Evaluate via bilinear interpolation. i64 in, i64 out.
    pub fn eval(&self, a: i64, b: i64) -> Result<i64, SolMathError> {
        let a_c = a.clamp(A_MIN_64, A_MAX_64);
        let b_c = b.clamp(A_MIN_64, A_MAX_64);

        let a_off = (a_c - A_MIN_64) as u64;
        let b_off = (b_c - A_MIN_64) as u64;
        let recip = self.dx_recip as u128;
        let n_max = (self.n - 2) as u128;
        let dx = self.dx as u64;

        let full_a = a_off as u128 * recip;
        let mut i = (full_a >> RECIP_SHIFT).min(n_max) as usize;
        let full_b = b_off as u128 * recip;
        let mut j = (full_b >> RECIP_SHIFT).min(n_max) as usize;

        // panic guard: usize underflow to 0 is correct here
        if (i as u64) * dx > a_off {
            i = i.saturating_sub(1);
        }
        if (j as u64) * dx > b_off {
            j = j.saturating_sub(1);
        }

        let a_frac = a_off - (i as u64) * dx;
        let b_frac = b_off - (j as u64) * dx;

        let ta = ((a_frac as u128 * recip * SCALE_I64 as u128) >> RECIP_SHIFT) as i64;
        let tb = ((b_frac as u128 * recip * SCALE_I64 as u128) >> RECIP_SHIFT) as i64;
        let ta = ta.min(SCALE_I64);
        let tb = tb.min(SCALE_I64);
        let one_minus_ta = SCALE_I64 - ta;
        let one_minus_tb = SCALE_I64 - tb;

        let n = self.n;
        let v00 = self.values[i * n + j];
        let v10 = self.values[(i + 1) * n + j];
        let v01 = self.values[i * n + (j + 1)];
        let v11 = self.values[(i + 1) * n + (j + 1)];

        let s = SCALE_I64 as i128;
        let v_lo = (one_minus_tb as i128 * v00 as i128 + tb as i128 * v01 as i128) / s;
        let v_hi = (one_minus_tb as i128 * v10 as i128 + tb as i128 * v11 as i128) / s;
        let result = (one_minus_ta as i128 * v_lo + ta as i128 * v_hi) / s;

        Ok((result as i64).clamp(0, SCALE_I64))
    }

    pub fn rho(&self) -> i64 {
        self.rho
    }

    pub fn grid_size(&self) -> usize {
        self.n
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let header = 28usize;
        let body = self.n * self.n * 8;
        let mut buf = vec![0u8; header + body];
        buf[0..8].copy_from_slice(&self.rho.to_le_bytes());
        buf[8..12].copy_from_slice(&(self.n as u32).to_le_bytes());
        buf[12..20].copy_from_slice(&self.dx.to_le_bytes());
        for (idx, &val) in self.values.iter().enumerate() {
            let offset = header + idx * 8;
            buf[offset..offset + 8].copy_from_slice(&val.to_le_bytes());
        }
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, SolMathError> {
        if buf.len() < 28 {
            return Err(SolMathError::DomainError);
        }
        let rho = i64::from_le_bytes(
            buf[0..8]
                .try_into()
                .map_err(|_| SolMathError::DomainError)?,
        );
        let n = u32::from_le_bytes(
            buf[8..12]
                .try_into()
                .map_err(|_| SolMathError::DomainError)?,
        ) as usize;
        let dx = i64::from_le_bytes(
            buf[12..20]
                .try_into()
                .map_err(|_| SolMathError::DomainError)?,
        );
        if n < 2 || dx <= 0 {
            return Err(SolMathError::DomainError);
        }
        let expected = 28 + n * n * 8;
        if buf.len() < expected {
            return Err(SolMathError::DomainError);
        }
        let dx_recip = compute_dx_recip(dx);
        let mut values = vec![0i64; n * n];
        for (idx, slot) in values.iter_mut().enumerate() {
            let offset = 28 + idx * 8;
            *slot = i64::from_le_bytes(
                buf[offset..offset + 8]
                    .try_into()
                    .map_err(|_| SolMathError::DomainError)?,
            );
        }
        Ok(BvnTable {
            rho,
            n,
            dx,
            dx_recip,
            values,
        })
    }

    pub fn byte_size(&self) -> usize {
        28 + self.n * self.n * 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bvn_cdf::bvn_cdf;
    use crate::SCALE;

    #[test]
    fn bvn_table_accuracy_sweep() {
        let rhos = [0.0, 0.3, -0.5, 0.8];
        let grid_sizes: [usize; 3] = [64, 128, 256];

        for &rho_f in &rhos {
            let rho_fp = (rho_f * SCALE as f64).round() as i64;
            for &n in &grid_sizes {
                let table = BvnTable::generate(rho_fp, n).expect("generate");
                let mut max_abs_err = 0.0f64;
                let num_test = 80;

                for ia in 0..num_test {
                    let a_f = -3.5 + 7.0 * ia as f64 / (num_test - 1) as f64;
                    let a_fp = (a_f * SCALE as f64).round() as i64;
                    for ib in 0..num_test {
                        let b_f = -3.5 + 7.0 * ib as f64 / (num_test - 1) as f64;
                        let b_fp = (b_f * SCALE as f64).round() as i64;

                        let tbl = table.eval(a_fp, b_fp).expect("eval") as f64 / SCALE as f64;
                        let ana =
                            bvn_cdf(a_fp, b_fp, rho_fp).expect("analytic") as f64 / SCALE as f64;
                        let err = (tbl - ana).abs();
                        if err > max_abs_err {
                            max_abs_err = err;
                        }
                    }
                }
                std::eprintln!(
                    "rho={rho_f:+.4} n={n:3}: max_abs={max_abs_err:.2e} pda={}",
                    table.byte_size()
                );
                assert!(max_abs_err < 0.01);
            }
        }
    }

    #[test]
    fn bvn_table_serialize_roundtrip() {
        let table = BvnTable::generate((0.3e12) as i64, 16).expect("generate");
        let bytes = table.to_bytes();
        let restored = BvnTable::from_bytes(&bytes).expect("deserialize");
        assert_eq!(table, restored);
    }

    #[test]
    fn bvn_table_clamp_extremes() {
        let table = BvnTable::generate(0, 64).expect("generate");
        assert!(table.eval(-10 * SCALE_I64, -10 * SCALE_I64).unwrap() < SCALE_I64 / 1_000);
        assert!(
            table.eval(10 * SCALE_I64, 10 * SCALE_I64).unwrap() > SCALE_I64 - SCALE_I64 / 1_000
        );
    }
}
