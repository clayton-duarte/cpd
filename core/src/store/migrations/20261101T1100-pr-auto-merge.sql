-- cpd-migration: additive
-- The auto-merge request of a posted PR (pro-d115, pro-d117; commands.md §9.8b; contract §4.4): `auto_merge` is `none` (nothing
-- requested: a row from before this migration, or a repo that does not allow auto-merge), `pending` (to send or retry),
-- `enabled` or `refused`; `auto_merge_at` is set exactly when it is `enabled` or `refused`. Two `ALTER TABLE ... ADD COLUMN`
-- statements add the columns (existing rows become `none`, NULL); `ALTER TABLE` cannot add a table CHECK, so the rule that
-- ties the two columns is the CHECK of the `auto_merge` column itself.
ALTER TABLE pr_posts ADD COLUMN auto_merge_at TEXT;
ALTER TABLE pr_posts ADD COLUMN auto_merge TEXT NOT NULL DEFAULT 'none' CHECK (auto_merge IN ('none','pending','enabled','refused') AND ((auto_merge IN ('enabled','refused')) = (auto_merge_at IS NOT NULL)));
