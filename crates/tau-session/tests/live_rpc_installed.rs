//! Explicit installed-Pi canary; never runs in the default test suite.
#[path = "../examples/live_rpc_check.rs"]
#[allow(dead_code)]
mod live_rpc_check;

#[test]
#[ignore = "requires explicit TAU_LIVE_RPC_CHECK=1; real installed Pi, no model calls"]
fn installed_pi_controller_and_process_contract() {
    live_rpc_check::run_canary().unwrap_or_else(|category| panic!("{category}"));
}
