use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use anyhow::{ensure, Context, Result};
use halcyon_sol_autocall_quote::autocall_v2_e11::{
    generate_pod_deim_tables, live_quote_training_params, GeneratedDeimLeg, GeneratedPodDeimTables,
};
use sha2::{Digest, Sha256};

fn main() -> Result<()> {
    let params = live_quote_training_params();
    let tables = generate_pod_deim_tables(&params)
        .map_err(|err| anyhow::anyhow!("train POD-DEIM tables: {err:?}"))?;
    validate_shapes(&tables)?;

    let hash = table_sha256(&tables);
    let rendered = render_tables(&tables, &hash);
    let output_path = output_path()?;

    fs::write(&output_path, rendered)
        .with_context(|| format!("write {}", output_path.display()))?;

    let mut hash_hex = String::with_capacity(hash.len() * 2);
    for byte in hash {
        write!(&mut hash_hex, "{byte:02x}").expect("hex write");
    }
    println!("wrote {} sha256={hash_hex}", output_path.display());
    Ok(())
}

fn output_path() -> Result<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = root.join("crates/halcyon_sol_autocall_quote/src/generated/pod_deim_table.rs");
    let parent = output
        .parent()
        .context("generated POD-DEIM file has no parent directory")?;
    ensure!(parent.exists(), "missing output directory {}", parent.display());
    Ok(output)
}

fn validate_shapes(tables: &GeneratedPodDeimTables) -> Result<()> {
    let n = tables.training_params.n_states;
    let d = tables.training_params.d;
    let m = tables.training_params.m;

    ensure!(tables.grid_reps.len() == n, "grid_reps len mismatch");
    ensure!(tables.grid_bounds.len() == n - 1, "grid_bounds len mismatch");
    ensure!(tables.p_ref_at_eim.len() == m, "p_ref_at_eim len mismatch");
    ensure!(tables.b_inv.len() == m * m, "b_inv len mismatch");
    ensure!(tables.eim_rows.len() == m, "eim_rows len mismatch");
    ensure!(tables.eim_cols.len() == m, "eim_cols len mismatch");
    ensure!(tables.atoms_v.len() == m * d * d, "atoms_v len mismatch");
    ensure!(tables.atoms_u.len() == m * d * d, "atoms_u len mismatch");
    ensure!(tables.p_ref_red_v.len() == d * d, "p_ref_red_v len mismatch");
    ensure!(tables.p_ref_red_u.len() == d * d, "p_ref_red_u len mismatch");
    validate_leg(&tables.v_leg, n, d, "v_leg")?;
    validate_leg(&tables.u_leg, n, d, "u_leg")?;
    Ok(())
}

fn validate_leg(leg: &GeneratedDeimLeg, n: usize, d: usize, label: &str) -> Result<()> {
    ensure!(leg.d == d, "{label}.d mismatch");
    ensure!(leg.phi_at_idx.len() == d * d, "{label}.phi_at_idx len mismatch");
    ensure!(leg.pt_inv.len() == d * d, "{label}.pt_inv len mismatch");
    ensure!(leg.phi_atm.len() == d, "{label}.phi_atm len mismatch");
    ensure!(leg.m_ki_red.len() == d * d, "{label}.m_ki_red len mismatch");
    ensure!(leg.m_nki_red.len() == d * d, "{label}.m_nki_red len mismatch");
    ensure!(leg.ki_at_idx.len() == d, "{label}.ki_at_idx len mismatch");
    ensure!(leg.cpn_at_idx.len() == d, "{label}.cpn_at_idx len mismatch");
    ensure!(leg.ac_at_idx.len() == d, "{label}.ac_at_idx len mismatch");
    ensure!(leg.phi.len() == n * d, "{label}.phi len mismatch");
    Ok(())
}

