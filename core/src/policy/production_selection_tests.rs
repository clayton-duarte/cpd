        CapabilityError, PolicyKey, RoleSelection, RoleSelections, SelectionCapabilities,
        resolve_owner_role_selection,
    };
    use crate::home::CpdHome;
    use crate::ids::{OwnerLogin, RepoName};
    use std::collections::HashMap;
    use std::path::Path;

    struct FakePi(HashMap<(String, u8), Result<bool, CapabilityError>>);
    impl SelectionCapabilities for FakePi {
        fn supports(&self, model: &str, effort: u8) -> bool {
            self.evidence(model, effort) == Ok(true)
        }
        fn evidence(&self, model: &str, effort: u8) -> Result<bool, CapabilityError> {
            self.0
                .get(&(model.to_owned(), effort))
                .cloned()
                .unwrap_or(Err(CapabilityError::UnknownModel))
        }
    }

    fn roles(provider: &str, names: [&str; 3], efforts: [u8; 3]) -> RoleSelections {
        let role = |i: usize| RoleSelection {
            model: format!("{provider}/{}", names[i]),
            effort: efforts[i],
        };
        RoleSelections {
            manager: role(0),
            lead: role(1),
            worker: role(2),
        }
    }
    fn shared() -> RoleSelections {
        roles(
            "openai",
            ["gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-luna"],
            [2, 2, 0],
        )
    }
    fn fixture(r: &RoleSelections) -> String {
        let provider = r.manager.model.split('/').next().unwrap();
        format!(
            "[role_selection]\nprovider = \"{provider}\"\n[role_selection.manager]\nmodel = \"{}\"\neffort = {}\n[role_selection.lead]\nmodel = \"{}\"\neffort = {}\n[role_selection.worker]\nmodel = \"{}\"\neffort = {}\n",
            r.manager.model,
            r.manager.effort,
            r.lead.model,
            r.lead.effort,
            r.worker.model,
            r.worker.effort
        )
    }
    fn write(home: &CpdHome, path: &str, contents: &str) {
        let path = home.root().join("policy").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    fn home(path: &Path) -> CpdHome {
        CpdHome::resolve(Some(path.as_os_str().to_owned()), None).unwrap()
    }
    fn key(owner: &str, repo: &str) -> PolicyKey {
        PolicyKey::Repo(
            OwnerLogin::parse(owner).unwrap(),
            RepoName::parse(repo).unwrap(),
        )
    }
    fn fake_pi(sets: &[&RoleSelections]) -> FakePi {
        let mut entries = HashMap::new();
        for set in sets {
            for role in [&set.manager, &set.lead, &set.worker] {
                entries.insert((role.model.clone(), role.effort), Ok(true));
            }
        }
        FakePi(entries)
    }

    #[test]
    fn production_owner_shared_resolution_and_scope() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(dir.path());
        let default = shared();
        let changed = roles("other", ["manager", "lead", "worker"], [2, 1, 0]);
        let mission = roles("bifrost", ["manager", "lead", "worker"], [2, 1, 0]);
        let pi = fake_pi(&[&default, &changed, &mission]);
        write(&home, "default.toml", &fixture(&default));
        for repo in ["alpha", "beta"] {
            assert_eq!(
                resolve_owner_role_selection(&home, &key("acme", repo), &pi).unwrap(),
                default
            );
        }
        write(&home, "default.toml", &fixture(&changed));
        assert_eq!(
            resolve_owner_role_selection(&home, &key("acme", "alpha"), &pi).unwrap(),
            changed
        );
        assert!(resolve_owner_role_selection(&home, &key("missionlane", "alpha"), &pi).is_err());
        let incomplete = fixture(&mission)
            .split("[role_selection.worker]")
            .next()
            .unwrap()
            .to_owned();
        write(&home, "missionlane/owner.toml", &incomplete);
        assert!(resolve_owner_role_selection(&home, &key("missionlane", "alpha"), &pi).is_err());
        write(&home, "missionlane/owner.toml", &fixture(&mission));
        for repo in ["alpha", "beta"] {
            assert_eq!(
                resolve_owner_role_selection(&home, &key("missionlane", repo), &pi).unwrap(),
                mission
            );
        }
        write(&home, "host.toml", &fixture(&default));
        assert!(resolve_owner_role_selection(&home, &key("acme", "alpha"), &pi).is_err());
        std::fs::remove_file(home.root().join("policy/host.toml")).unwrap();
        write(&home, "acme/alpha.toml", &fixture(&default));
        assert!(resolve_owner_role_selection(&home, &key("acme", "alpha"), &pi).is_err());
    }

    #[test]
    fn production_fake_pi_capability_and_worker_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(dir.path());
        let default = shared();
        write(&home, "default.toml", &fixture(&default));
        let key = key("acme", "alpha");
        let mut pi = fake_pi(&[&default]);
        assert_eq!(
            resolve_owner_role_selection(&home, &key, &pi).unwrap(),
            default
        );
        let worker = default.worker.model.clone();
        pi.0.insert((worker.clone(), 0), Ok(false));
        pi.0.insert((worker.clone(), 1), Ok(true));
        let mut fallback = default.clone();
        fallback.worker.effort = 1;
        assert_eq!(
            resolve_owner_role_selection(&home, &key, &pi).unwrap(),
            fallback
        );
        pi.0.insert((worker.clone(), 0), Err(CapabilityError::Unavailable));
        assert!(resolve_owner_role_selection(&home, &key, &pi).is_err());
        pi.0.remove(&(worker.clone(), 0));
        assert!(resolve_owner_role_selection(&home, &key, &pi).is_err());
        pi.0.insert((worker.clone(), 0), Ok(false));
        pi.0.insert((worker, 1), Ok(false));
        assert!(resolve_owner_role_selection(&home, &key, &pi).is_err());
        let mut pi = fake_pi(&[&default]);
        pi.0.insert(
            (default.manager.model.clone(), default.manager.effort),
            Ok(false),
        );
        assert!(resolve_owner_role_selection(&home, &key, &pi).is_err());
        pi.0.insert(
            (default.manager.model.clone(), default.manager.effort),
            Err(CapabilityError::UnknownModel),
        );
        assert!(resolve_owner_role_selection(&home, &key, &pi).is_err());
    }
