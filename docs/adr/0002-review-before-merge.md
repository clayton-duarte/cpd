# Attempts are reviewed before they are merged, by a forked read-only Reviewer

An Attempt reaches the Lead Branch only after it passes the Gate and a Reviewer approves it, so unreviewed work never lands and Dependents always branch from approved commits. The Reviewer is a fork of the Lead session (sharing its understanding of the Requirements) restricted to read-only tools, rather than the Lead itself, so reviewing scales horizontally and the Lead stays free for the User.

## Considered Options

- Merge first, review after (original CPD draft): rejected because rejections then require reverts on the Lead Branch and Dependents may build on rejected work.
- Lead reviews every Attempt: rejected because it blocks the Lead and fills its context.