fn table_sha256(tables: &GeneratedPodDeimTables) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hash_i64(&mut hasher, tables.training_params.alpha_6);
    hash_i64(&mut hasher, tables.training_params.beta_6);
    hash_i64(&mut hasher, tables.training_params.reference_step_days);
    hash_usize(&mut hasher, tables.training_params.n_obs);
    hash_usize(&mut hasher, tables.training_params.no_autocall_first_n_obs);
    hash_i64(&mut hasher, tables.training_params.knock_in_log_6);
    hash_i64(&mut hasher, tables.training_params.autocall_log_6);
    hash_usize(&mut hasher, tables.training_params.n_states);
    hash_usize(&mut hasher, tables.training_params.d);
    hash_usize(&mut hasher, tables.training_params.m);

    hash_usize(&mut hasher, tables.atm_state);
    hash_usize(&mut hasher, tables.ki_state_max);
    hash_usize(&mut hasher, tables.ki_boundary_idx);

    hash_i64_slice(&mut hasher, &tables.grid_reps);
    hash_i64_slice(&mut hasher, &tables.grid_bounds);
    hash_i64_slice(&mut hasher, &tables.p_ref_at_eim);
    hash_i64_slice(&mut hasher, &tables.b_inv);
    hash_u16_slice(&mut hasher, &tables.eim_rows);
    hash_u16_slice(&mut hasher, &tables.eim_cols);
    hash_i64_slice(&mut hasher, &tables.atoms_v);
    hash_i64_slice(&mut hasher, &tables.atoms_u);
    hash_i64_slice(&mut hasher, &tables.p_ref_red_v);
    hash_i64_slice(&mut hasher, &tables.p_ref_red_u);
    hash_leg(&mut hasher, &tables.v_leg);
    hash_leg(&mut hasher, &tables.u_leg);

    hasher.finalize().into()
}

fn hash_leg(hasher: &mut Sha256, leg: &GeneratedDeimLeg) {
    hash_usize(hasher, leg.d);
    hash_i64_slice(hasher, &leg.phi_at_idx);
    hash_i64_slice(hasher, &leg.pt_inv);
    hash_i64_slice(hasher, &leg.phi_atm);
    hash_i64_slice(hasher, &leg.m_ki_red);
    hash_i64_slice(hasher, &leg.m_nki_red);
    hash_bool_slice(hasher, &leg.ki_at_idx);
    hash_bool_slice(hasher, &leg.cpn_at_idx);
    hash_bool_slice(hasher, &leg.ac_at_idx);
    hash_i64_slice(hasher, &leg.phi);
}

fn hash_i64(hasher: &mut Sha256, value: i64) {
    hasher.update(value.to_le_bytes());
}

fn hash_usize(hasher: &mut Sha256, value: usize) {
    hasher.update((value as u64).to_le_bytes());
}

fn hash_i64_slice(hasher: &mut Sha256, values: &[i64]) {
    for &value in values {
        hash_i64(hasher, value);
    }
}

fn hash_u16_slice(hasher: &mut Sha256, values: &[u16]) {
    for &value in values {
        hasher.update(value.to_le_bytes());
    }
}

fn hash_bool_slice(hasher: &mut Sha256, values: &[bool]) {
    for &value in values {
        hasher.update([u8::from(value)]);
    }
}

fn render_tables(tables: &GeneratedPodDeimTables, hash: &[u8; 32]) -> String {
    let mut out = String::new();
    out.push_str("//! Generated by `cargo run -p halcyon_sol_autocall_quote_training`.\n");
    out.push_str("//! Regenerate this file from a clean checkout; do not edit by hand.\n\n");
    out.push_str("//! Reduced-basis/operator arrays are emitted at Q20 (`1 << 20`).\n");
    out.push_str("//! Grid geometry and training parameters remain at SCALE_6.\n\n");

    writeln!(
        &mut out,
        "pub const TRAINING_ALPHA_S6: i64 = {};",
        tables.training_params.alpha_6
    )
    .unwrap();
    writeln!(
        &mut out,
        "pub const TRAINING_BETA_S6: i64 = {};",
        tables.training_params.beta_6
    )
    .unwrap();
    writeln!(
        &mut out,
        "pub const TRAINING_REFERENCE_STEP_DAYS: i64 = {};",
        tables.training_params.reference_step_days
    )
    .unwrap();
    writeln!(
        &mut out,
        "pub const TRAINING_N_OBS: usize = {};",
        tables.training_params.n_obs
    )
    .unwrap();
    writeln!(
        &mut out,
        "pub const TRAINING_NO_AUTOCALL_FIRST_N_OBS: usize = {};",
        tables.training_params.no_autocall_first_n_obs
    )
    .unwrap();
    writeln!(
        &mut out,
        "pub const TRAINING_KNOCK_IN_LOG_6: i64 = {};",
        tables.training_params.knock_in_log_6
    )
    .unwrap();
    writeln!(
        &mut out,
        "pub const TRAINING_AUTOCALL_LOG_6: i64 = {};",
        tables.training_params.autocall_log_6
    )
    .unwrap();
    out.push('\n');

    writeln!(
        &mut out,
        "pub const N_STATES: usize = {};",
        tables.training_params.n_states
    )
    .unwrap();
    writeln!(&mut out, "pub const D: usize = {};", tables.training_params.d).unwrap();
    writeln!(&mut out, "pub const M: usize = {};", tables.training_params.m).unwrap();
    writeln!(&mut out, "pub const TABLE_SCALE_Q20: i64 = {};", 1i64 << 20).unwrap();
    out.push('\n');

    writeln!(&mut out, "pub const ATM_STATE: usize = {};", tables.atm_state).unwrap();
    writeln!(
        &mut out,
        "pub const KI_STATE_MAX: usize = {};",
        tables.ki_state_max
    )
    .unwrap();
    writeln!(
        &mut out,
        "pub const KI_BOUNDARY_IDX: usize = {};",
        tables.ki_boundary_idx
    )
    .unwrap();
    out.push('\n');

    write_u8_hex_array(&mut out, "POD_DEIM_TABLE_SHA256", "32", hash);
    write_i64_array(&mut out, "GRID_REPS", "N_STATES", &tables.grid_reps);
    write_i64_array(&mut out, "GRID_BOUNDS", "N_STATES - 1", &tables.grid_bounds);
    write_i64_array(&mut out, "P_REF_AT_EIM", "M", &tables.p_ref_at_eim);
    write_i64_array(&mut out, "B_INV", "M * M", &tables.b_inv);
    write_u16_array(&mut out, "EIM_ROWS", "M", &tables.eim_rows);
    write_u16_array(&mut out, "EIM_COLS", "M", &tables.eim_cols);
    write_i64_array(&mut out, "ATOMS_V", "M * D * D", &tables.atoms_v);
    write_i64_array(&mut out, "ATOMS_U", "M * D * D", &tables.atoms_u);
    write_i64_array(&mut out, "P_REF_RED_V", "D * D", &tables.p_ref_red_v);
    write_i64_array(&mut out, "P_REF_RED_U", "D * D", &tables.p_ref_red_u);

    write_leg(&mut out, "V", &tables.v_leg);
    write_leg(&mut out, "U", &tables.u_leg);

    out
}

