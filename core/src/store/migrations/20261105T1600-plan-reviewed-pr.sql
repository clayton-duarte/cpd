-- cpd-migration: additive
-- plan-reviewed-pr (pro-d412): the pull request a review Plan reviews, written together by `inbox claim`; all null or all set.
ALTER TABLE plans ADD COLUMN reviewed_repo TEXT CHECK (reviewed_repo IS NULL OR reviewed_repo LIKE '_%/_%');
ALTER TABLE plans ADD COLUMN reviewed_number INTEGER CHECK (reviewed_number IS NULL OR reviewed_number >= 1);
ALTER TABLE plans ADD COLUMN reviewed_title TEXT;
