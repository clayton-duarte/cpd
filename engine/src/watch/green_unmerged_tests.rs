//! The alert is a notification, never an automatic merge (commands §9.10a).

#[derive(Clone, Copy)]
struct PullRequest {
    owned: bool,
    open: bool,
    green: bool,
}

#[derive(Default)]
struct FakeServices {
    manager_alerts: usize,
    user_alerts: usize,
    merges: usize,
}

// Stub for the implementation Job: a tick observes a PR at an injected time and
// delivers through mocked services. It deliberately does nothing until implemented.
#[derive(Default)]
struct GreenAlert;

impl GreenAlert {
    fn tick(&mut self, _now_seconds: u64, _pr: PullRequest, _services: &mut FakeServices) {}
}

#[test]
fn green_unmerged_alert_after_thirty_minutes() {
    let mut alert = GreenAlert;
    let mut services = FakeServices::default();
    let green = PullRequest {
        owned: true,
        open: true,
        green: true,
    };
    let not_green = PullRequest {
        green: false,
        ..green
    };
    let closed = PullRequest {
        open: false,
        ..green
    };
    let unowned = PullRequest {
        owned: false,
        ..green
    };
    // Read the fixture fields here as well as passing them to the tick, so the
    // stub stays buildable under the workspace's deny-warnings quick check.
    assert!(green.owned && green.open && green.green);

    alert.tick(0, green, &mut services);
    alert.tick(1_799, green, &mut services);
    assert_eq!((services.manager_alerts, services.user_alerts), (0, 0));
    alert.tick(1_800, green, &mut services);
    assert_eq!(
        (services.manager_alerts, services.user_alerts),
        (1, 1),
        "both Manager and User are alerted at thirty continuous minutes"
    );
    alert.tick(3_600, green, &mut services);
    assert_eq!(
        (services.manager_alerts, services.user_alerts),
        (1, 1),
        "once per green period"
    );
    assert_eq!(
        services.merges, 0,
        "an alert never authorizes an automatic merge"
    );

    alert.tick(3_601, not_green, &mut services);
    alert.tick(3_602, green, &mut services);
    alert.tick(5_401, green, &mut services);
    assert_eq!((services.manager_alerts, services.user_alerts), (1, 1));
    alert.tick(5_402, green, &mut services);
    assert_eq!(
        (services.manager_alerts, services.user_alerts),
        (2, 2),
        "a new green period alerts again"
    );
    alert.tick(5_403, closed, &mut services);
    alert.tick(9_000, closed, &mut services);
    alert.tick(9_001, unowned, &mut services);
    alert.tick(12_601, unowned, &mut services);
    assert_eq!((services.manager_alerts, services.user_alerts), (2, 2));
    assert_eq!(services.merges, 0);
}