fn write_leg(out: &mut String, prefix: &str, leg: &GeneratedDeimLeg) {
    write_i64_array(out, &format!("{prefix}_PHI_AT_IDX"), "D * D", &leg.phi_at_idx);
    write_i64_array(out, &format!("{prefix}_PT_INV"), "D * D", &leg.pt_inv);
    write_i64_array(out, &format!("{prefix}_PHI_ATM"), "D", &leg.phi_atm);
    write_i64_array(out, &format!("{prefix}_M_KI_RED"), "D * D", &leg.m_ki_red);
    write_i64_array(out, &format!("{prefix}_M_NKI_RED"), "D * D", &leg.m_nki_red);
    write_bool_array(out, &format!("{prefix}_KI_AT_IDX"), "D", &leg.ki_at_idx);
    write_bool_array(out, &format!("{prefix}_CPN_AT_IDX"), "D", &leg.cpn_at_idx);
    write_bool_array(out, &format!("{prefix}_AC_AT_IDX"), "D", &leg.ac_at_idx);
    write_i64_array(out, &format!("{prefix}_PHI"), "N_STATES * D", &leg.phi);
}

fn write_i64_array(out: &mut String, name: &str, len_expr: &str, values: &[i64]) {
    writeln!(out, "pub const {name}: [i64; {len_expr}] = [").unwrap();
    for chunk in values.chunks(8) {
        out.push_str("    ");
        for &value in chunk {
            write!(out, "{value}, ").unwrap();
        }
        out.push('\n');
    }
    out.push_str("];\n\n");
}

fn write_u16_array(out: &mut String, name: &str, len_expr: &str, values: &[u16]) {
    writeln!(out, "pub const {name}: [u16; {len_expr}] = [").unwrap();
    for chunk in values.chunks(12) {
        out.push_str("    ");
        for &value in chunk {
            write!(out, "{value}, ").unwrap();
        }
        out.push('\n');
    }
    out.push_str("];\n\n");
}

fn write_bool_array(out: &mut String, name: &str, len_expr: &str, values: &[bool]) {
    writeln!(out, "pub const {name}: [bool; {len_expr}] = [").unwrap();
    for chunk in values.chunks(12) {
        out.push_str("    ");
        for &value in chunk {
            write!(out, "{value}, ").unwrap();
        }
        out.push('\n');
    }
    out.push_str("];\n\n");
}

fn write_u8_hex_array(out: &mut String, name: &str, len_expr: &str, values: &[u8]) {
    writeln!(out, "pub const {name}: [u8; {len_expr}] = [").unwrap();
    for chunk in values.chunks(8) {
        out.push_str("    ");
        for &value in chunk {
            write!(out, "0x{value:02x}, ").unwrap();
        }
        out.push('\n');
    }
    out.push_str("];\n\n");
}
