//! The Jira title of a Workflow (contract §7.6 "The Jira title", pro-d399): `<type>[!]: <KEY>/<summary>`.

/// The title with `key` composed in after `<type>[!]: `, or unchanged when it already holds `key`; `None` when it cannot conform.
pub fn conform(_title: &str, _key: &str) -> Option<String> {
    None
}
