mod contract_types {
    use std::path::PathBuf;

    #[test]
    fn generated_types_match_core_contracts() {
        // Contract type generation routine for cpd core contracts.
        // In red/stub state: calls the generation routine (stubbed or checked) and verifies:
        // 1. web/src/generated/types.ts is produced
        // 2. Contains definitions for AgentKey, Phase, Urgency, JobKind, and PlanPhase
        // 3. Matches the checked-in file without diff

        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .to_path_buf();
        let generated_file = workspace_root.join("web/src/generated/types.ts");

        // Stub: contract type generation routine is not yet implemented.
        // When implemented, generate_contract_types(&workspace_root) or similar exports
        // types using ts-rs to web/src/generated/types.ts or returns the generated string.
        fn generate_contract_types(_root: &std::path::Path) -> Result<String, &'static str> {
            Err("ts-rs derives and contract generator not yet implemented")
        }

        let generated_content = generate_contract_types(&workspace_root)
            .expect("contract type generation routine should succeed");

        assert!(
            generated_file.exists(),
            "web/src/generated/types.ts must be produced at {}",
            generated_file.display()
        );

        let required_types = ["AgentKey", "Phase", "Urgency", "JobKind", "PlanPhase"];
        for ty in required_types {
            let pattern_type = format!("type {ty}");
            let pattern_interface = format!("interface {ty}");
            assert!(
                generated_content.contains(&pattern_type)
                    || generated_content.contains(&pattern_interface),
                "generated types must contain definition for {ty}"
            );
        }

        let checked_in_content = std::fs::read_to_string(&generated_file).unwrap_or_else(|_| {
            panic!(
                "could not read checked-in file at {}",
                generated_file.display()
            )
        });

        assert_eq!(
            generated_content, checked_in_content,
            "generated types must match checked-in web/src/generated/types.ts without diff. Run 'pnpm generate:types' to update."
        );
    }
}
