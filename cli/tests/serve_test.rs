//! Tests for `cpd serve` and UI-only preview proxy (docs/commands.md §12a, docs/contract.md §3.7b).

#[test]
fn serve_binds_webui_and_engine_on_port_3333() {
    cpd_core::serve::serve();
}

#[test]
fn preview_serves_ui_only_proxying_host_engine() {
    cpd_core::serve::preview();
}
