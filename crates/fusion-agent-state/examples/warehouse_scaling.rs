//! Warehouse scaling benchmark — prints T=10..200 table validating
//! bounded context, linear NanoUSD, and trajectory equivalence.

use fusion_agent_state::benchmark::{run_scaling_report, verify_trajectory_equivalence, check_bounded_context, check_linear_cumulative};

fn main() {
    let horizons = [10u64, 25, 50, 100, 150, 200];
    let reports = run_scaling_report(&horizons);

    println!("=== SKILL.state Warehouse Scaling ===");
    println!("{:<8} {:>10} {:>10} {:>14} {:>14} {:>14} {:>10} {:>10}",
        "Horizon", "StateOK", "S.FinalCtx", "H.FinalCtx", "State TotalTk", "History TotalTk", "S.NanoUSD", "H.NanoUSD");
    println!("{}", "-".repeat(96));
    for r in &reports {
        let s_last = r.state.last().unwrap();
        let h_last = r.history.last().unwrap();
        println!("{:<8} {:>10} {:>10} {:>14} {:>14} {:>14} {:>10} {:>10}",
            r.horizon,
            if r.state_success { "1.00" } else { "0.00" },
            s_last.context_tokens,
            h_last.context_tokens,
            s_last.total_tokens,
            h_last.total_tokens,
            s_last.total_cost_nanos,
            h_last.total_cost_nanos,
        );
    }

    println!("\n=== Invariant Checks ===");
    // Aggregate across all horizons for context/linerarity checks (use 200 horizon detailed)
    let r200 = reports.iter().find(|r| r.horizon == 200).unwrap();
    let (bounded_ok, bounded_msg) = check_bounded_context(&r200.state, &r200.history);
    println!("A. Context growth (T=200): {} — {}", if bounded_ok { "PASS" } else { "FAIL" }, bounded_msg);
    let (linear_ok, linear_msg) = check_linear_cumulative(&r200.state);
    println!("B. Linear cumulative (T=200): {} — {}", if linear_ok { "PASS" } else { "FAIL" }, linear_msg);

    // Trajectory equivalence
    println!("\nC. Trajectory equivalence (EventLog observational):");
    for &h in &horizons {
        let (ok, msg) = verify_trajectory_equivalence(h);
        println!("  T={:<3} {} — {}", h, if ok { "PASS" } else { "FAIL" }, msg);
    }

    // Summary statement
    println!("\n=== Summary ===");
    let s200 = reports.iter().find(|r| r.horizon == 200).unwrap().state.last().unwrap();
    let h200 = reports.iter().find(|r| r.horizon == 200).unwrap().history.last().unwrap();
    let ratio = h200.total_tokens as f64 / s200.total_tokens as f64;
    println!("State T=200: ctx {} tokens (bounded), total {} tokens, cost {} nanos", s200.context_tokens, s200.total_tokens, s200.total_cost_nanos);
    println!("History T=200: ctx {} tokens (grows), total {} tokens, cost {} nanos", h200.context_tokens, h200.total_tokens, h200.total_cost_nanos);
    println!("History/State total token ratio at T=200: {:.1}x", ratio);
    if bounded_ok && linear_ok {
        println!("\nValidated: execution context remains bounded while cumulative resource consumption grows linearly with state transitions.");
    } else {
        println!("\nInvariant violation — see above.");
        std::process::exit(1);
    }
}
